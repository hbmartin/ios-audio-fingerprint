use std::collections::VecDeque;

use crate::audio::resampler::{
    downmix_i16_to_mono, downmix_to_mono, samples_for_milliseconds, validate_audio_shape,
    StreamResampler, TARGET_SAMPLE_RATE,
};
use crate::error::FingerprintError;
use crate::fingerprint::encoder::compute_hash;
use crate::fingerprint::fft::FftProcessor;
use crate::fingerprint::{
    duration_ms_for_samples, encoder::encode_chroma_frames, FRAME_SIZE, HASH_FRAME_COUNT,
    HASH_STRIDE_FRAMES, HOP_SIZE, PITCH_CLASSES,
};

use super::WindowedFingerprint;

pub struct StreamingFingerprinter {
    channels: u16,
    /// `None` when the source is already at [`TARGET_SAMPLE_RATE`]; otherwise a
    /// stateful resampler whose filter context carries across pushes.
    resampler: Option<StreamResampler>,
    /// Target-rate samples not yet consumed by framing; the front sits on the
    /// global `HOP_SIZE` grid.
    pending: Vec<f32>,
    chroma_frames: VecDeque<[f32; PITCH_CLASSES]>,
    total_samples_at_target_rate: usize,
    fft: FftProcessor,
}

pub struct StreamingWindowedFingerprinter {
    channels: u16,
    window_duration_ms: u32,
    window_samples: usize,
    interval_samples: usize,
    resampler: Option<StreamResampler>,
    /// Target-rate samples awaiting framing; `pending[0]` has global sample
    /// index `frames_computed * HOP_SIZE`.
    pending: Vec<f32>,
    /// Retained chroma frames; `frames[0]` has global frame index
    /// `first_frame_index`. Storing frames instead of raw samples shrinks the
    /// buffered state from one window of PCM to `PITCH_CLASSES` floats per
    /// `HOP_SIZE` samples.
    frames: VecDeque<[f32; PITCH_CLASSES]>,
    first_frame_index: usize,
    frames_computed: usize,
    next_window_start: usize,
    total_samples_at_target_rate: usize,
    fft: FftProcessor,
}

impl StreamingFingerprinter {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, FingerprintError> {
        validate_audio_shape(sample_rate, channels)?;
        Ok(Self {
            channels,
            resampler: stream_resampler_for(sample_rate),
            pending: Vec::new(),
            chroma_frames: VecDeque::new(),
            total_samples_at_target_rate: 0,
            fft: FftProcessor::new(TARGET_SAMPLE_RATE),
        })
    }

    pub fn duration_ms(&self) -> u32 {
        duration_ms_for_samples(self.total_samples_at_target_rate)
    }

    pub fn push_samples(&mut self, samples: &[i16]) -> Vec<u32> {
        let mono = downmix_i16_to_mono(samples, self.channels);
        self.ingest(mono)
    }

    pub fn push_samples_f32(&mut self, samples: &[f32], channels: u16) -> Vec<u32> {
        if channels == 0 {
            return Vec::new();
        }
        let mono = downmix_to_mono(samples, channels);
        self.ingest(mono)
    }

    pub fn flush(&mut self) -> Vec<u32> {
        self.emit_hashes()
    }

    pub fn reset(&mut self) {
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        self.pending.clear();
        self.chroma_frames.clear();
        self.total_samples_at_target_rate = 0;
    }

    fn ingest(&mut self, mono: Vec<f32>) -> Vec<u32> {
        let at_target_rate = match &mut self.resampler {
            Some(resampler) => resampler.push(&mono),
            None => mono,
        };
        self.total_samples_at_target_rate = self
            .total_samples_at_target_rate
            .saturating_add(at_target_rate.len());
        self.pending.extend(at_target_rate);
        self.process_pending();
        self.emit_hashes()
    }

    fn process_pending(&mut self) {
        while self.pending.len() >= FRAME_SIZE {
            let chroma = self.fft.process_to_chroma(&self.pending[..FRAME_SIZE]);
            self.chroma_frames.push_back(chroma);
            self.pending.drain(..HOP_SIZE);
        }
    }

    fn emit_hashes(&mut self) -> Vec<u32> {
        let mut hashes = Vec::new();
        while self.chroma_frames.len() >= HASH_FRAME_COUNT {
            let (first, second) = self.chroma_frames.as_slices();
            let mut frames = [[0.0f32; PITCH_CLASSES]; HASH_FRAME_COUNT];
            if first.len() >= HASH_FRAME_COUNT {
                frames.copy_from_slice(&first[..HASH_FRAME_COUNT]);
            } else {
                frames[..first.len()].copy_from_slice(first);
                let remaining = HASH_FRAME_COUNT - first.len();
                frames[first.len()..].copy_from_slice(&second[..remaining]);
            }

            hashes.push(compute_hash(&frames));
            self.chroma_frames.drain(..HASH_STRIDE_FRAMES);
        }
        hashes
    }
}

