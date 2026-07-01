use crate::error::FingerprintError;

pub const TARGET_SAMPLE_RATE: u32 = 11_025;

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

pub fn resample_to_mono(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<f32> {
    let channel_count = (channels as usize).max(1);
    let frame_count = samples.len() / channel_count;
    if frame_count == 0 {
        return Vec::new();
    }

    // Downmix a single source frame to mono on demand. Computing this inline lets
    // the resampling path avoid allocating a separate full-length mono buffer; the
    // arithmetic is identical to materializing mono first, so the output is
    // unchanged.
    let mono_frame = |frame: usize| -> f32 {
        if channel_count == 1 {
            samples[frame]
        } else {
            let base = frame * channel_count;
            let sum: f32 = samples[base..base + channel_count].iter().sum();
            sum / channel_count as f32
        }
    };

    if sample_rate == TARGET_SAMPLE_RATE {
        return (0..frame_count).map(mono_frame).collect();
    }
    if sample_rate == 0 {
        return Vec::new();
    }

    let ratio = sample_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let output_count = (frame_count as f64 / ratio).floor() as usize;
    if output_count == 0 {
        return Vec::new();
    }

    let mut output = vec![0.0; output_count];
    for (out_index, value) in output.iter_mut().enumerate() {
        let source_position = out_index as f64 * ratio;
        let source_index = source_position.floor() as usize;
        let fraction = (source_position - source_index as f64) as f32;

        *value = if source_index + 1 < frame_count {
            let current = mono_frame(source_index);
            let next = mono_frame(source_index + 1);
            current + (next - current) * fraction
        } else if source_index < frame_count {
            mono_frame(source_index)
        } else {
            0.0
        };
    }

    output
}
