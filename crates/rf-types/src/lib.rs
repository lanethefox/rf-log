//! Shared cross-crate contracts for RF-LOG v2.
//!
//! These types are the frozen interface between the survey/sensor/DSP crates, the
//! data layer (`rf-bus`, `rf-catalog`), the mission orchestrator (`rf-mission`), and
//! the Tauri app. Keeping them in one tiny, dependency-light crate lets the other
//! P0 crates be built in parallel against a stable contract.
//!
//! Everything here is plain data with `serde` derives so it can cross the bus, land
//! in the catalog, and serialize over the Tauri IPC boundary unchanged.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

/// Frequency in hertz. Wideband work spans ~24 MHz–6 GHz, so `f64` (≈15–16
/// significant digits) gives sub-hertz precision across the whole range.
pub type Hz = f64;

/// A wall-clock timestamp as nanoseconds since the Unix epoch. Chosen over a
/// `chrono` type to keep this crate dependency-light; convert at the edges.
pub type UnixNanos = i64;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Stable identifier for a sensor within a running pool (not a DB row id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SensorId(pub u32);

/// Catalog row id for a mission (SQLite `INTEGER PRIMARY KEY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MissionId(pub i64);

// ---------------------------------------------------------------------------
// Sensor domain
// ---------------------------------------------------------------------------

/// IQ sample formats a sensor may deliver. The pool normalizes everything to
/// `Cf32` before DSP; this enum captures what the device produces natively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleFormat {
    /// Interleaved complex `f32` (RTL-SDR via SoapySDR delivers this after conversion).
    Cf32,
    /// Interleaved complex `i8` (HackRF native).
    Cs8,
    /// Interleaved complex `i16` (Airspy / many SDRs).
    Cs16,
}

/// Static capabilities of a sensor, used by the pool to plan roles and tiling.
/// A heterogeneous pool (RTL now; HackRF/Airspy later) is the whole point, so the
/// scheduler must reason over these rather than assume one device class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorCapabilities {
    /// Tunable center-frequency range, inclusive.
    pub freq_min_hz: Hz,
    pub freq_max_hz: Hz,
    /// Maximum instantaneous (sampling) bandwidth, in Hz. Sets the tile width.
    pub max_bandwidth_hz: Hz,
    /// Native sample formats the device can produce.
    pub sample_formats: Vec<SampleFormat>,
    /// Overall gain range in dB (tuner/aggregate), for AGC and headroom decisions.
    pub gain_min_db: f32,
    pub gain_max_db: f32,
}

impl SensorCapabilities {
    /// Whether this sensor can tune to `center_hz` with `bandwidth_hz` fully inside
    /// its tunable range.
    pub fn covers(&self, center_hz: Hz, bandwidth_hz: Hz) -> bool {
        let half = bandwidth_hz / 2.0;
        bandwidth_hz <= self.max_bandwidth_hz
            && center_hz - half >= self.freq_min_hz
            && center_hz + half <= self.freq_max_hz
    }
}

/// The job the pool has assigned a sensor. Only `SurveySweep` is active in P0;
/// `DwellCollect` (lossless IQ capture) and `DedicatedBand` arrive in P1+.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SensorRole {
    /// Step across the mission's tile plan, emitting a `PsdFrame` per dwell.
    SurveySweep,
    /// Park on a signal of interest and capture lossless IQ (P1).
    DwellCollect { center_hz: Hz, bandwidth_hz: Hz },
    /// Hold a fixed center (e.g. a future control-channel watcher).
    DedicatedBand { center_hz: Hz },
}

/// Lifecycle state of a sensor, surfaced to the UI as live status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorState {
    Disconnected,
    Connected,
    Sweeping,
    Dwelling,
    Error,
}

// ---------------------------------------------------------------------------
// Spectral data
// ---------------------------------------------------------------------------

/// One power-spectral-density frame from a single dwell on a single tile.
///
/// `psd_dbfs[i]` is the power of bin `i` in dBFS; bin 0 is the lowest frequency
/// in the tile (FFT-shifted so the spectrum is monotonic). Use [`PsdFrame::bin_hz_center`]
/// to map a bin to an absolute frequency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PsdFrame {
    /// Center frequency the sensor was tuned to for this dwell.
    pub tile_center_hz: Hz,
    /// Frequency resolution per bin (sample_rate / fft_size).
    pub bin_hz: Hz,
    /// Power per bin in dBFS, low frequency first.
    pub psd_dbfs: Vec<f32>,
    pub t_unix_ns: UnixNanos,
    pub sensor: SensorId,
}

