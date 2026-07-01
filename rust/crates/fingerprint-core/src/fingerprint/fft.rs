use std::f32::consts::PI;
use std::sync::Arc;

use num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::fingerprint::chroma::ChromaMap;
use crate::fingerprint::{FRAME_SIZE, PITCH_CLASSES};

pub struct FftProcessor {
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    /// Reused magnitude scratch (`FRAME_SIZE / 2 + 1` bins) so per-frame calls do
    /// not allocate.
    magnitudes: Vec<f32>,
    chroma_map: ChromaMap,
}

impl FftProcessor {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FRAME_SIZE);
        let hann = (0..FRAME_SIZE)
            .map(|index| 0.5 * (1.0 - (2.0 * PI * index as f32 / (FRAME_SIZE - 1) as f32).cos()))
            .collect();
        let buffer = vec![Complex::new(0.0, 0.0); FRAME_SIZE];
        let magnitudes = vec![0.0; FRAME_SIZE / 2 + 1];
        let chroma_map = ChromaMap::new(FRAME_SIZE, sample_rate);

        Self {
            fft,
            hann,
            buffer,
            magnitudes,
            chroma_map,
        }
    }

    pub fn process_to_chroma(&mut self, frame: &[f32]) -> [f32; PITCH_CLASSES] {
        self.compute_magnitudes(frame);
        self.chroma_map.chroma(&self.magnitudes)
    }

    fn compute_magnitudes(&mut self, frame: &[f32]) {
        for index in 0..FRAME_SIZE {
            let real = frame.get(index).copied().unwrap_or(0.0) * self.hann[index];
            self.buffer[index] = Complex::new(real, 0.0);
        }

        self.fft.process(&mut self.buffer);
        for (magnitude, value) in self
            .magnitudes
            .iter_mut()
            .zip(self.buffer[..=FRAME_SIZE / 2].iter())
        {
            *magnitude = (value.re * value.re + value.im * value.im).sqrt();
        }
    }
}
