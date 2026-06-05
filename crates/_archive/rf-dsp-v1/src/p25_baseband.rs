//! P25 C4FM baseband conditioning: DC blocker + RRC matched filter.
//!
//! These filters are applied between the FM discriminator output and the P25
//! decoder to remove DC bias and apply the matched filter for optimal symbol
//! recovery.

/// Single-pole IIR DC-blocking filter: y[n] = x[n] - x[n-1] + α·y[n-1]
pub struct DcBlocker {
    prev_x: f32,
    prev_y: f32,
    alpha: f32,
}

impl DcBlocker {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        // α ≈ 1 - 2π·fc/fs — pole near z=1 for very low cutoff
        let alpha = 1.0 - (2.0 * std::f32::consts::PI * cutoff_hz / sample_rate);
        Self { prev_x: 0.0, prev_y: 0.0, alpha }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.prev_x + self.alpha * self.prev_y;
        self.prev_x = x;
        self.prev_y = y;
        y
    }

    pub fn reset(&mut self) {
        self.prev_x = 0.0;
        self.prev_y = 0.0;
    }
}

/// FIR filter with ring-buffer delay line (for RRC matched filter).
pub struct BasebandFir {
    taps: Vec<f32>,
    delay: Vec<f32>,
    pos: usize,
}

impl BasebandFir {
    pub fn new(taps: Vec<f32>) -> Self {
        let len = taps.len();
        Self { taps, delay: vec![0.0; len], pos: 0 }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        self.delay[self.pos] = x;
        let len = self.taps.len();
        let mut sum = 0.0_f32;
        for i in 0..len {
            let idx = (self.pos + len - i) % len;
            sum += self.taps[i] * self.delay[idx];
        }
        self.pos = (self.pos + 1) % len;
        sum
    }

    pub fn reset(&mut self) {
        self.delay.fill(0.0);
        self.pos = 0;
    }
}

/// Generate root-raised-cosine filter coefficients.
/// - `sps`: samples per symbol (10 for P25 at 48 kHz)
/// - `alpha`: roll-off factor (0.2 per TIA-102.BAAA)
/// - `ntaps`: filter length (odd recommended)
pub fn rrc_coefficients(sps: usize, alpha: f32, ntaps: usize) -> Vec<f32> {
    use std::f32::consts::PI;
    let center = ntaps / 2;
    let sps_f = sps as f32;
    let mut taps = Vec::with_capacity(ntaps);

    for i in 0..ntaps {
        let t = (i as f32 - center as f32) / sps_f; // time in symbol periods
        let h = if t.abs() < 1e-10 {
            // t = 0
            1.0 - alpha + 4.0 * alpha / PI
        } else if ((4.0 * alpha * t).abs() - 1.0).abs() < 1e-6 {
            // t = ±1/(4·α) — singularity
            (alpha / 2.0_f32.sqrt())
                * ((1.0 + 2.0 / PI) * (PI / (4.0 * alpha)).sin()
                    + (1.0 - 2.0 / PI) * (PI / (4.0 * alpha)).cos())
        } else {
            let pi_t = PI * t;
            ((pi_t * (1.0 - alpha)).sin()
                + 4.0 * alpha * t * (pi_t * (1.0 + alpha)).cos())
                / (pi_t * (1.0 - (4.0 * alpha * t).powi(2)))
        };
        taps.push(h);
    }

    // Normalize so passband gain = 1
    let energy: f32 = taps.iter().map(|t| t * t).sum();
    let norm = energy.sqrt();
    if norm > 0.0 {
        for t in &mut taps {
            *t /= norm;
        }
    }

    taps
}
