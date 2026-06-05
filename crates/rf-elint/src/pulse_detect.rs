use crate::pdw::Pdw;
use num_complex::Complex32;

/// Detects pulses in an IQ buffer by thresholding the power envelope.
/// Returns a list of PDWs extracted from the buffer.
pub struct PulseDetector {
    /// Threshold in linear power (above which a pulse is active)
    threshold_linear: f32,
    /// Sample rate in Hz (used for timing)
    sample_rate: f64,
    /// Center frequency in MHz
    center_freq_mhz: f64,
    /// State: whether we are currently inside a pulse
    in_pulse: bool,
    /// Sample index where the current pulse started
    pulse_start: u64,
    /// Running sample counter
    sample_count: u64,
    /// Previous pulse TOA for PRI calculation
    prev_toa: Option<f64>,
}

impl PulseDetector {
    pub fn new(threshold_dbfs: f32, sample_rate: f64, center_freq_mhz: f64) -> Self {
        let threshold_linear = 10f32.powf(threshold_dbfs / 10.0);
        Self {
            threshold_linear,
            sample_rate,
            center_freq_mhz,
            in_pulse: false,
            pulse_start: 0,
            sample_count: 0,
            prev_toa: None,
        }
    }

    /// Feed a slice of IQ samples, returning any completed PDWs.
    pub fn feed(&mut self, samples: &[Complex32]) -> Vec<Pdw> {
        let mut pdws = Vec::new();

        for &s in samples {
            let power = s.norm_sqr(); // |I|² + |Q|²
            self.sample_count += 1;

            if !self.in_pulse && power >= self.threshold_linear {
                self.in_pulse = true;
                self.pulse_start = self.sample_count;
            } else if self.in_pulse && power < self.threshold_linear {
                self.in_pulse = false;
                let toa = self.pulse_start as f64 / self.sample_rate;
                let pw_us = (self.sample_count - self.pulse_start) as f64
                    / self.sample_rate
                    * 1e6;

                let pri_us = self.prev_toa.map(|prev| (toa - prev) * 1e6);
                self.prev_toa = Some(toa);

                pdws.push(Pdw {
                    toa,
                    pw_us,
                    freq_mhz: self.center_freq_mhz,
                    amplitude_dbfs: 10.0 * power.log10(),
                    pri_us,
                });
            }
        }

        pdws
    }
}