impl PsdFrame {
    /// Absolute center frequency of bin `i`.
    pub fn bin_hz_center(&self, i: usize) -> Hz {
        let n = self.psd_dbfs.len() as f64;
        let low_edge = self.tile_center_hz - (n / 2.0) * self.bin_hz;
        low_edge + (i as f64 + 0.5) * self.bin_hz
    }
}

/// A detected signal of interest, produced by the CFAR stage and persisted to the
/// catalog. This is the atomic survey output that accretes into the emitter catalog
/// and the pattern-of-life timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    /// Estimated center frequency of the signal.
    pub center_hz: Hz,
    /// 99%-energy occupied bandwidth.
    pub bandwidth_hz: Hz,
    /// Peak power in dBFS.
    pub power_dbfs: f32,
    /// Signal-to-noise ratio above the local CFAR noise estimate, in dB.
    pub snr_db: f32,
    pub t_unix_ns: UnixNanos,
    /// Tile this detection was found in (for provenance / dedup across tiles).
    pub tile_center_hz: Hz,
    pub sensor: SensorId,
}

impl Detection {
    /// Lower / upper edges implied by `center_hz ± bandwidth/2`.
    pub fn band(&self) -> (Hz, Hz) {
        let half = self.bandwidth_hz / 2.0;
        (self.center_hz - half, self.center_hz + half)
    }
}

// ---------------------------------------------------------------------------
// Mission domain
// ---------------------------------------------------------------------------

/// A named frequency span the mission surveys. The scheduler tiles each band into
/// sensor-bandwidth-sized chunks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub name: String,
    pub low_hz: Hz,
    pub high_hz: Hz,
}

/// Mission lifecycle. Persisted so a stopped mission is resumable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissionPhase {
    Created,
    Running,
    Paused,
    Stopped,
    Complete,
}

// ---------------------------------------------------------------------------
// Event bus
// ---------------------------------------------------------------------------

/// Everything that flows over `rf-bus`. PSD frames are lossy telemetry (drop-oldest
/// under backpressure); detections and state changes are lossless (they must reach
/// the catalog and the UI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BusEvent {
    /// Live spectrum telemetry — lossy.
    Psd(PsdFrame),
    /// A detected signal of interest — lossless, persisted.
    Detection(Detection),
    /// A sensor changed state — lossless.
    SensorStatus { id: SensorId, state: SensorState },
    /// A mission changed phase — lossless.
    MissionState { id: MissionId, phase: MissionPhase },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_cover_only_in_range() {
        let caps = SensorCapabilities {
            freq_min_hz: 24e6,
            freq_max_hz: 1.766e9,
            max_bandwidth_hz: 2.4e6,
            sample_formats: vec![SampleFormat::Cf32],
            gain_min_db: 0.0,
            gain_max_db: 49.6,
        };
        assert!(caps.covers(150e6, 2.0e6));
        // bandwidth wider than the device supports
        assert!(!caps.covers(150e6, 3.0e6));
        // tile would run past the upper tuning edge
        assert!(!caps.covers(1.766e9, 2.0e6));
        // below the lower tuning edge
        assert!(!caps.covers(24e6, 2.0e6));
    }

    #[test]
    fn psd_bin_to_frequency_is_monotonic_and_centered() {
        let frame = PsdFrame {
            tile_center_hz: 100e6,
            bin_hz: 1e3,
            psd_dbfs: vec![0.0; 1024],
            t_unix_ns: 0,
            sensor: SensorId(0),
        };
        let f0 = frame.bin_hz_center(0);
        let f_last = frame.bin_hz_center(1023);
        assert!(f0 < frame.tile_center_hz);
        assert!(f_last > frame.tile_center_hz);
        // midpoint of the band sits at the tile center (within half a bin)
        let mid = (frame.bin_hz_center(511) + frame.bin_hz_center(512)) / 2.0;
        assert!((mid - frame.tile_center_hz).abs() < frame.bin_hz);
    }

    #[test]
    fn detection_band_edges() {
        let d = Detection {
            center_hz: 462.5625e6,
            bandwidth_hz: 12.5e3,
            power_dbfs: -42.0,
            snr_db: 18.0,
            t_unix_ns: 0,
            tile_center_hz: 462.0e6,
            sensor: SensorId(1),
        };
        let (lo, hi) = d.band();
        assert!((lo - 462.55625e6).abs() < 1.0);
        assert!((hi - 462.56875e6).abs() < 1.0);
    }
}
