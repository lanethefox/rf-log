//! rf-same — SAME (Specific Area Message Encoding) decoder for NOAA Weather Radio.
//!
//! Self-contained IQ → SAME alert pipeline:
//! IQ → NCO (tune to WX freq) → Resample to 48 kHz → NFM demod → AFSK decode → SAME parse.

pub mod afsk;
pub mod codes;
pub mod fips;
pub mod parser;

pub use codes::Severity;
pub use fips::FipsLocation;
pub use parser::SameAlert;

use num_complex::Complex32;

/// Complete SAME decoder with embedded NFM demodulation chain.
///
/// Feed raw IQ samples from the SDR and receive decoded weather alerts.
/// The decoder internally handles NCO mixing, resampling, FM demodulation,
/// AFSK tone detection, and SAME protocol parsing.
pub struct SameDecoder {
    // NFM demod chain (rf-liquid)
    nco: rf_liquid::Nco,
    resampler: rf_liquid::Resampler,
    demod: rf_liquid::FreqDemod,

    // AFSK + SAME
    afsk: afsk::AfskDemod,
    same_parser: parser::SameParser,

    // Working buffers
    mix_buf: Vec<Complex32>,
    resamp_buf: Vec<Complex32>,
    audio_buf: Vec<f32>,

    // Config
    sample_rate: f64,
}

impl SameDecoder {
    /// Create a new SAME decoder.
    ///
    /// - `sample_rate`: SDR sample rate in Hz (e.g., 2_400_000.0)
    /// - `target_freq_hz`: WX frequency to decode (e.g., 162_400_000.0)
    /// - `center_freq_hz`: Current SDR center frequency in Hz
    pub fn new(sample_rate: f64, target_freq_hz: f64, center_freq_hz: f64) -> Self {
        let audio_rate = 48_000.0;

        // NCO: tune from center to target frequency
        let offset_hz = target_freq_hz - center_freq_hz;
        let nco_freq = 2.0 * std::f64::consts::PI * offset_hz / sample_rate;
        let nco = rf_liquid::Nco::new(nco_freq as f32);

        // Resampler: SDR rate → 48 kHz
        let resamp_rate = audio_rate / sample_rate;
        let resampler = rf_liquid::Resampler::new(resamp_rate as f32, 60.0);

        // FM demodulator: kf = 0.5 for NFM (±5 kHz deviation at 48 kHz)
        let demod = rf_liquid::FreqDemod::new(0.5);

        // AFSK demodulator at 48 kHz
        let afsk = afsk::AfskDemod::new(audio_rate as f32);

        Self {
            nco,
            resampler,
            demod,
            afsk,
            same_parser: parser::SameParser::new(),
            mix_buf: Vec::new(),
            resamp_buf: Vec::new(),
            audio_buf: Vec::new(),
            sample_rate,
        }
    }

    /// Feed raw IQ samples and attempt SAME decode.
    ///
    /// Returns `Some(SameAlert)` when a complete SAME message is decoded.
    pub fn feed_iq(&mut self, iq: &[Complex32]) -> Option<SameAlert> {
        if iq.is_empty() {
            return None;
        }

        // Ensure working buffers are large enough
        let n = iq.len();
        self.mix_buf.resize(n, Complex32::new(0.0, 0.0));

        // Step 1: NCO mix down to baseband
        self.nco.mix_block_down(iq, &mut self.mix_buf);

        // Step 2: Resample to 48 kHz
        let max_out = (n as f32 * self.resampler.rate()).ceil() as usize + 64;
        self.resamp_buf.resize(max_out, Complex32::new(0.0, 0.0));
        let n_out = self.resampler.execute(&self.mix_buf[..n], &mut self.resamp_buf);

        if n_out == 0 {
            return None;
        }

        // Step 3: FM demodulate to audio
        self.audio_buf.resize(n_out, 0.0);
        self.demod.demod_block(&self.resamp_buf[..n_out], &mut self.audio_buf);

        // Step 4: AFSK decode
        let bytes = self.afsk.feed(&self.audio_buf[..n_out]);

        // Step 5: Feed decoded bytes to SAME parser
        let mut result = None;
        for &byte in bytes {
            if let Some(alert) = self.same_parser.feed_byte(byte) {
                result = Some(alert);
            }
        }

        result
    }

    /// Returns true if a SAME preamble has been detected.
    /// Used by the scan controller to decide whether to extend dwell.
    pub fn preamble_detected(&self) -> bool {
        self.afsk.preamble_detected() || self.same_parser.has_partial_decode()
    }

    /// Reset the decoder for a new frequency or new decode attempt.
    pub fn reset(&mut self) {
        self.afsk.reset();
        self.same_parser.reset();
        self.demod.reset();
    }

    /// Update target and center frequencies (e.g., when alternating WX channels).
    pub fn set_frequencies(&mut self, target_freq_hz: f64, center_freq_hz: f64) {
        let offset_hz = target_freq_hz - center_freq_hz;
        let nco_freq = 2.0 * std::f64::consts::PI * offset_hz / self.sample_rate;
        self.nco.set_frequency(nco_freq as f32);
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_creation() {
        let decoder = SameDecoder::new(2_400_000.0, 162_400_000.0, 162_400_000.0);
        assert!(!decoder.preamble_detected());
    }

    #[test]
    fn test_decoder_feed_empty() {
        let mut decoder = SameDecoder::new(2_400_000.0, 162_400_000.0, 162_400_000.0);
        assert!(decoder.feed_iq(&[]).is_none());
    }

    #[test]
    fn test_decoder_feed_silence() {
        let mut decoder = SameDecoder::new(2_400_000.0, 162_400_000.0, 162_400_000.0);
        let silence = vec![Complex32::new(0.0, 0.0); 4096];
        assert!(decoder.feed_iq(&silence).is_none());
        assert!(!decoder.preamble_detected());
    }

    #[test]
    fn test_decoder_reset() {
        let mut decoder = SameDecoder::new(2_400_000.0, 162_400_000.0, 162_400_000.0);
        decoder.reset();
        assert!(!decoder.preamble_detected());
    }

    #[test]
    fn test_set_frequencies() {
        let mut decoder = SameDecoder::new(2_400_000.0, 162_400_000.0, 162_400_000.0);
        decoder.set_frequencies(162_550_000.0, 162_400_000.0);
        assert!(!decoder.preamble_detected());
    }
}
