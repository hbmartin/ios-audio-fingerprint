use std::f64::consts::PI;

use crate::error::FingerprintError;

pub const TARGET_SAMPLE_RATE: u32 = 11_025;

/// Number of quantized filter phases per source-sample step. Output positions
/// are snapped to this grid, which keeps coefficient lookup table-driven and
/// deterministic on every platform.
const PHASES: usize = 128;
/// Sinc zero crossings kept on each side of the filter's center. Six lobes of
/// a Blackman-windowed sinc leave the stopband fully formed well below the
/// frequencies that could fold into the chroma analysis band (whose ceiling,
/// `MAX_CHROMA_FREQUENCY_HZ` = 3,520 Hz, sits far under the 5,512 Hz target
/// Nyquist), so extra taps would buy no fingerprint accuracy.
const LOBES: f64 = 6.0;
/// Fraction of the ideal cutoff actually used, leaving room for the filter's
/// transition band below the folding frequency.
const ROLLOFF: f64 = 0.9;

pub fn samples_for_milliseconds(milliseconds: u32) -> usize {
    ((milliseconds as u64 * TARGET_SAMPLE_RATE as u64) / 1_000).min(usize::MAX as u64) as usize
}

pub fn validate_audio_shape(sample_rate: u32, channels: u16) -> Result<(), FingerprintError> {
    if sample_rate == 0 {
        return Err(FingerprintError::invalid(
            "sample rate must be greater than 0",
        ));
    }
    if channels == 0 {
        return Err(FingerprintError::invalid(
            "channel count must be greater than 0",
        ));
    }
    Ok(())
}

/// Average interleaved channels into a mono buffer without resampling.
pub(crate) fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channel_count = (channels as usize).max(1);
    if channel_count == 1 {
        return samples.to_vec();
    }
    let frame_count = samples.len() / channel_count;
    (0..frame_count)
        .map(|frame| {
            let base = frame * channel_count;
            let sum: f32 = samples[base..base + channel_count].iter().sum();
            sum / channel_count as f32
        })
        .collect()
}

/// Widen 16-bit PCM to floats and average interleaved channels in one pass, so
/// the streaming push path performs a single allocation instead of first
/// materializing a full-length interleaved float buffer.
pub(crate) fn downmix_i16_to_mono(samples: &[i16], channels: u16) -> Vec<f32> {
    const SCALE: f32 = 1.0 / 32_768.0;
    let channel_count = (channels as usize).max(1);
    if channel_count == 1 {
        return samples
            .iter()
            .map(|sample| *sample as f32 * SCALE)
            .collect();
    }
    let frame_count = samples.len() / channel_count;
    (0..frame_count)
        .map(|frame| {
            let base = frame * channel_count;
            let sum: f32 = samples[base..base + channel_count]
                .iter()
                .map(|sample| *sample as f32)
                .sum();
            sum * SCALE / channel_count as f32
        })
        .collect()
}

