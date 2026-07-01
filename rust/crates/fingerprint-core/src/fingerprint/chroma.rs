use crate::fingerprint::{
    A4_HZ, A4_PITCH_CLASS, MAX_CHROMA_FREQUENCY_HZ, MIN_CHROMA_FREQUENCY_HZ, PITCH_CLASSES,
};

/// Precomputed mapping from FFT bin index to pitch class.
///
/// The bin frequencies depend only on the (fixed) frame size and sample rate, so
/// the per-bin `log2`/modulo pitch assignment and the range test are computed
/// once at construction instead of on every frame. Accumulation still happens in
/// ascending bin order with the same arithmetic, so the resulting chroma vector
/// is bit-identical to computing it inline per frame.
pub struct ChromaMap {
    /// Pitch class for each FFT bin, or `None` when the bin is out of the chroma
    /// frequency band. Length is `frame_size / 2 + 1`.
    pitch_by_bin: Vec<Option<usize>>,
    /// Number of contributing bins per pitch class, used to average each bin.
    counts: [f32; PITCH_CLASSES],
}

impl ChromaMap {
    pub fn new(frame_size: usize, sample_rate: u32) -> Self {
        let bin_count = frame_size / 2 + 1;
        // Reconstruct the FFT length the original code derived from the magnitude
        // slice length so bin frequencies match exactly.
        let denominator = ((bin_count * 2).saturating_sub(2)).max(1);

        let mut pitch_by_bin = Vec::with_capacity(bin_count);
        let mut counts = [0.0; PITCH_CLASSES];
        for index in 0..bin_count {
            let frequency = (sample_rate as f32 / denominator as f32) * index as f32;
            if !(MIN_CHROMA_FREQUENCY_HZ..MAX_CHROMA_FREQUENCY_HZ).contains(&frequency) {
                pitch_by_bin.push(None);
                continue;
            }

            let mut raw_pitch = ((frequency / A4_HZ).log2() * 12.0 + A4_PITCH_CLASS) % 12.0;
            if raw_pitch < 0.0 {
                raw_pitch += 12.0;
            }
            let pitch = (raw_pitch as usize).min(PITCH_CLASSES - 1);
            pitch_by_bin.push(Some(pitch));
            counts[pitch] += 1.0;
        }

        Self {
            pitch_by_bin,
            counts,
        }
    }

    pub fn chroma(&self, magnitudes: &[f32]) -> [f32; PITCH_CLASSES] {
        let mut bins = [0.0; PITCH_CLASSES];
        for (index, magnitude) in magnitudes.iter().copied().enumerate() {
            if let Some(Some(pitch)) = self.pitch_by_bin.get(index) {
                bins[*pitch] += magnitude * magnitude;
            }
        }

        for (pitch, bin) in bins.iter_mut().enumerate() {
            if self.counts[pitch] > 0.0 {
                *bin /= self.counts[pitch];
            }
        }

        let norm = bins.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.000001 {
            for bin in &mut bins {
                *bin /= norm;
            }
        }

        bins
    }
}
