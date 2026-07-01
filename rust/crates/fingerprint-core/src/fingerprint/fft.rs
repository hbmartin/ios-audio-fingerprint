use std::f32::consts::PI;
use std::sync::Arc;

use num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::fingerprint::chroma::chroma_from_magnitudes;
use crate::fingerprint::{FRAME_SIZE, PITCH_CLASSES};

pub struct FftProcessor {
    sample_rate: u32,
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    buffer: Vec<Complex<f32>>,
}

impl FftProcessor {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FRAME_SIZE);
        let hann = (0..FRAME_SIZE)
            .map(|index| 0.5 * (1.0 - (2.0 * PI * index as f32 / (FRAME_SIZE - 1) as f32).cos()))
            .collect();
        let buffer = vec![Complex::new(0.0, 0.0); FRAME_SIZE];

        Self {
            sample_rate,
            fft,
            hann,
            buffer,
        }
    }

    pub fn process_to_chroma(&mut self, frame: &[f32]) -> [f32; PITCH_CLASSES] {
        let magnitudes = self.magnitudes(frame);
        chroma_from_magnitudes(&magnitudes, self.sample_rate)
    }

    pub fn magnitudes(&mut self, frame: &[f32]) -> Vec<f32> {
        for index in 0..FRAME_SIZE {
            let real = frame.get(index).copied().unwrap_or(0.0) * self.hann[index];
            self.buffer[index] = Complex::new(real, 0.0);
        }

        self.fft.process(&mut self.buffer);
        self.buffer[..=FRAME_SIZE / 2]
            .iter()
            .map(|value| (value.re * value.re + value.im * value.im).sqrt())
            .collect()
    }
}
