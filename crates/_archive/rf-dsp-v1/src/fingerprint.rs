//! RF fingerprint extraction — IQ-level emitter identification.
//!
//! Accumulates IQ statistics during squelch-open windows and produces
//! a fingerprint on squelch close: CFO, IQ imbalance, power stats.

use num_complex::Complex32;

/// Extracted RF fingerprint from a squelch-open window.
#[derive(Debug, Clone)]
pub struct RfFingerprint {
    /// Carrier frequency offset in Hz (NCO locked freq − nominal freq).
    pub cfo_hz: f64,
    /// IQ amplitude imbalance: |mean(I²) − mean(Q²)| / (mean(I²) + mean(Q²)).
    pub iq_amplitude_imbal: f64,
    /// IQ phase imbalance: normalized mean(I·Q).
    pub iq_phase_imbal: f64,
    /// Average power in dB over the window.
    pub avg_power_db: f64,
    /// Power variance (dB²).
    pub power_variance: f64,
    /// Number of IQ samples accumulated.
    pub sample_count: usize,
}

/// Running accumulator for fingerprint statistics.
/// Feed IQ samples while squelch is open, finalize on squelch close.
pub struct FingerprintAccumulator {
    sum_i2: f64,
    sum_q2: f64,
    sum_iq: f64,
    sum_power: f64,
    sum_power_sq: f64,
    cfo_sum: f64,
    cfo_count: u32,
    sample_count: usize,
}

impl FingerprintAccumulator {
    pub fn new() -> Self {
        Self {
            sum_i2: 0.0,
            sum_q2: 0.0,
            sum_iq: 0.0,
            sum_power: 0.0,
            sum_power_sq: 0.0,
            cfo_sum: 0.0,
            cfo_count: 0,
            sample_count: 0,
        }
    }

    /// Accumulate IQ statistics from a block of channel-filtered samples.
    pub fn feed_iq(&mut self, samples: &[Complex32]) {
        for s in samples {
            let i = s.re as f64;
            let q = s.im as f64;
            self.sum_i2 += i * i;
            self.sum_q2 += q * q;
            self.sum_iq += i * q;
            let pwr = i * i + q * q;
            self.sum_power += pwr;
            self.sum_power_sq += pwr * pwr;
        }
        self.sample_count += samples.len();
    }

    /// Accumulate a CFO measurement (Hz) from the NCO or PLL.
    pub fn feed_cfo(&mut self, cfo_hz: f64) {
        self.cfo_sum += cfo_hz;
        self.cfo_count += 1;
    }

    /// Finalize and produce a fingerprint. Returns None if too few samples.
    pub fn finalize(&self) -> Option<RfFingerprint> {
        if self.sample_count < 480 {
            // Need at least ~10ms at 48kHz
            return None;
        }

        let n = self.sample_count as f64;
        let mean_i2 = self.sum_i2 / n;
        let mean_q2 = self.sum_q2 / n;
        let mean_iq = self.sum_iq / n;
        let mean_power = self.sum_power / n;
        let mean_power_sq = self.sum_power_sq / n;

        let iq_sum = mean_i2 + mean_q2;
        let iq_amplitude_imbal = if iq_sum > 1e-12 {
            (mean_i2 - mean_q2).abs() / iq_sum
        } else {
            0.0
        };

        let iq_phase_imbal = if iq_sum > 1e-12 {
            mean_iq.abs() / iq_sum
        } else {
            0.0
        };

        let avg_power_db = if mean_power > 1e-20 {
            10.0 * mean_power.log10()
        } else {
            -200.0
        };

        let power_variance = (mean_power_sq - mean_power * mean_power).max(0.0);

        let cfo_hz = if self.cfo_count > 0 {
            self.cfo_sum / self.cfo_count as f64
        } else {
            0.0
        };

        Some(RfFingerprint {
            cfo_hz,
            iq_amplitude_imbal,
            iq_phase_imbal,
            avg_power_db,
            power_variance,
            sample_count: self.sample_count,
        })
    }

    /// Reset for a new accumulation window.
    pub fn reset(&mut self) {
        self.sum_i2 = 0.0;
        self.sum_q2 = 0.0;
        self.sum_iq = 0.0;
        self.sum_power = 0.0;
        self.sum_power_sq = 0.0;
        self.cfo_sum = 0.0;
        self.cfo_count = 0;
        self.sample_count = 0;
    }
}
