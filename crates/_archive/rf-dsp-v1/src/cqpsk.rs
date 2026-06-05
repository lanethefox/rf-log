//! CQPSK/LSM (pi/4 DQPSK) demodulator for P25.
//!
//! Processes Complex32 IQ samples at 48 kHz and outputs P25 dibits at 4800 baud.
//! Uses Gardner timing error detector for symbol timing recovery and a Costas-style
//! PLL for carrier tracking. Modeled on OP25's gardner_costas_cc implementation.
//!
//! Key differences from the naive first implementation:
//! - Double-buffered delay line (not a 4-sample circular buffer)
//! - Linear interpolation matching OP25 (proven reliable)
//! - Proper Gardner TED using stored previous symbol + delay-line midpoint
//! - Costas loop on absolute symbols (not differential) with alternating constellation
//! - Lock detector with adaptive gains for fast acquisition → stable tracking

use num_complex::Complex32;
use p25::bits::Dibit;

/// Length of the interpolation delay line.
const DLLEN: usize = 48;

/// CQPSK demodulator: IQ at 48 kHz → P25 dibits at 4800 baud.
pub struct CqpskDemod {
    // ── Delay line (double-buffered for easy indexing) ──
    dl: [Complex32; DLLEN * 2],
    dl_index: usize,

    // ── Symbol timing recovery (Gardner TED) ──
    mu: f32,
    omega: f32,
    omega_mid: f32,
    omega_limit: f32,
    gain_mu: f32,
    gain_omega: f32,
    last_symbol: Complex32,

    // ── Carrier tracking PLL ──
    phase: f32,
    freq: f32,
    alpha: f32,
    beta: f32,
    interp_counter: u32, // alternating constellation tracker

    // ── Differential decode ──
    prev_symbol: Complex32,

    // ── Lock detector ──
    phase_err_accum: f32,
    phase_err_count: u32,
    locked: bool,
    samples_fed: u64,

    // ── Diagnostics (readable externally) ──
    pub diag: CqpskDiag,
}

/// Diagnostic counters for the CQPSK demodulator.
#[derive(Clone, Debug, Default)]
pub struct CqpskDiag {
    pub symbols_out: u64,
    pub carrier_freq_hz: f32,
    pub timing_offset: f32,
    pub phase_err_rms: f32,
    pub locked: bool,
}

impl CqpskDemod {
    pub fn new() -> Self {
        let omega_nom: f32 = 10.0; // 48000 / 4800 = 10 samples/symbol

        // Acquisition gains (wider bandwidth for fast lock)
        let gain_mu: f32 = 0.05;
        let gain_omega: f32 = gain_mu * gain_mu / 4.0;
        let alpha: f32 = 0.08;
        let beta: f32 = alpha * alpha / 4.0;

        CqpskDemod {
            dl: [Complex32::new(0.0, 0.0); DLLEN * 2],
            dl_index: 0,
            mu: 0.0,
            omega: omega_nom,
            omega_mid: omega_nom,
            omega_limit: omega_nom * 0.005,
            gain_mu,
            gain_omega,
            last_symbol: Complex32::new(0.0, 0.0),
            phase: 0.0,
            freq: 0.0,
            alpha,
            beta,
            interp_counter: 0,
            prev_symbol: Complex32::new(1.0, 0.0),
            phase_err_accum: 0.0,
            phase_err_count: 0,
            locked: false,
            samples_fed: 0,
            diag: CqpskDiag::default(),
        }
    }

