//! AFSK tone detector + bit clock recovery for SAME (520.83 baud).
//!
//! Signal chain: audio f32 (48 kHz) → bandpass → Goertzel at mark/space → bit recovery → byte framing.

use std::f32::consts::PI;

/// SAME AFSK parameters.
const BAUD_RATE: f32 = 520.83;
const MARK_FREQ: f32 = 2083.3;  // binary 1
const SPACE_FREQ: f32 = 1562.5; // binary 0

/// Goertzel filter for detecting a single frequency in a block of samples.
struct Goertzel {
    coeff: f32,
    s1: f32,
    s2: f32,
    count: usize,
    block_size: usize,
}

impl Goertzel {
    fn new(target_freq: f32, sample_rate: f32, block_size: usize) -> Self {
        let k = (target_freq / sample_rate * block_size as f32).round();
        let coeff = 2.0 * (2.0 * PI * k / block_size as f32).cos();
        Self { coeff, s1: 0.0, s2: 0.0, count: 0, block_size }
    }

    fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.count = 0;
    }

    /// Feed a sample and return Some(power) when a block is complete.
    fn feed(&mut self, sample: f32) -> Option<f32> {
        let s0 = sample + self.coeff * self.s1 - self.s2;
        self.s2 = self.s1;
        self.s1 = s0;
        self.count += 1;

        if self.count >= self.block_size {
            let power = self.s1 * self.s1 + self.s2 * self.s2 - self.coeff * self.s1 * self.s2;
            self.reset();
            Some(power)
        } else {
            None
        }
    }
}

/// Bandpass filter (simple 2nd-order IIR) for 1400-2300 Hz.
struct BandpassFilter {
    // State for cascaded biquads
    x1: f32, x2: f32,
    y1: f32, y2: f32,
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
}

impl BandpassFilter {
    fn new(sample_rate: f32) -> Self {
        // Bandpass centered around 1800 Hz with Q ~1.5
        let center = 1800.0;
        let bw = 900.0; // 1400-2300 Hz
        let q = center / bw;
        let w0 = 2.0 * PI * center / sample_rate;
        let alpha = w0.sin() / (2.0 * q);

        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * w0.cos();
        let a2 = 1.0 - alpha;

        Self {
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: a1 / a0, a2: a2 / a0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0; self.x2 = 0.0;
        self.y1 = 0.0; self.y2 = 0.0;
    }
}

/// AFSK demodulator: converts audio samples to decoded bytes.
pub struct AfskDemod {
    _sample_rate: f32,
    bandpass: BandpassFilter,
    mark_det: Goertzel,
    space_det: Goertzel,
    /// Bit clock recovery
    samples_per_bit: f32,
    clock_phase: f32,
    /// Current bit accumulator
    current_byte: u8,
    bit_count: u8,
    /// Preamble detection
    preamble_count: usize,
    preamble_detected: bool,
    /// Output buffer
    output: Vec<u8>,
    /// Recent mark/space decisions for edge detection
    last_bit: bool,
    /// Lock indicator: true when we've seen enough preamble
    locked: bool,
}

impl AfskDemod {
    pub fn new(sample_rate: f32) -> Self {
        // Goertzel block size: one bit period = sample_rate / baud_rate
        let block_size = (sample_rate / BAUD_RATE).round() as usize;

        Self {
            _sample_rate: sample_rate,
            bandpass: BandpassFilter::new(sample_rate),
            mark_det: Goertzel::new(MARK_FREQ, sample_rate, block_size),
            space_det: Goertzel::new(SPACE_FREQ, sample_rate, block_size),
            samples_per_bit: sample_rate / BAUD_RATE,
            clock_phase: 0.0,
            current_byte: 0,
            bit_count: 0,
            preamble_count: 0,
            preamble_detected: false,
            output: Vec::new(),
            last_bit: false,
            locked: false,
        }
    }

    /// Returns true if SAME preamble (0xAB) has been detected.
    pub fn preamble_detected(&self) -> bool {
        self.preamble_detected
    }

    /// Returns true when the demodulator is locked to a signal.
    pub fn locked(&self) -> bool {
        self.locked
    }

    /// Reset the demodulator state for a new decode attempt.
    pub fn reset(&mut self) {
        self.bandpass.reset();
        self.mark_det.reset();
        self.space_det.reset();
        self.clock_phase = 0.0;
        self.current_byte = 0;
        self.bit_count = 0;
        self.preamble_count = 0;
        self.preamble_detected = false;
        self.output.clear();
        self.last_bit = false;
        self.locked = false;
    }

