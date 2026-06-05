//! Decode IMBE frames into an audio signal.

use collect_slice::CollectSlice;
use rand::rngs::SmallRng;
use rand::SeedableRng;

use coefs::Coefficients;
use consts::SAMPLES_PER_FRAME;
use descramble::{descramble, Bootstrap};
use enhance::{self, EnhancedSpectrals, FrameEnergy, EnhanceErrors};
use frame::{AudioBuf, ReceivedFrame};
use gain::Gains;
use params::BaseParams;
use prev::PrevFrame;
use spectral::Spectrals;
use unvoiced::{UnvoicedDft, Unvoiced};
use voiced::{Phase, PhaseBase, Voiced};

/// Decodes a stream of IMBE frames.
pub struct ImbeDecoder {
    /// Tracks saved parameters across frames.
    prev: PrevFrame,
    /// Fast PRNG for noise generation, seeded from entropy once.
    rng: SmallRng,
}

impl ImbeDecoder {
    /// Create a new `ImbeDecoder` in the default state.
    pub fn new() -> ImbeDecoder {
        ImbeDecoder {
            prev: PrevFrame::default(),
            rng: SmallRng::from_entropy(),
        }
    }

    /// Decode the given frame into the given audio sample buffer.
    pub fn decode(&mut self, frame: ReceivedFrame, buf: &mut AudioBuf) {
        let period = match Bootstrap::new(&frame.chunks) {
            Bootstrap::Period(p) => p,
            Bootstrap::Invalid => {
                // Repeat previous frame on invalid period [p46].
                self.repeat(buf);
                return;
            },
            Bootstrap::Silence => {
                self.silence(buf);
                return;
            },
        };

        let errors = EnhanceErrors::new(&frame.errors, self.prev.err_rate);

        if enhance::should_repeat(&errors) {
            self.repeat(buf);
            return;
        }

        if enhance::should_mute(&errors) {
            self.silence(buf);
            return;
        }

        let params = BaseParams::new(period);
        let (amps, mut voice, gain_idx) = descramble(&frame.chunks, &params);
        let gains = Gains::new(gain_idx, &amps, &params);
        let coefs = Coefficients::new(&gains, &amps, &params);
        let spectrals = Spectrals::new(&coefs, &params, &self.prev);
        let energy = FrameEnergy::new(&spectrals, &self.prev.energy, &params);

        let mut enhanced = EnhancedSpectrals::new(&spectrals, &energy, &params);
        let amp_thresh = enhance::amp_thresh(&errors, self.prev.amp_thresh);
        enhance::smooth(&mut enhanced, &mut voice, &errors, &energy, amp_thresh);

        let udft = UnvoicedDft::new(&params, &voice, &enhanced, &mut self.rng);
        let vbase = PhaseBase::new(&params, &self.prev);
        let vphase = Phase::new(&vbase, &params, &self.prev, &voice, &mut self.rng);

        // Sequential synthesis (replaces crossbeam parallel scope — 160 samples is trivial).
        let unvoiced = Unvoiced::new(&udft, &self.prev.unvoiced);
        let voiced = Voiced::new(&params, &self.prev, &vphase, &enhanced, &voice);

        (0..SAMPLES_PER_FRAME)
            .map(|n| unvoiced.get(n) + voiced.get(n))
            .collect_slice_checked(&mut buf[..]);

        // Save current parameters.
        self.prev = PrevFrame {
            params: params,
            spectrals: spectrals,
            enhanced: enhanced,
            voice: voice,
            err_rate: errors.rate,
            energy: energy,
            amp_thresh: amp_thresh,
            unvoiced: udft,
            phase_base: vbase,
            phase: vphase,
        };
    }

    /// Fill the given audio buffer with silence.
    fn silence(&self, buf: &mut AudioBuf) {
        (0..SAMPLES_PER_FRAME).map(|_| 0.0).collect_slice_checked(&mut buf[..]);
    }

    /// Repeat the previous frame into the given audio buffer.
    fn repeat(&mut self, buf: &mut AudioBuf) {
        // Apply Eqs 99 through 104.
        let params = self.prev.params.clone();
        let voice = self.prev.voice.clone();
        let enhanced = self.prev.enhanced.clone();

        let udft = UnvoicedDft::new(&params, &voice, &enhanced, &mut self.rng);
        let vbase = PhaseBase::new(&params, &self.prev);
        let vphase = Phase::new(&vbase, &params, &self.prev, &voice, &mut self.rng);

        let unvoiced = Unvoiced::new(&udft, &self.prev.unvoiced);
        let voiced = Voiced::new(&params, &self.prev, &vphase, &enhanced, &voice);

        // Repeat frame using previous parameters [p47].
        (0..SAMPLES_PER_FRAME)
            .map(|n| unvoiced.get(n) + voiced.get(n))
            .collect_slice_checked(&mut buf[..]);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use consts::SAMPLES_PER_FRAME;

    // Thread constants removed — synthesis is now sequential.

    #[test]
    fn verify_frame_size() {
        assert_eq!(SAMPLES_PER_FRAME, 160);
    }
}
