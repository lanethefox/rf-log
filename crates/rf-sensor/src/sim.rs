//! Simulated IQ sensor — synthesizes complex tones for a Portland-flavored signal
//! set, so the whole survey stack runs without hardware. Lifted/cleaned from the v1
//! `SimulatedSdr`; the external frequency JSON dependency is dropped in favor of a
//! built-in table.

use crate::{IqSensor, SensorError};
use num_complex::Complex32;
use rf_types::{Hz, SampleFormat, SensorCapabilities, SensorId};
use std::f64::consts::TAU;

/// Built-in synthetic emitters (Hz, relative amplitude) across the Portland VHF/UHF
/// landscape — enough that multiple survey tiles have content to detect.
const DEFAULT_SIGNALS: &[(f64, f32)] = &[
    (146_520_000.0, 0.040), // 2m calling
    (147_040_000.0, 0.035), // 2m repeater
    (154_430_000.0, 0.050), // PF&R
    (155_010_000.0, 0.060), // PPB
    (156_800_000.0, 0.030), // Marine 16
    (159_120_000.0, 0.035), // state
    (162_400_000.0, 0.080), // NOAA WX
    (162_550_000.0, 0.075), // NOAA WX
    (453_450_000.0, 0.050), // PF&R UHF
    (460_525_000.0, 0.045), // MCSO
    (462_562_500.0, 0.025), // FRS 1
    (465_000_000.0, 0.030), // itinerant
    (470_100_000.0, 0.035), // business
    (769_500_000.0, 0.055), // 700 MHz P25
    (855_237_500.0, 0.050), // 800 MHz trunk
];

struct SimSignal {
    freq_hz: f64,
    amplitude: f32,
}

/// A simulated SDR. Generates a noise floor plus any built-in emitters that fall
/// within `±sample_rate/2` of the currently tuned center.
pub struct SimSensor {
    id: SensorId,
    caps: SensorCapabilities,
    sample_rate: f64,
    center_hz: f64,
    signals: Vec<SimSignal>,
    sample_count: u64,
    noise_amp: f32,
}

impl SimSensor {
    /// A sensor covering `[freq_min, freq_max]` at `sample_rate`, seeded with the
    /// built-in signal table.
    pub fn new(id: SensorId, freq_min_hz: Hz, freq_max_hz: Hz, sample_rate: Hz) -> Self {
        let caps = SensorCapabilities {
            freq_min_hz,
            freq_max_hz,
            max_bandwidth_hz: sample_rate,
            sample_formats: vec![SampleFormat::Cf32],
            gain_min_db: 0.0,
            gain_max_db: 49.6,
        };
        let signals = DEFAULT_SIGNALS
            .iter()
            .filter(|(f, _)| *f >= freq_min_hz && *f <= freq_max_hz)
            .map(|&(freq_hz, amplitude)| SimSignal { freq_hz, amplitude })
            .collect();
        Self {
            id,
            caps,
            sample_rate,
            center_hz: (freq_min_hz + freq_max_hz) / 2.0,
            signals,
            sample_count: 0,
            noise_amp: 0.004,
        }
    }
}

impl IqSensor for SimSensor {
    fn id(&self) -> SensorId {
        self.id
    }
    fn capabilities(&self) -> &SensorCapabilities {
        &self.caps
    }
    fn sample_rate(&self) -> Hz {
        self.sample_rate
    }
    fn is_simulated(&self) -> bool {
        true
    }

    fn tune(&mut self, center_hz: Hz) -> Result<(), SensorError> {
        if !self.caps.covers(center_hz, self.sample_rate) {
            return Err(SensorError::UnsupportedFreq(center_hz));
        }
        self.center_hz = center_hz;
        Ok(())
    }

    fn set_gain(&mut self, _db: f32) -> Result<(), SensorError> {
        Ok(())
    }

    fn read(&mut self, out: &mut [Complex32]) -> Result<usize, SensorError> {
        let dt = 1.0 / self.sample_rate;
        let half_bw = self.sample_rate / 2.0;
        for (k, slot) in out.iter_mut().enumerate() {
            let idx = self.sample_count + k as u64;
            let t = idx as f64 * dt;
            let mut re = (rng(idx * 2) - 0.5) * 2.0 * self.noise_amp;
            let mut im = (rng(idx * 2 + 1) - 0.5) * 2.0 * self.noise_amp;
            for sig in &self.signals {
                let offset = sig.freq_hz - self.center_hz;
                if offset.abs() > half_bw {
                    continue;
                }
                let phase = TAU * offset * t;
                re += sig.amplitude * phase.cos() as f32;
                im += sig.amplitude * phase.sin() as f32;
            }
            *slot = Complex32::new(re, im);
        }
        self.sample_count += out.len() as u64;
        Ok(out.len())
    }
}

/// Deterministic splitmix64-based uniform in [0,1).
fn rng(x: u64) -> f32 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) as f32) / (u64::MAX as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tune_rejects_out_of_range() {
        let mut s = SimSensor::new(SensorId(0), 144e6, 148e6, 2.4e6);
        assert!(s.tune(146e6).is_ok());
        assert!(matches!(
            s.tune(900e6),
            Err(SensorError::UnsupportedFreq(_))
        ));
    }

    #[test]
    fn read_fills_buffer_with_finite_samples() {
        let mut s = SimSensor::new(SensorId(1), 144e6, 175e6, 2.4e6);
        s.tune(162.4e6).unwrap();
        let mut buf = vec![Complex32::new(0.0, 0.0); 4096];
        let n = s.read(&mut buf).unwrap();
        assert_eq!(n, 4096);
        assert!(buf.iter().all(|c| c.re.is_finite() && c.im.is_finite()));
        // a tuned-in signal makes the block carry real power above the noise-only level
        let power: f32 = buf.iter().map(|c| c.norm_sqr()).sum::<f32>() / buf.len() as f32;
        assert!(
            power > (s.noise_amp * s.noise_amp),
            "power {power} should exceed noise floor"
        );
    }
}
