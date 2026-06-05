use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

/// FFT processor with Blackman-Harris window.
pub struct FftProcessor {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    fft_size: usize,
    scratch: Vec<Complex32>,
}

impl FftProcessor {
    pub fn new(fft_size: usize) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);
        let scratch_len = fft.get_inplace_scratch_len();

        // Blackman-Harris window
        let window: Vec<f32> = (0..fft_size)
            .map(|n| {
                let x = 2.0 * PI * n as f32 / fft_size as f32;
                0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos()
                    - 0.01168 * (3.0 * x).cos()
            })
            .collect();

        Self {
            fft,
            window,
            fft_size,
            scratch: vec![Complex32::new(0.0, 0.0); scratch_len],
        }
    }

    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Process IQ samples → power spectral density in dBFS.
    /// Input must be exactly `fft_size` samples.
    /// Output is `fft_size` bins, DC-centered (FFT-shifted).
    pub fn process(&mut self, iq: &[Complex32]) -> Vec<f32> {
        assert_eq!(iq.len(), self.fft_size);

        // Apply window
        let mut buf: Vec<Complex32> = iq
            .iter()
            .zip(self.window.iter())
            .map(|(&s, &w)| s * w)
            .collect();

        // Forward FFT (in-place)
        self.fft.process_with_scratch(&mut buf, &mut self.scratch);

        // Magnitude squared → dBFS, with FFT shift
        let n = self.fft_size;
        let half = n / 2;
        let scale = 1.0 / (n as f32);

        let mut psd = vec![0.0f32; n];
        for i in 0..n {
            // FFT shift: move DC from index 0 to center
            let src = (i + half) % n;
            let mag_sq = buf[src].norm_sqr() * scale * scale;
            psd[i] = if mag_sq > 1e-20 {
                10.0 * mag_sq.log10()
            } else {
                -200.0
            };
        }

        psd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_dc_signal() {
        let mut proc = FftProcessor::new(256);
        // DC signal: all ones
        let iq: Vec<Complex32> = vec![Complex32::new(1.0, 0.0); 256];
        let psd = proc.process(&iq);
        // DC bin (center) should be the strongest
        let center = 128;
        let dc_power = psd[center];
        let edge_power = psd[0];
        assert!(dc_power > edge_power + 20.0, "DC should be dominant");
    }

    #[test]
    fn fft_size_check() {
        let proc = FftProcessor::new(2048);
        assert_eq!(proc.fft_size(), 2048);
    }
}
