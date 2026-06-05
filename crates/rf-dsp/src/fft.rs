use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

/// Windowed FFT → calibrated power spectral density (dBFS).
///
/// Hardened from the v1 processor (`crates/_archive/rf-dsp-v1/src/fft.rs`):
/// - the Blackman–Harris window's **coherent gain is compensated**, so a
///   full-scale, bin-aligned complex tone reads ≈ 0 dBFS (v1 was off by a fixed
///   window-dependent offset);
/// - the **caller owns the output buffer** — no per-frame heap allocation on the
///   survey hot path.
pub struct FftProcessor {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    inv_window_sum: f32,
    fft_size: usize,
    buf: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl FftProcessor {
    pub fn new(fft_size: usize) -> Self {
        assert!(
            fft_size >= 8 && fft_size.is_power_of_two(),
            "fft_size must be a power of two >= 8"
        );
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];

        // 4-term Blackman–Harris, symmetric form (argument over N-1).
        let denom = (fft_size - 1) as f32;
        let window: Vec<f32> = (0..fft_size)
            .map(|n| {
                let x = 2.0 * PI * n as f32 / denom;
                0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
            })
            .collect();
        let window_sum: f32 = window.iter().sum();

        Self {
            fft,
            window,
            inv_window_sum: 1.0 / window_sum,
            fft_size,
            buf: vec![Complex32::new(0.0, 0.0); fft_size],
            scratch,
        }
    }

    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// One windowed PSD frame from exactly `fft_size` samples into `out`
    /// (resized to `fft_size`). dBFS, FFT-shifted so bin 0 is the lowest frequency.
    pub fn process_into(&mut self, iq: &[Complex32], out: &mut Vec<f32>) {
        assert_eq!(
            iq.len(),
            self.fft_size,
            "process_into needs exactly fft_size samples"
        );
        for ((dst, &s), &w) in self.buf.iter_mut().zip(iq).zip(&self.window) {
            *dst = s * w;
        }
        self.fft
            .process_with_scratch(&mut self.buf, &mut self.scratch);
        out.clear();
        out.resize(self.fft_size, 0.0);
        let half = self.fft_size / 2;
        for (i, slot) in out.iter_mut().enumerate() {
            let src = (i + half) % self.fft_size; // FFT shift: DC → center
            let amp = self.buf[src].norm() * self.inv_window_sum;
            *slot = if amp > 1e-12 {
                20.0 * amp.log10()
            } else {
                -240.0
            };
        }
    }

    /// Welch PSD over a long capture: average the linear power of overlapping
    /// windowed segments, then convert to dBFS. Returns one `fft_size`-bin frame.
    /// This is the survey PSD estimator — averaging buys the SNR that CFAR needs.
    pub fn welch_into(&mut self, iq: &[Complex32], overlap: f32, out: &mut Vec<f32>) {
        let n = self.fft_size;
        let overlap = overlap.clamp(0.0, 0.95);
        let hop = (((n as f32) * (1.0 - overlap)) as usize).max(1);
        let inv_ws_sq = (self.inv_window_sum * self.inv_window_sum) as f64;
        let half = n / 2;

        let mut accum = vec![0.0f64; n];
        let mut segs = 0usize;
        let mut start = 0usize;
        while start + n <= iq.len() {
            let seg = &iq[start..start + n];
            for ((dst, &s), &w) in self.buf.iter_mut().zip(seg).zip(&self.window) {
                *dst = s * w;
            }
            self.fft
                .process_with_scratch(&mut self.buf, &mut self.scratch);
            for (i, acc) in accum.iter_mut().enumerate() {
                let src = (i + half) % n;
                *acc += self.buf[src].norm_sqr() as f64 * inv_ws_sq;
            }
            segs += 1;
            start += hop;
        }

        out.clear();
        out.resize(n, 0.0);
        if segs == 0 {
            // capture shorter than one window: zero-pad to a single frame
            let mut v = iq.to_vec();
            v.resize(n, Complex32::new(0.0, 0.0));
            // reuse process_into on the padded copy
            let mut tmp = Vec::new();
            self.process_into(&v, &mut tmp);
            out.copy_from_slice(&tmp);
            return;
        }
        let inv = 1.0 / segs as f64;
        for (slot, &p) in out.iter_mut().zip(&accum) {
            let p = p * inv;
            *slot = if p > 1e-24 {
                (10.0 * p.log10()) as f32
            } else {
                -240.0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A full-scale, bin-aligned complex tone should read ≈ 0 dBFS after coherent-gain
    /// compensation. (v1 failed this — it was off by a window-dependent offset.)
    #[test]
    fn full_scale_tone_is_zero_dbfs() {
        let n = 1024;
        let mut p = FftProcessor::new(n);
        let k = 64usize; // bins above DC
        let iq: Vec<Complex32> = (0..n)
            .map(|i| {
                let ph = TAU * (k as f32) * (i as f32) / (n as f32);
                Complex32::new(ph.cos(), ph.sin())
            })
            .collect();
        let mut psd = Vec::new();
        p.process_into(&iq, &mut psd);
        let peak = psd.iter().cloned().fold(f32::MIN, f32::max);
        assert!((peak - 0.0).abs() < 0.5, "peak {peak} dBFS should be ~0");
        // peak lands at center+k after the FFT shift
        let peak_idx = psd
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(peak_idx, n / 2 + k);
    }

    #[test]
    fn welch_matches_single_for_one_segment() {
        let n = 256;
        let mut p = FftProcessor::new(n);
        let iq: Vec<Complex32> = (0..n)
            .map(|i| Complex32::new((i as f32 * 0.1).cos(), 0.0))
            .collect();
        let mut a = Vec::new();
        let mut b = Vec::new();
        p.process_into(&iq, &mut a);
        p.welch_into(&iq, 0.0, &mut b);
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-2);
        }
    }
}
