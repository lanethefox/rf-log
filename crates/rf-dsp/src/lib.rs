//! RF-LOG v2 survey DSP core.
//!
//! The pieces of the `SURVEY → DETECT` stage of the recon pipeline:
//! - [`FftProcessor`] — windowed, coherent-gain-calibrated PSD + Welch averaging,
//! - [`cfar_detect`] / [`CfarConfig`] — CA-CFAR detection over a PSD frame,
//! - [`OccupancyMap`] — stitches per-tile frames into a wideband spectrum,
//! - [`SurveyDsp`] — the convenience that turns one dwell of IQ into a
//!   [`PsdFrame`] plus its [`Detection`]s.
//!
//! Pure Rust (`rustfft`); no LiquidDSP. The demod/decode drill-down (which needs
//! `rf-liquid`) is a separate, later concern.

mod cfar;
mod fft;
mod occupancy;

pub use cfar::{CfarConfig, detect as cfar_detect};
pub use fft::FftProcessor;
pub use occupancy::OccupancyMap;

use num_complex::Complex32;
use rf_types::{Detection, Hz, PsdFrame, SensorId, UnixNanos};

/// Turns one dwell of IQ into a PSD frame + detections — the per-dwell survey step.
pub struct SurveyDsp {
    fft: FftProcessor,
    /// CFAR parameters (public so a mission can tune sensitivity).
    pub cfar: CfarConfig,
    overlap: f32,
}

impl SurveyDsp {
    pub fn new(fft_size: usize) -> Self {
        Self {
            fft: FftProcessor::new(fft_size),
            cfar: CfarConfig::default(),
            overlap: 0.5,
        }
    }

    pub fn fft_size(&self) -> usize {
        self.fft.fft_size()
    }

    /// Process a dwell: Welch PSD over the capture, then CFAR. `sample_rate` sets
    /// the bin resolution and the absolute frequency mapping.
    pub fn process_dwell(
        &mut self,
        iq: &[Complex32],
        tile_center_hz: Hz,
        sample_rate: Hz,
        sensor: SensorId,
        t_unix_ns: UnixNanos,
    ) -> (PsdFrame, Vec<Detection>) {
        let mut psd = Vec::new();
        self.fft.welch_into(iq, self.overlap, &mut psd);
        let bin_hz = sample_rate / self.fft.fft_size() as f64;
        let dets = cfar::detect(&psd, &self.cfar, bin_hz, tile_center_hz, sensor, t_unix_ns);
        let frame = PsdFrame {
            tile_center_hz,
            bin_hz,
            psd_dbfs: psd,
            t_unix_ns,
            sensor,
        };
        (frame, dets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// End-to-end golden test: a synthetic tone at a known offset in a dwell must
    /// produce exactly one detection at the right absolute frequency.
    #[test]
    fn golden_dwell_detects_known_tone() {
        let fft_size = 4096;
        let sample_rate = 2_400_000.0;
        let tile_center = 150_000_000.0;
        let mut dsp = SurveyDsp::new(fft_size);

        // tone 300 kHz above the tile center, on a realistic complex noise floor
        let offset = 300_000.0f64;
        let len = fft_size * 8;
        let rng = |x: u64| {
            // splitmix64 → f32 in [0,1)
            let mut z = x.wrapping_add(0x9E3779B97F4A7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            ((z ^ (z >> 31)) as f32) / (u64::MAX as f32)
        };
        let iq: Vec<Complex32> = (0..len)
            .map(|i| {
                let t = i as f64 / sample_rate;
                let ph = (TAU as f64 * offset * t) as f32;
                let ni = (rng(i as u64 * 2) - 0.5) * 0.008;
                let nq = (rng(i as u64 * 2 + 1) - 0.5) * 0.008;
                Complex32::new(0.5 * ph.cos() + ni, 0.5 * ph.sin() + nq)
            })
            .collect();

        let (frame, dets) = dsp.process_dwell(&iq, tile_center, sample_rate, SensorId(7), 42);
        assert_eq!(frame.psd_dbfs.len(), fft_size);
        assert_eq!(dets.len(), 1, "expected one detection, got {}", dets.len());
        let d = &dets[0];
        assert!(
            (d.center_hz - (tile_center + offset)).abs() < 2.0 * frame.bin_hz,
            "center {} vs expected {}",
            d.center_hz,
            tile_center + offset
        );
        assert_eq!(d.sensor, SensorId(7));
        assert_eq!(d.t_unix_ns, 42);
        assert!(d.snr_db > 20.0);
    }
}