    /// Feed one complex IQ sample (at 48 kHz). Returns a dibit when a symbol
    /// boundary is reached (~every 10 samples).
    pub fn feed(&mut self, sample: Complex32) -> Option<Dibit> {
        self.samples_fed += 1;

        // 1. Apply carrier correction
        let corrected = sample * Complex32::from_polar(1.0, -self.phase);

        // 2. Store in double-buffered delay line
        self.dl[self.dl_index] = corrected;
        self.dl[self.dl_index + DLLEN] = corrected;
        self.dl_index = (self.dl_index + 1) % DLLEN;

        // 3. Decrement fractional sample counter
        self.mu -= 1.0;

        if self.mu > 0.0 {
            return None;
        }

        // ── Symbol boundary reached ──

        // Interpolate current symbol
        let mu_frac = self.mu + 1.0; // in [0, 1)
        let symbol = self.interp_at(0, mu_frac);

        // Interpolate midpoint (half a symbol period back)
        let half_omega = (self.omega / 2.0) as usize;
        let mid_mu = mu_frac + self.omega / 2.0 - half_omega as f32;
        let midpoint = self.interp_at(half_omega, mid_mu);

        // 4. Gardner timing error detector
        //    error = Re{(last_symbol - symbol) * conj(midpoint)}
        let ted_error = ((self.last_symbol - symbol) * midpoint.conj()).re;

        // 5. Update symbol timing
        self.omega += self.gain_omega * ted_error;
        self.omega = clamp(
            self.omega,
            self.omega_mid - self.omega_limit,
            self.omega_mid + self.omega_limit,
        );
        self.mu += self.omega + self.gain_mu * ted_error;

        self.last_symbol = symbol;

        // 6. Costas carrier tracking — decision-directed phase error
        //    pi/4 DQPSK alternates between two QPSK constellations
        let phase_error = if self.interp_counter & 1 == 0 {
            // Even symbols: constellation at 0°, 90°, 180°, 270°
            Self::qpsk_phase_error(&symbol)
        } else {
            // Odd symbols: constellation rotated 45°
            let rotated = symbol * Complex32::from_polar(1.0, -std::f32::consts::FRAC_PI_4);
            Self::qpsk_phase_error(&rotated)
        };
        self.interp_counter += 1;

        self.freq += self.beta * phase_error;
        // Clamp carrier frequency offset to ±600 Hz (±0.785 rad/symbol at 4800 baud)
        // Prevents unbounded growth that could cause phase to reach infinity.
        const MAX_FREQ: f32 = 0.785;
        self.freq = self.freq.clamp(-MAX_FREQ, MAX_FREQ);
        self.phase += self.freq + self.alpha * phase_error;

        // Wrap phase to [-π, π]
        self.phase = wrap_phase(self.phase);

        // 7. Lock detection — track RMS phase error
        let pe2 = phase_error * phase_error;
        self.phase_err_accum += if pe2.is_finite() { pe2 } else { 1.0 };
        self.phase_err_count += 1;

        if self.phase_err_count >= 480 {
            // Evaluate every 100 symbols (480 = ~100 symbols at 4800 baud)
            let rms = (self.phase_err_accum / self.phase_err_count as f32).sqrt();
            let was_locked = self.locked;
            self.locked = rms < 0.6; // threshold for "locked"

            // Adapt loop gains based on lock state
            if self.locked && !was_locked {
                // Just locked — switch to tracking gains (narrower bandwidth)
                self.gain_mu = 0.025;
                self.gain_omega = self.gain_mu * self.gain_mu / 4.0;
                self.alpha = 0.04;
                self.beta = self.alpha * self.alpha / 4.0;
                tracing::info!(
                    "CQPSK: LOCKED (rms_err={:.3}, freq_offset={:.1} Hz, omega={:.3})",
                    rms,
                    self.freq * 4800.0 / (2.0 * std::f32::consts::PI),
                    self.omega,
                );
            } else if !self.locked && was_locked {
                // Lost lock — switch to acquisition gains (wider bandwidth)
                self.gain_mu = 0.05;
                self.gain_omega = self.gain_mu * self.gain_mu / 4.0;
                self.alpha = 0.08;
                self.beta = self.alpha * self.alpha / 4.0;
                tracing::warn!(
                    "CQPSK: LOST LOCK (rms_err={:.3}, freq_offset={:.1} Hz)",
                    rms,
                    self.freq * 4800.0 / (2.0 * std::f32::consts::PI),
                );
            }

            // Update diagnostics
            self.diag.phase_err_rms = rms;
            self.diag.carrier_freq_hz =
                self.freq * 4800.0 / (2.0 * std::f32::consts::PI);
            self.diag.timing_offset = self.omega - self.omega_mid;
            self.diag.locked = self.locked;

            self.phase_err_accum = 0.0;
            self.phase_err_count = 0;
        }

        self.diag.symbols_out += 1;

        // 8. Differential decode: diff = curr * conj(prev)
        let diff = symbol * self.prev_symbol.conj();
        self.prev_symbol = symbol;

        // 9. Slice differential phase to dibit
        let angle = diff.arg();
        Some(Self::slice_dibit(angle))
    }

