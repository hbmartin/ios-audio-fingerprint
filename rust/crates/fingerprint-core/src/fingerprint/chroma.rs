use crate::fingerprint::{
    A4_HZ, A4_PITCH_CLASS, MAX_CHROMA_FREQUENCY_HZ, MIN_CHROMA_FREQUENCY_HZ, PITCH_CLASSES,
};

pub fn chroma_from_magnitudes(magnitudes: &[f32], sample_rate: u32) -> [f32; PITCH_CLASSES] {
    let mut bins = [0.0; PITCH_CLASSES];
    let mut counts = [0.0; PITCH_CLASSES];
    let denominator = ((magnitudes.len() * 2).saturating_sub(2)).max(1);

    for (index, magnitude) in magnitudes.iter().copied().enumerate() {
        let frequency = (sample_rate as f32 / denominator as f32) * index as f32;
        if !(MIN_CHROMA_FREQUENCY_HZ..MAX_CHROMA_FREQUENCY_HZ).contains(&frequency) {
            continue;
        }

        let mut raw_pitch = ((frequency / A4_HZ).log2() * 12.0 + A4_PITCH_CLASS) % 12.0;
        if raw_pitch < 0.0 {
            raw_pitch += 12.0;
        }
        let pitch = (raw_pitch as usize).min(PITCH_CLASSES - 1);
        bins[pitch] += magnitude * magnitude;
        counts[pitch] += 1.0;
    }

    for pitch in 0..PITCH_CLASSES {
        if counts[pitch] > 0.0 {
            bins[pitch] /= counts[pitch];
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