/// Downmix interleaved samples to mono and resample them to
/// [`TARGET_SAMPLE_RATE`] in one shot.
///
/// Resampling uses a windowed-sinc polyphase filter (see [`ResampleKernel`]),
/// which low-pass filters below the target Nyquist frequency before
/// decimation so out-of-band source content does not alias into the chroma
/// band. Samples outside the input are treated as zero, and the output length
/// is `floor(frame_count / (sample_rate / TARGET_SAMPLE_RATE))`, matching the
/// previous linear resampler's length contract.
pub fn resample_to_mono(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<f32> {
    let channel_count = (channels as usize).max(1);
    let frame_count = samples.len() / channel_count;
    if frame_count == 0 || sample_rate == 0 {
        return Vec::new();
    }

    let mono = downmix_to_mono(&samples[..frame_count * channel_count], channels);
    if sample_rate == TARGET_SAMPLE_RATE {
        return mono;
    }

    let kernel = ResampleKernel::new(sample_rate);
    let output_count = kernel.output_len(frame_count);
    (0..output_count)
        .map(|index| kernel.sample_at(&mono, index, 0))
        .collect()
}

/// Precomputed polyphase windowed-sinc filter bank for one source rate.
///
/// The kernel is built once per resampler (or per one-shot call) from the
/// source/target ratio alone: `PHASES` filter phases, each `taps` coefficients
/// long, Blackman-windowed and normalized to unity DC gain. Output sample `n`
/// is taken at source position `n * ratio` snapped to the phase grid, so the
/// same construction serves both the one-shot and the streaming paths and the
/// two produce identical values wherever both have full context.
struct ResampleKernel {
    ratio: f64,
    half_width: isize,
    taps: usize,
    /// `PHASES * taps` coefficients, phase-major.
    coefficients: Vec<f32>,
}

impl ResampleKernel {
    fn new(sample_rate: u32) -> Self {
        let ratio = sample_rate as f64 / TARGET_SAMPLE_RATE as f64;
        // Cutoff in cycles per *source* sample: at most the target Nyquist when
        // decimating, at most the source Nyquist when interpolating upward.
        let cutoff = 0.5 * (1.0 / ratio).min(1.0) * ROLLOFF;
        let half_width = (LOBES / (2.0 * cutoff)).ceil() as isize;
        let taps = (2 * half_width + 2) as usize;

        let mut coefficients = vec![0.0f32; PHASES * taps];
        for phase in 0..PHASES {
            let fractional = phase as f64 / PHASES as f64;
            let row = &mut coefficients[phase * taps..(phase + 1) * taps];
            let mut sum = 0.0f64;
            for (tap, value) in row.iter_mut().enumerate() {
                let t = tap as f64 - half_width as f64 - fractional;
                let windowed = windowed_sinc(t, cutoff, half_width as f64);
                *value = windowed as f32;
                sum += windowed;
            }
            if sum != 0.0 {
                let normalize = (1.0 / sum) as f32;
                for value in row.iter_mut() {
                    *value *= normalize;
                }
            }
        }

        Self {
            ratio,
            half_width,
            taps,
            coefficients,
        }
    }

    fn output_len(&self, frame_count: usize) -> usize {
        (frame_count as f64 / self.ratio).floor() as usize
    }

    /// Filtered value for output index `output`, reading `source` whose first
    /// element sits at absolute source index `source_start`. Source samples
    /// outside `source` are treated as zero.
    fn sample_at(&self, source: &[f32], output: usize, source_start: usize) -> f32 {
        let position = output as f64 * self.ratio;
        let base = position.floor() as isize;
        let fractional = position - base as f64;
        let phase = ((fractional * PHASES as f64) as usize).min(PHASES - 1);
        let row = &self.coefficients[phase * self.taps..(phase + 1) * self.taps];

        let first = base - self.half_width - source_start as isize;
        if first >= 0 && first as usize + self.taps <= source.len() {
            let window = &source[first as usize..first as usize + self.taps];
            return dot(row, window);
        }

        // Edge of the stream: gather the in-range samples into a zero-padded
        // window so the arithmetic (and thus the value) matches the fast path
        // exactly for whichever samples exist.
        let mut padded = vec![0.0f32; self.taps];
        for (tap, slot) in padded.iter_mut().enumerate() {
            let index = first + tap as isize;
            if index >= 0 {
                if let Some(sample) = source.get(index as usize) {
                    *slot = *sample;
                }
            }
        }
        dot(row, &padded)
    }

    /// Absolute source index of the last sample output `output` reads.
    fn last_source_index(&self, output: usize) -> isize {
        (output as f64 * self.ratio).floor() as isize + self.half_width + 1
    }

    /// Absolute source index of the first sample output `output` reads.
    fn first_source_index(&self, output: usize) -> isize {
        (output as f64 * self.ratio).floor() as isize - self.half_width
    }
}

/// Dot product with four independent accumulators.
///
/// The accumulation order is fixed by the code (four strided partial sums,
/// then one canonical reduction), so the result is deterministic on every
/// platform while still letting the compiler keep four multiply-add chains in
/// flight — the scalar-chain version was the resampler's bottleneck.
fn dot(coefficients: &[f32], window: &[f32]) -> f32 {
    let mut lanes = [0.0f32; 4];
    let mut coefficient_chunks = coefficients.chunks_exact(4);
    let mut window_chunks = window.chunks_exact(4);
    for (c, w) in (&mut coefficient_chunks).zip(&mut window_chunks) {
        lanes[0] += c[0] * w[0];
        lanes[1] += c[1] * w[1];
        lanes[2] += c[2] * w[2];
        lanes[3] += c[3] * w[3];
    }
    let mut tail = 0.0f32;
    for (c, w) in coefficient_chunks
        .remainder()
        .iter()
        .zip(window_chunks.remainder())
    {
        tail += c * w;
    }
    (lanes[0] + lanes[1]) + (lanes[2] + lanes[3]) + tail
}

fn windowed_sinc(t: f64, cutoff: f64, half_width: f64) -> f64 {
    if t.abs() > half_width {
        return 0.0;
    }
    let sinc = if t == 0.0 {
        1.0
    } else {
        let x = PI * 2.0 * cutoff * t;
        x.sin() / x
    };
    let progress = t / half_width;
    let window = 0.42 + 0.5 * (PI * progress).cos() + 0.08 * (2.0 * PI * progress).cos();
    sinc * window
}

/// Stateful wrapper around [`ResampleKernel`] for streaming input.
///
/// Unlike the previous per-push linear resampler, filter state carries across
/// pushes: chunk boundaries introduce no phase resets or edge artifacts, so a
/// signal streamed in arbitrary chunk sizes resamples to the same values as
/// the same signal resampled in one shot (except for the trailing
/// `half_width` source samples, which are only emitted once enough context
/// arrives). The small retained tail is never flushed with zero padding; a
/// stream that has truly ended simply forgoes the final < 1 ms of output.
pub(crate) struct StreamResampler {
    kernel: ResampleKernel,
    /// Source samples not yet fully consumed; `history[0]` has absolute source
    /// index `history_start`.
    history: Vec<f32>,
    history_start: usize,
    next_output: usize,
}

impl StreamResampler {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            kernel: ResampleKernel::new(sample_rate),
            history: Vec::new(),
            history_start: 0,
            next_output: 0,
        }
    }

    pub(crate) fn push(&mut self, mono: &[f32]) -> Vec<f32> {
        self.history.extend_from_slice(mono);
        let total = self.history_start + self.history.len();

        let mut output = Vec::new();
        while self.kernel.last_source_index(self.next_output) < total as isize {
            output.push(
                self.kernel
                    .sample_at(&self.history, self.next_output, self.history_start),
            );
            self.next_output += 1;
        }

        let keep_from = self.kernel.first_source_index(self.next_output).max(0) as usize;
        if keep_from > self.history_start {
            let discard = (keep_from - self.history_start).min(self.history.len());
            self.history.drain(..discard);
            self.history_start += discard;
        }

        output
    }

    pub(crate) fn reset(&mut self) {
        self.history.clear();
        self.history_start = 0;
        self.next_output = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(sample_rate: u32, seconds: f32, frequency: f32) -> Vec<f32> {
        let count = (sample_rate as f32 * seconds) as usize;
        (0..count)
            .map(|index| {
                (2.0 * std::f32::consts::PI * frequency * index as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    #[test]
    fn passthrough_at_target_rate_is_exact_downmix() {
        let stereo = vec![1.0, -1.0, 0.5, 0.25, -0.5, 0.5];
        assert_eq!(
            resample_to_mono(&stereo, TARGET_SAMPLE_RATE, 2),
            vec![0.0, 0.375, 0.0]
        );
    }

    #[test]
    fn output_length_matches_floor_contract() {
        let input = vec![0.0; 44_101];
        assert_eq!(resample_to_mono(&input, 44_100, 1).len(), 11_025);
        // 48_000 / (48_000 / 11_025) computes to just under 11_025 in f64; the
        // floor contract intentionally reproduces that arithmetic.
        let input = vec![0.0; 48_000];
        assert_eq!(resample_to_mono(&input, 48_000, 1).len(), 11_024);
        let input = vec![0.0; 7];
        assert_eq!(resample_to_mono(&input, 22_050, 1).len(), 3);
    }

    #[test]
    fn preserves_in_band_tone_amplitude() {
        // A 440 Hz tone sits far below the 5,512 Hz target Nyquist and must
        // survive 44.1 kHz -> 11.025 kHz with its amplitude nearly intact.
        let output = resample_to_mono(&sine(44_100, 1.0, 440.0), 44_100, 1);
        let interior = &output[500..output.len() - 500];
        let peak = interior.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(
            (0.97..=1.03).contains(&peak),
            "in-band peak drifted: {peak}"
        );
    }

    #[test]
    fn attenuates_alias_band_tone() {
        // 7 kHz lies above the 5,512.5 Hz target Nyquist: the old linear
        // resampler folded it back into band, the filtered resampler must
        // suppress it.
        let output = resample_to_mono(&sine(44_100, 1.0, 7_000.0), 44_100, 1);
        let interior = &output[500..output.len() - 500];
        let peak = interior.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(peak < 0.02, "alias-band tone leaked through: {peak}");
    }

    #[test]
    fn streaming_matches_one_shot_for_any_chunking() {
        let source = sine(44_100, 0.5, 440.0);
        let one_shot = resample_to_mono(&source, 44_100, 1);

        for chunk_size in [64usize, 1_000, 4_096, 22_050] {
            let mut resampler = StreamResampler::new(44_100);
            let mut streamed = Vec::new();
            for chunk in source.chunks(chunk_size) {
                streamed.extend(resampler.push(chunk));
            }
            assert!(!streamed.is_empty());
            assert!(streamed.len() <= one_shot.len());
            for (index, (a, b)) in streamed.iter().zip(&one_shot).enumerate() {
                assert!(
                    a == b,
                    "chunk {chunk_size}: sample {index} differs: {a} vs {b}"
                );
            }
        }
    }

    #[test]
    fn streaming_reset_restarts_the_stream() {
        let source = sine(44_100, 0.2, 440.0);
        let mut resampler = StreamResampler::new(44_100);
        let first = resampler.push(&source);
        resampler.reset();
        let second = resampler.push(&source);
        assert_eq!(first, second);
    }
}
