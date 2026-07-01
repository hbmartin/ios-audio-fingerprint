pub mod decoder;
pub mod resampler;
pub mod wav;

pub use decoder::decode_audio_bytes;
pub use resampler::{resample_to_mono, samples_for_milliseconds, TARGET_SAMPLE_RATE};

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}
