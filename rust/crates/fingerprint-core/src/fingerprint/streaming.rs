use std::collections::VecDeque;

use crate::audio::resampler::{
    resample_to_mono, samples_for_milliseconds, validate_audio_shape, TARGET_SAMPLE_RATE,
};
use crate::error::FingerprintError;
use crate::fingerprint::encoder::compute_hash;
use crate::fingerprint::fft::FftProcessor;
use crate::fingerprint::{
    duration_ms_for_samples, fingerprint_samples, FRAME_SIZE, HASH_FRAME_COUNT, HASH_STRIDE_FRAMES,
    HOP_SIZE, PITCH_CLASSES,
};

use super::WindowedFingerprint;

pub struct StreamingFingerprinter {
    sample_rate: u32,
    channels: u16,
    buffer: VecDeque<f32>,
    chroma_frames: VecDeque<[f32; PITCH_CLASSES]>,
    total_samples_at_target_rate: usize,
    fft: FftProcessor,
}

pub struct StreamingWindowedFingerprinter {
    sample_rate: u32,
    channels: u16,
    window_duration_ms: u32,
    window_interval_ms: u32,
    samples_at_target_rate: VecDeque<f32>,
    buffer_start_sample: usize,
    next_window_start: usize,
    total_samples_seen_at_target_rate: usize,
}

impl StreamingFingerprinter {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, FingerprintError> {
        validate_audio_shape(sample_rate, channels)?;
        Ok(Self {
            sample_rate,
            channels,
            buffer: VecDeque::new(),
            chroma_frames: VecDeque::new(),
            total_samples_at_target_rate: 0,
            fft: FftProcessor::new(TARGET_SAMPLE_RATE),
        })
    }

    pub fn duration_ms(&self) -> u32 {
        duration_ms_for_samples(self.total_samples_at_target_rate)
    }

    pub fn push_samples(&mut self, samples: &[i16]) -> Vec<u32> {
        let floats: Vec<f32> = samples
            .iter()
            .map(|sample| *sample as f32 / 32_768.0)
            .collect();
        self.push_interleaved_f32(&floats, self.channels)
    }

    pub fn push_samples_f32(&mut self, samples: &[f32], channels: u16) -> Vec<u32> {
        self.push_interleaved_f32(samples, channels)
    }

    pub fn flush(&mut self) -> Vec<u32> {
        self.emit_hashes()
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.chroma_frames.clear();
        self.total_samples_at_target_rate = 0;
    }

    fn push_interleaved_f32(&mut self, samples: &[f32], channels: u16) -> Vec<u32> {
        if channels == 0 || self.sample_rate == 0 {
            return Vec::new();
        }

        let mono = resample_to_mono(samples, self.sample_rate, channels);
        self.total_samples_at_target_rate =
            self.total_samples_at_target_rate.saturating_add(mono.len());
        self.buffer.extend(mono);
        self.process_buffer();
        self.emit_hashes()
    }

    fn process_buffer(&mut self) {
        while self.buffer.len() >= FRAME_SIZE {
            let frame: Vec<f32> = self.buffer.iter().take(FRAME_SIZE).copied().collect();
            self.chroma_frames
                .push_back(self.fft.process_to_chroma(&frame));
            for _ in 0..HOP_SIZE.min(self.buffer.len()) {
                self.buffer.pop_front();
            }
        }
    }

    fn emit_hashes(&mut self) -> Vec<u32> {
        let mut hashes = Vec::new();
        while self.chroma_frames.len() >= HASH_FRAME_COUNT {
            let frames: Vec<[f32; PITCH_CLASSES]> = self
                .chroma_frames
                .iter()
                .take(HASH_FRAME_COUNT)
                .copied()
                .collect();
            hashes.push(compute_hash(&frames));
            for _ in 0..HASH_STRIDE_FRAMES.min(self.chroma_frames.len()) {
                self.chroma_frames.pop_front();
            }
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
            sample_rate,
            channels,
            window_duration_ms,
            window_interval_ms,
            samples_at_target_rate: VecDeque::new(),
            buffer_start_sample: 0,
            next_window_start: 0,
            total_samples_seen_at_target_rate: 0,
        })
    }

    pub fn duration_ms(&self) -> u32 {
        duration_ms_for_samples(self.total_samples_seen_at_target_rate)
    }

    pub fn push_samples(&mut self, samples: &[i16]) -> Vec<WindowedFingerprint> {
        let floats: Vec<f32> = samples
            .iter()
            .map(|sample| *sample as f32 / 32_768.0)
            .collect();
        self.push_interleaved_f32(&floats, self.channels)
    }

    pub fn push_samples_f32(&mut self, samples: &[f32], channels: u16) -> Vec<WindowedFingerprint> {
        self.push_interleaved_f32(samples, channels)
    }

    pub fn flush(&mut self) -> Vec<WindowedFingerprint> {
        self.emit_windows()
    }

    pub fn reset(&mut self) {
        self.samples_at_target_rate.clear();
        self.buffer_start_sample = 0;
        self.next_window_start = 0;
        self.total_samples_seen_at_target_rate = 0;
    }

    fn push_interleaved_f32(&mut self, samples: &[f32], channels: u16) -> Vec<WindowedFingerprint> {
        if channels == 0 || self.sample_rate == 0 {
            return Vec::new();
        }

        let mono = resample_to_mono(samples, self.sample_rate, channels);
        self.total_samples_seen_at_target_rate = self
            .total_samples_seen_at_target_rate
            .saturating_add(mono.len());
        self.samples_at_target_rate.extend(mono);
        self.emit_windows()
    }

    fn emit_windows(&mut self) -> Vec<WindowedFingerprint> {
        let window_samples = samples_for_milliseconds(self.window_duration_ms);
        let interval_samples = samples_for_milliseconds(self.window_interval_ms);
        if window_samples < FRAME_SIZE || interval_samples == 0 {
            return Vec::new();
        }

        let mut windows = Vec::new();
        let available_end = self
            .buffer_start_sample
            .saturating_add(self.samples_at_target_rate.len());

        while self.next_window_start.saturating_add(window_samples) <= available_end {
            let relative_start = self.next_window_start - self.buffer_start_sample;
            let window: Vec<f32> = self
                .samples_at_target_rate
                .iter()
                .skip(relative_start)
                .take(window_samples)
                .copied()
                .collect();
            let timestamp_ms = duration_ms_for_samples(self.next_window_start);
            let hashes = fingerprint_samples(&window, self.window_duration_ms).hashes;
            windows.push(WindowedFingerprint {
                timestamp_ms,
                duration_ms: self.window_duration_ms,
                hashes,
            });
            self.next_window_start = self.next_window_start.saturating_add(interval_samples);
        }

        self.compact();
        windows
    }

    fn compact(&mut self) {
        if self.next_window_start <= self.buffer_start_sample {
            return;
        }

        let discard = (self.next_window_start - self.buffer_start_sample)
            .min(self.samples_at_target_rate.len());
        for _ in 0..discard {
            self.samples_at_target_rate.pop_front();
        }
        self.buffer_start_sample += discard;
    }
}
