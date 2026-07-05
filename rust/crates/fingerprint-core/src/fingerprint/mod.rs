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

/// Fingerprint overlapping windows of `samples`.
///
/// All windows are cut from a single short-time transform over the input: the
/// chroma frame at global index `j` covers samples `[j * HOP_SIZE,
/// j * HOP_SIZE + FRAME_SIZE)`, and a window hashes exactly the frames that
/// lie fully inside it. Each frame is therefore transformed once no matter
/// how much consecutive windows overlap, instead of once per window that
/// contains it, and the windowed streaming path slices the same global frame
/// grid, so one-shot and streaming windows agree for identical input.
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

    let starts: Vec<usize> = (0usize..)
        .map(|step| step * interval_samples)
        .take_while(|&start| start + window_samples <= samples.len())
        .collect();
    let Some(&last_start) = starts.last() else {
        return Ok(Vec::new());
    };

    // Frames past the last window's end are never hashed, so stop there.
    let needed_frames = (last_start + window_samples - FRAME_SIZE) / HOP_SIZE + 1;
    let frames = compute_chroma_frames(samples, needed_frames);

    let windows = starts
        .into_iter()
        .map(|start| {
            let first = start.div_ceil(HOP_SIZE);
            let last = (start + window_samples - FRAME_SIZE) / HOP_SIZE;
            let hashes = if first <= last {
                encode_chroma_frames(&frames[first..=last])
            } else {
                Vec::new()
            };
            WindowedFingerprint {
                timestamp_ms: duration_ms_for_samples(start),
                duration_ms: window_duration_ms,
                hashes,
            }
        })
        .collect();

    Ok(windows)
}

pub fn fingerprint_samples(samples: &[f32], duration_ms: u32) -> Fingerprint {
    let mut fft = FftProcessor::new(TARGET_SAMPLE_RATE);
    let frames = compute_chroma_frames_with(&mut fft, samples, usize::MAX);
    Fingerprint {
        hashes: encode_chroma_frames(&frames),
        duration_ms,
    }
}

/// Chroma frames at every complete `HOP_SIZE` offset of `samples`, capped at
/// `frame_limit` frames.
///
/// Frames are independent of each other, so the parallel and sequential
/// implementations produce identical output; the parallel path amortizes plan
/// construction across rayon's splits via `map_init` rather than building a
/// processor per frame.
#[cfg(feature = "parallel")]
fn compute_chroma_frames(samples: &[f32], frame_limit: usize) -> Vec<[f32; PITCH_CLASSES]> {
    use rayon::prelude::*;

    let available = complete_frame_count(samples.len()).min(frame_limit);
    (0..available)
        .into_par_iter()
        .map_init(
            || FftProcessor::new(TARGET_SAMPLE_RATE),
            |fft, index| {
                let offset = index * HOP_SIZE;
                fft.process_to_chroma(&samples[offset..offset + FRAME_SIZE])
            },
        )
        .collect()
}

#[cfg(not(feature = "parallel"))]
fn compute_chroma_frames(samples: &[f32], frame_limit: usize) -> Vec<[f32; PITCH_CLASSES]> {
    let mut fft = FftProcessor::new(TARGET_SAMPLE_RATE);
    compute_chroma_frames_with(&mut fft, samples, frame_limit)
}

fn compute_chroma_frames_with(
    fft: &mut FftProcessor,
    samples: &[f32],
    frame_limit: usize,
) -> Vec<[f32; PITCH_CLASSES]> {
    let available = complete_frame_count(samples.len()).min(frame_limit);
    (0..available)
        .map(|index| {
            let offset = index * HOP_SIZE;
            fft.process_to_chroma(&samples[offset..offset + FRAME_SIZE])
        })
        .collect()
}

fn complete_frame_count(sample_count: usize) -> usize {
    if sample_count < FRAME_SIZE {
        0
    } else {
        (sample_count - FRAME_SIZE) / HOP_SIZE + 1
    }
}

pub(crate) fn duration_ms_for_samples(samples: usize) -> u32 {
    ((samples as u128 * 1_000 + (TARGET_SAMPLE_RATE as u128 / 2)) / TARGET_SAMPLE_RATE as u128)
        .min(u32::MAX as u128) as u32
}
