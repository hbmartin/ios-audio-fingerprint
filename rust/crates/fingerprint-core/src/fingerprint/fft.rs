use std::f32::consts::PI;
use std::sync::Arc;

use num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use crate::fingerprint::chroma::ChromaMap;
use crate::fingerprint::{FRAME_SIZE, PITCH_CLASSES};

pub struct FftProcessor {
    /// Real-to-complex transform: audio frames are real-valued, so the
    /// half-size real FFT does roughly half the work of the complex FFT the
    /// processor previously ran on a zero-imaginary buffer.
    fft: Arc<dyn RealToComplex<f32>>,
    hann: Vec<f32>,
    /// Windowed input frame (`FRAME_SIZE` reals), reused across calls.
    input: Vec<f32>,
    /// Spectrum output (`FRAME_SIZE / 2 + 1` bins), reused across calls.
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// Reused magnitude scratch (`FRAME_SIZE / 2 + 1` bins) so per-frame calls do
    /// not allocate.
    magnitudes: Vec<f32>,
    chroma_map: ChromaMap,
}

impl FftProcessor {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FRAME_SIZE);
        let hann = (0..FRAME_SIZE)
            .map(|index| 0.5 * (1.0 - (2.0 * PI * index as f32 / (FRAME_SIZE - 1) as f32).cos()))
            .collect();
        let input = fft.make_input_vec();
        let spectrum = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        let magnitudes = vec![0.0; FRAME_SIZE / 2 + 1];
        let chroma_map = ChromaMap::new(FRAME_SIZE, sample_rate);

        Self {
            fft,
            hann,
            input,
            spectrum,
            scratch,
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
            self.input[index] = frame.get(index).copied().unwrap_or(0.0) * self.hann[index];
        }

        self.fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .expect("real FFT buffers are sized by the plan");
        for (magnitude, value) in self.magnitudes.iter_mut().zip(self.spectrum.iter()) {
            *magnitude = (value.re * value.re + value.im * value.im).sqrt();
        }
    }
}