impl StreamingWindowedFingerprinter {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        window_duration_ms: u32,
        window_interval_ms: u32,
    ) -> Result<Self, FingerprintError> {
        validate_audio_shape(sample_rate, channels)?;
        let window_samples = samples_for_milliseconds(window_duration_ms);
        let interval_samples = samples_for_milliseconds(window_interval_ms);
        if window_samples < FRAME_SIZE {
            return Err(FingerprintError::invalid(format!(
                "Window too short: {window_samples} samples, need at least {FRAME_SIZE}"
            )));
        }
        if interval_samples == 0 {
            return Err(FingerprintError::invalid(
                "Window interval must be greater than 0",
            ));
        }

        Ok(Self {
            channels,
            window_duration_ms,
            window_samples,
            interval_samples,
            resampler: stream_resampler_for(sample_rate),
            pending: Vec::new(),
            frames: VecDeque::new(),
            first_frame_index: 0,
            frames_computed: 0,
            next_window_start: 0,
            total_samples_at_target_rate: 0,
            fft: FftProcessor::new(TARGET_SAMPLE_RATE),
        })
    }

    pub fn duration_ms(&self) -> u32 {
        duration_ms_for_samples(self.total_samples_at_target_rate)
    }

    pub fn push_samples(&mut self, samples: &[i16]) -> Vec<WindowedFingerprint> {
        let mono = downmix_i16_to_mono(samples, self.channels);
        self.ingest(mono)
    }

    pub fn push_samples_f32(&mut self, samples: &[f32], channels: u16) -> Vec<WindowedFingerprint> {
        if channels == 0 {
            return Vec::new();
        }
        let mono = downmix_to_mono(samples, channels);
        self.ingest(mono)
    }

    pub fn flush(&mut self) -> Vec<WindowedFingerprint> {
        self.emit_windows()
    }

    pub fn reset(&mut self) {
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        self.pending.clear();
        self.frames.clear();
        self.first_frame_index = 0;
        self.frames_computed = 0;
        self.next_window_start = 0;
        self.total_samples_at_target_rate = 0;
    }

    fn ingest(&mut self, mono: Vec<f32>) -> Vec<WindowedFingerprint> {
        let at_target_rate = match &mut self.resampler {
            Some(resampler) => resampler.push(&mono),
            None => mono,
        };
        self.total_samples_at_target_rate = self
            .total_samples_at_target_rate
            .saturating_add(at_target_rate.len());
        self.pending.extend(at_target_rate);
        self.process_pending();
        self.emit_windows()
    }

    fn process_pending(&mut self) {
        while self.pending.len() >= FRAME_SIZE {
            let chroma = self.fft.process_to_chroma(&self.pending[..FRAME_SIZE]);
            self.frames.push_back(chroma);
            self.frames_computed += 1;
            self.pending.drain(..HOP_SIZE);
        }
    }

    /// Emit every window whose full sample span has arrived. Windows slice the
    /// same global frame grid as the one-shot path (`fingerprint_windows`), so
    /// the two produce identical hashes for identical input.
    fn emit_windows(&mut self) -> Vec<WindowedFingerprint> {
        let mut windows = Vec::new();
        while self.next_window_start.saturating_add(self.window_samples)
            <= self.total_samples_at_target_rate
        {
            let start = self.next_window_start;
            let first = start.div_ceil(HOP_SIZE);
            let last = (start + self.window_samples - FRAME_SIZE) / HOP_SIZE;
            let hashes = if first <= last {
                // Every needed frame exists: `last * HOP_SIZE + FRAME_SIZE`
                // lies within the received span, and frames below
                // `first_frame_index` were only discarded once no future
                // window could reference them.
                let offset = first - self.first_frame_index;
                let contiguous = self.frames.make_contiguous();
                encode_chroma_frames(&contiguous[offset..=last - self.first_frame_index])
            } else {
                Vec::new()
            };
            windows.push(WindowedFingerprint {
                timestamp_ms: duration_ms_for_samples(start),
                duration_ms: self.window_duration_ms,
                hashes,
            });
            self.next_window_start = self.next_window_start.saturating_add(self.interval_samples);
        }

        self.discard_unreachable_frames();
        windows
    }

    fn discard_unreachable_frames(&mut self) {
        let needed_first = self.next_window_start.div_ceil(HOP_SIZE);
        if needed_first > self.first_frame_index {
            let discard = (needed_first - self.first_frame_index).min(self.frames.len());
            self.frames.drain(..discard);
            self.first_frame_index += discard;
        }
    }
}

fn stream_resampler_for(sample_rate: u32) -> Option<StreamResampler> {
    if sample_rate == TARGET_SAMPLE_RATE {
        None
    } else {
        Some(StreamResampler::new(sample_rate))
    }
}
