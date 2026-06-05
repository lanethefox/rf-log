//! RF-LOG v2 heterogeneous IQ sensor layer.
//!
//! The `SURVEY` front of the pipeline: a [`SensorPool`] of [`IqSensor`]s, each
//! assigned a [`SensorRole`](rf_types::SensorRole), sweeping a tiled frequency plan
//! and emitting [`Dwell`]s for the survey DSP. P0 ships the simulated path
//! ([`SimSensor`]); the SoapySDR/RTL backend (with proper teardown) arrives in the
//! hardware phase. Fan-out primitives ([`LossyIqRing`]/[`LosslessIqRing`]) provide
//! the per-role ring semantics the streaming/collector paths need.

mod error;
mod pool;
mod ring;
pub mod sim;

pub use error::SensorError;
pub use pool::{PoolConfig, PoolHandle, SensorPool};
pub use ring::{
    LosslessConsumer, LosslessIqRing, LosslessProducer, LossyConsumer, LossyIqRing, LossyProducer,
};
pub use sim::SimSensor;

use num_complex::Complex32;
use rf_types::{Band, Hz, SensorCapabilities, SensorId};
use std::time::{SystemTime, UNIX_EPOCH};

/// A heterogeneous IQ source. The pool reasons over [`capabilities`](IqSensor::capabilities)
/// to plan tiling, so mixed device classes (RTL now; HackRF/Airspy later) coexist.
pub trait IqSensor: Send {
    fn id(&self) -> SensorId;
    fn capabilities(&self) -> &SensorCapabilities;
    /// Streaming sample rate ≈ instantaneous bandwidth (complex sampling).
    fn sample_rate(&self) -> Hz;
    fn tune(&mut self, center_hz: Hz) -> Result<(), SensorError>;
    fn set_gain(&mut self, db: f32) -> Result<(), SensorError>;
    /// Fill as much of `out` as available; returns the count. `Ok(0)` means no data
    /// right now (caller may retry).
    fn read(&mut self, out: &mut [Complex32]) -> Result<usize, SensorError>;
    fn is_simulated(&self) -> bool {
        false
    }
}

/// One captured dwell: a block of IQ from one tile, ready for the survey DSP.
#[derive(Clone)]
pub struct Dwell {
    pub sensor: SensorId,
    pub tile_center_hz: Hz,
    pub sample_rate: Hz,
    pub iq: Vec<Complex32>,
    pub t_unix_ns: i64,
}

/// Wall-clock nanoseconds since the Unix epoch (0 if the clock is before the epoch).
pub fn now_unix_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Tile a set of bands into `tile_bw`-wide center frequencies (~10% overlap).
pub fn plan_tiles(bands: &[Band], tile_bw: Hz) -> Vec<Hz> {
    let mut tiles = Vec::new();
    if tile_bw <= 0.0 {
        return tiles;
    }
    for b in bands {
        if b.high_hz <= b.low_hz {
            continue;
        }
        let lo_c = b.low_hz + tile_bw / 2.0;
        let hi_c = b.high_hz - tile_bw / 2.0;
        if hi_c <= lo_c {
            tiles.push((b.low_hz + b.high_hz) / 2.0); // band narrower than one tile
            continue;
        }
        let step = tile_bw * 0.9;
        let mut c = lo_c;
        while c < hi_c + step {
            tiles.push(c.min(hi_c));
            c += step;
        }
    }
    tiles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    tiles.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_a_band() {
        let bands = vec![Band {
            name: "VHF".into(),
            low_hz: 144e6,
            high_hz: 174e6,
        }];
        let tiles = plan_tiles(&bands, 2.4e6);
        assert!(!tiles.is_empty());
        assert!(*tiles.first().unwrap() >= 144e6 && *tiles.last().unwrap() <= 174e6);
        assert!(
            tiles.last().unwrap() - tiles.first().unwrap() > 25e6,
            "coverage spans the band"
        );
    }
}