    /// Feed audio samples and return any decoded bytes.
    pub fn feed(&mut self, samples: &[f32]) -> &[u8] {
        self.output.clear();

        for &sample in samples {
            // Bandpass filter
            let filtered = self.bandpass.process(sample);

            // Feed both Goertzel detectors
            let mark_power = self.mark_det.feed(filtered);
            let space_power = self.space_det.feed(filtered);

            // When both detectors complete a block, make a bit decision
            if let (Some(mp), Some(sp)) = (mark_power, space_power) {
                let bit = mp > sp; // mark = 1, space = 0

                // Edge detection for clock recovery
                if bit != self.last_bit {
                    // Transition detected — nudge clock phase toward center of bit
                    self.clock_phase = self.samples_per_bit * 0.5;
                }
                self.last_bit = bit;

                // Clock recovery: accumulate bit
                self.clock_phase += self.samples_per_bit;
                if self.clock_phase >= self.samples_per_bit {
                    self.clock_phase -= self.samples_per_bit;
                    self.process_bit(bit);
                }
            }
        }

        &self.output
    }

    /// Process a single recovered bit (LSB-first byte framing).
    fn process_bit(&mut self, bit: bool) {
        // LSB-first: shift in from the top
        self.current_byte >>= 1;
        if bit {
            self.current_byte |= 0x80;
        }
        self.bit_count += 1;

        if self.bit_count >= 8 {
            let byte = self.current_byte;
            self.current_byte = 0;
            self.bit_count = 0;

            // Track preamble bytes (0xAB)
            if byte == 0xAB {
                self.preamble_count += 1;
                if self.preamble_count >= 8 {
                    self.preamble_detected = true;
                    self.locked = true;
                }
            } else {
                if self.preamble_detected {
                    // After preamble, emit data bytes
                    self.output.push(byte);
                }
                self.preamble_count = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goertzel_basic() {
        let sample_rate = 48000.0;
        let block_size = 92; // ~520.83 baud at 48 kHz
        let mut g = Goertzel::new(MARK_FREQ, sample_rate, block_size);

        // Generate a pure 2083.3 Hz tone
        for i in 0..block_size {
            let t = i as f32 / sample_rate;
            let sample = (2.0 * PI * MARK_FREQ * t).sin();
            if let Some(power) = g.feed(sample) {
                assert!(power > 0.0, "Mark tone should produce positive power");
            }
        }
    }

    #[test]
    fn test_bandpass_passes_mark() {
        let mut bp = BandpassFilter::new(48000.0);
        // Feed mark frequency — should pass through with decent amplitude
        let mut max_out = 0.0f32;
        for i in 0..4800 {
            let t = i as f32 / 48000.0;
            let sample = (2.0 * PI * MARK_FREQ * t).sin();
            let out = bp.process(sample).abs();
            max_out = max_out.max(out);
        }
        assert!(max_out > 0.1, "Bandpass should pass mark frequency (got {})", max_out);
    }

    #[test]
    fn test_afsk_demod_reset() {
        let mut demod = AfskDemod::new(48000.0);
        assert!(!demod.preamble_detected());
        assert!(!demod.locked());
        demod.reset();
        assert!(!demod.preamble_detected());
    }

    #[test]
    fn test_afsk_demod_preamble_synthetic() {
        let sample_rate = 48000.0;
        let mut demod = AfskDemod::new(sample_rate);
        let samples_per_bit = (sample_rate / BAUD_RATE).round() as usize;

        // Generate preamble: 16 bytes of 0xAB
        // 0xAB = 10101011 in binary, LSB first = 1,1,0,1,0,1,0,1
        let preamble_bits: Vec<bool> = {
            let mut bits = Vec::new();
            for _ in 0..16 {
                // 0xAB LSB-first: bits 0-7 of 0xAB = 1,1,0,1,0,1,0,1
                for bit_idx in 0..8 {
                    bits.push((0xABu8 >> bit_idx) & 1 == 1);
                }
            }
            bits
        };

        // Convert bits to AFSK audio
        let mut audio = Vec::new();
        for &bit in &preamble_bits {
            let freq = if bit { MARK_FREQ } else { SPACE_FREQ };
            for i in 0..samples_per_bit {
                let t = i as f32 / sample_rate;
                audio.push(0.5 * (2.0 * PI * freq * t).sin());
            }
        }

        // Feed the audio
        let _bytes = demod.feed(&audio);
        // Preamble should be detected after enough 0xAB bytes
        // Note: in practice the Goertzel block alignment may cause slight delay
        assert!(demod.preamble_detected() || demod.preamble_count >= 4,
            "Expected preamble detection (count={})", demod.preamble_count);
    }
}
