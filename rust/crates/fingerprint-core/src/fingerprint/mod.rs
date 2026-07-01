pub mod chroma;
pub mod encoder;
pub mod fft;
pub mod streaming;

use crate::audio::decoder::decode_audio_bytes;
use crate::audio::resampler::{
    resample_to_mono, samples_for_milliseconds, validate_audio_shape, TARGET_SAMPLE_RATE,
};
use crate::error::FingerprintError;

use self::encoder::encode_chroma_frames;
use self::fft::FftProcessor;

pub const FRAME_SIZE: usize = 4_096;
pub const HOP_SIZE: usize = 1_024;
pub const HASH_FRAME_COUNT: usize = 8;
pub const HASH_STRIDE_FRAMES: usize = 2;
pub const PITCH_CLASSES: usize = 12;
pub const MIN_CHROMA_FREQUENCY_HZ: f32 = 28.0;
pub const MAX_CHROMA_FREQUENCY_HZ: f32 = 3_520.0;
pub const A4_HZ: f32 = 440.0;
pub const A4_PITCH_CLASS: f32 = 9.0;
pub const HASH_THRESHOLD: f32 = 0.05;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    pub hashes: Vec<u32>,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowedFingerprint {
    pub timestamp_ms: u32,
    pub duration_ms: u32,
    pub hashes: Vec<u32>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Fingerprinter;

impl Fingerprint {
    pub fn to_bytes(&self) -> Vec<u8> {
        crate::fingerprint_to_bytes(&self.hashes, self.duration_ms)
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        crate::fingerprint_from_bytes(data)
    }
}

impl Fingerprinter {
    pub fn new() -> Self {
        Self
    }

    pub fn fingerprint_data_windowed(
        &self,
        data: &[u8],
        window_duration_ms: u32,
        window_interval_ms: u32,
    ) -> Result<Vec<WindowedFingerprint>, FingerprintError> {
        let audio = decode_audio_bytes(data)?;
        validate_audio_shape(audio.sample_rate, audio.channels)?;
        let samples = resample_to_mono(&audio.samples, audio.sample_rate, audio.channels);
        fingerprint_windows(&samples, window_duration_ms, window_interval_ms)
    }
}

pub fn fingerprint_windows(
    samples: &[f32],
    window_duration_ms: u32,
    window_interval_ms: u32,
) -> Result<Vec<WindowedFingerprint>, FingerprintError> {
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
    if samples.len() < window_samples {
        return Ok(Vec::new());
    }

    let mut windows = Vec::new();
    let mut start = 0usize;
    while start + window_samples <= samples.len() {
        let window = &samples[start..start + window_samples];
        let fingerprint = fingerprint_samples(window, window_duration_ms);
        windows.push(WindowedFingerprint {
            timestamp_ms: duration_ms_for_samples(start),
            duration_ms: window_duration_ms,
            hashes: fingerprint.hashes,
        });
        start += interval_samples;
    }
    Ok(windows)
}

pub fn fingerprint_samples(samples: &[f32], duration_ms: u32) -> Fingerprint {
    let mut fft = FftProcessor::new(TARGET_SAMPLE_RATE);
    let mut chroma_frames = Vec::new();
    let mut offset = 0usize;
    while offset + FRAME_SIZE <= samples.len() {
        chroma_frames.push(fft.process_to_chroma(&samples[offset..offset + FRAME_SIZE]));
        offset += HOP_SIZE;
    }

    Fingerprint {
        hashes: encode_chroma_frames(&chroma_frames),
        duration_ms,
    }
}

pub(crate) fn duration_ms_for_samples(samples: usize) -> u32 {
    ((samples as u128 * 1_000 + (TARGET_SAMPLE_RATE as u128 / 2)) / TARGET_SAMPLE_RATE as u128)
        .min(u32::MAX as u128) as u32
}