    /// Reset demodulator state (call on frequency change).
    pub fn reset(&mut self) {
        self.dl = [Complex32::new(0.0, 0.0); DLLEN * 2];
        self.dl_index = 0;
        self.mu = 0.0;
        self.omega = self.omega_mid;
        self.last_symbol = Complex32::new(0.0, 0.0);
        self.phase = 0.0;
        self.freq = 0.0;
        self.interp_counter = 0;
        self.prev_symbol = Complex32::new(1.0, 0.0);
        self.phase_err_accum = 0.0;
        self.phase_err_count = 0;
        self.locked = false;
        self.samples_fed = 0;
        self.diag = CqpskDiag::default();

        // Reset to acquisition gains
        self.gain_mu = 0.05;
        self.gain_omega = self.gain_mu * self.gain_mu / 4.0;
        self.alpha = 0.08;
        self.beta = self.alpha * self.alpha / 4.0;
    }

    /// Linear interpolation from the delay line.
    ///
    /// `samples_back` is the integer number of samples back from the most recent.
    /// `mu` is the fractional sample offset [0, 1).
    fn interp_at(&self, samples_back: usize, mu: f32) -> Complex32 {
        // Base index in the delay line (most recent sample is at dl_index - 1)
        let base = self.dl_index + DLLEN - 1 - samples_back;
        let mu_clamped = mu.clamp(0.0, 1.0);

        // Linear interpolation between sample at `base` and `base - 1`
        let s0 = self.dl[base];     // newer
        let s1 = self.dl[base - 1]; // older
        s0 * (1.0 - mu_clamped) + s1 * mu_clamped
    }

    /// QPSK phase error (decision-directed).
    /// Hard-decides the nearest constellation point in {1, j, -1, -j},
    /// then computes error = Im{conj(decision) * sym}, normalized by amplitude.
    fn qpsk_phase_error(sym: &Complex32) -> f32 {
        let norm_sq = sym.norm_sqr();
        if norm_sq < 1e-12 {
            return 0.0;
        }
        // Hard decision: nearest QPSK constellation point {1, j, -1, -j}
        let decision = if sym.re.abs() >= sym.im.abs() {
            Complex32::new(if sym.re >= 0.0 { 1.0 } else { -1.0 }, 0.0)
        } else {
            Complex32::new(0.0, if sym.im >= 0.0 { 1.0 } else { -1.0 })
        };
        (decision.conj() * *sym).im / norm_sq.sqrt()
    }

    /// Map differential phase angle to P25 dibit (pi/4 DQPSK mapping).
    ///
    /// | Phase change | Symbol | Dibit |
    /// |---|---|---|
    /// | +135° (+3π/4) | +3 | 01 |
    /// | +45° (+π/4) | +1 | 00 |
    /// | -45° (-π/4) | -1 | 10 |
    /// | -135° (-3π/4) | -3 | 11 |
    fn slice_dibit(angle: f32) -> Dibit {
        // Normalize to [0, 2π)
        let a = if angle < 0.0 {
            angle + 2.0 * std::f32::consts::PI
        } else {
            angle
        };

        // Decision boundaries at 0°, 90°, 180°, 270°
        if a < std::f32::consts::FRAC_PI_2 {
            Dibit::new(0b00) // +π/4
        } else if a < std::f32::consts::PI {
            Dibit::new(0b01) // +3π/4
        } else if a < 3.0 * std::f32::consts::FRAC_PI_2 {
            Dibit::new(0b11) // -3π/4
        } else {
            Dibit::new(0b10) // -π/4
        }
    }

    /// Whether the demodulator has achieved carrier/timing lock.
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

#[inline]
fn clamp(val: f32, min: f32, max: f32) -> f32 {
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

#[inline]
fn wrap_phase(p: f32) -> f32 {
    if !p.is_finite() {
        return 0.0;
    }
    // Use modular arithmetic instead of a while loop to avoid
    // infinite loops when phase grows large.
    let mut wrapped = p % (2.0 * std::f32::consts::PI);
    if wrapped > std::f32::consts::PI {
        wrapped -= 2.0 * std::f32::consts::PI;
    } else if wrapped < -std::f32::consts::PI {
        wrapped += 2.0 * std::f32::consts::PI;
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slice_dibit() {
        use std::f32::consts::FRAC_PI_4;
        assert_eq!(CqpskDemod::slice_dibit(FRAC_PI_4).bits(), 0b00);
        assert_eq!(CqpskDemod::slice_dibit(3.0 * FRAC_PI_4).bits(), 0b01);
        assert_eq!(CqpskDemod::slice_dibit(-FRAC_PI_4).bits(), 0b10);
        assert_eq!(CqpskDemod::slice_dibit(-3.0 * FRAC_PI_4).bits(), 0b11);
    }

    #[test]
    fn test_qpsk_phase_error() {
        // Perfect symbol at 1+0j → error should be ~0
        let err = CqpskDemod::qpsk_phase_error(&Complex32::new(1.0, 0.0));
        assert!(err.abs() < 0.01, "got {}", err);

        // Perfect symbol at 0+1j → error should be ~0
        let err = CqpskDemod::qpsk_phase_error(&Complex32::new(0.0, 1.0));
        assert!(err.abs() < 0.01, "got {}", err);

        // Symbol with small phase error → error should be small and signed
        let err = CqpskDemod::qpsk_phase_error(&Complex32::new(0.98, 0.2));
        assert!(err > 0.0 && err < 0.5, "got {}", err);
    }

    #[test]
    fn test_new_reset() {
        let mut d = CqpskDemod::new();
        for _ in 0..100 {
            let _ = d.feed(Complex32::new(0.001, 0.001));
        }
        assert!(d.diag.symbols_out > 0);
        d.reset();
        assert_eq!(d.diag.symbols_out, 0);
        assert!(!d.locked);
    }

    #[test]
    fn test_interp_at_edges() {
        let mut d = CqpskDemod::new();
        // Fill delay line with a ramp
        for i in 0..DLLEN {
            let v = i as f32;
            d.dl[i] = Complex32::new(v, 0.0);
            d.dl[i + DLLEN] = Complex32::new(v, 0.0);
        }
        d.dl_index = DLLEN; // wraps to 0

        // mu=0 should return newest sample
        let s = d.interp_at(0, 0.0);
        assert!((s.re - (DLLEN - 1) as f32).abs() < 0.01);

        // mu=1 should return one-older sample
        let s = d.interp_at(0, 1.0);
        assert!((s.re - (DLLEN - 2) as f32).abs() < 0.01);

        // mu=0.5 should be midpoint
        let s = d.interp_at(0, 0.5);
        let expected = ((DLLEN - 1) as f32 + (DLLEN - 2) as f32) / 2.0;
        assert!((s.re - expected).abs() < 0.01);
    }

    #[test]
    fn test_wrap_phase_edge_cases() {
        // Normal wrapping
        let w = super::wrap_phase(4.0);
        assert!(w > -std::f32::consts::PI && w <= std::f32::consts::PI);

        // Large positive
        let w = super::wrap_phase(1000.0);
        assert!(w > -std::f32::consts::PI && w <= std::f32::consts::PI);

        // Large negative
        let w = super::wrap_phase(-1000.0);
        assert!(w >= -std::f32::consts::PI && w <= std::f32::consts::PI);

        // NaN returns 0 (not infinite loop)
        let w = super::wrap_phase(f32::NAN);
        assert_eq!(w, 0.0);

        // Infinity returns 0 (not infinite loop)
        let w = super::wrap_phase(f32::INFINITY);
        assert_eq!(w, 0.0);

        let w = super::wrap_phase(f32::NEG_INFINITY);
        assert_eq!(w, 0.0);
    }
}
