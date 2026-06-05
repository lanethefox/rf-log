//! Real SDR hardware backend via SoapySDR (RTL-SDR today; HackRF/Airspy later through
//! the same factory). Gated behind the `soapy` feature.
//!
//! Replaces the v1 teardown hack: v1 did `std::mem::forget(dev)` on disconnect to dodge
//! a SoapySDR/USB cleanup segfault, leaking the device + stream every cycle. Here the
//! Rust `soapysdr` RAII types own cleanup, and [`Drop`] deactivates the stream first —
//! no leak.

use crate::{IqSensor, SensorError};
use num_complex::Complex32;
use rf_types::{Hz, SampleFormat, SensorCapabilities, SensorId};
use soapysdr::{Device, Direction, ErrorCode, RxStream};

const RX: Direction = Direction::Rx;
const CH: usize = 0;

/// A discovered SoapySDR device.
#[derive(Debug, Clone)]
pub struct SoapyDeviceInfo {
    /// Device-open argument string (e.g. `driver=rtlsdr,serial=...`).
    pub args: String,
    pub label: String,
}

/// Enumerate RTL-SDR devices. Returns empty (not an error) when none are attached.
pub fn enumerate() -> Vec<SoapyDeviceInfo> {
    soapysdr::enumerate("driver=rtlsdr")
        .unwrap_or_default()
        .into_iter()
        .map(|args| SoapyDeviceInfo {
            label: args.get("label").map(|s| s.to_string()).unwrap_or_default(),
            args: args.to_string(),
        })
        .collect()
}

/// A live RTL-SDR (or other SoapySDR) receiver behind the [`IqSensor`] trait.
pub struct SoapySdrSensor {
    id: SensorId,
    caps: SensorCapabilities,
    sample_rate: f64,
    center: f64,
    dev: Device,
    stream: RxStream<Complex32>,
}

impl SoapySdrSensor {
    /// Open the device described by `args`, set the sample rate, and start its RX stream.
    pub fn open(id: SensorId, args: &str, sample_rate: f64) -> Result<Self, SensorError> {
        let dev = Device::new(args).map_err(io)?;
        dev.set_sample_rate(RX, CH, sample_rate).map_err(io)?;

        let (fmin, fmax) = match dev.frequency_range(RX, CH) {
            Ok(ranges) if !ranges.is_empty() => (
                ranges
                    .iter()
                    .map(|r| r.minimum)
                    .fold(f64::INFINITY, f64::min),
                ranges
                    .iter()
                    .map(|r| r.maximum)
                    .fold(f64::NEG_INFINITY, f64::max),
            ),
            _ => (24e6, 1.766e9), // RTL-SDR R820T2 fallback
        };
        let (gmin, gmax) = match dev.gain_range(RX, CH) {
            Ok(r) => (r.minimum as f32, r.maximum as f32),
            _ => (0.0, 49.6),
        };

        let mut stream = dev.rx_stream::<Complex32>(&[CH]).map_err(io)?;
        stream.activate(None).map_err(io)?;

        let caps = SensorCapabilities {
            freq_min_hz: fmin,
            freq_max_hz: fmax,
            max_bandwidth_hz: sample_rate,
            sample_formats: vec![SampleFormat::Cf32],
            gain_min_db: gmin,
            gain_max_db: gmax,
        };
        Ok(Self {
            id,
            caps,
            sample_rate,
            center: (fmin + fmax) / 2.0,
            dev,
            stream,
        })
    }
}

impl IqSensor for SoapySdrSensor {
    fn id(&self) -> SensorId {
        self.id
    }
    fn capabilities(&self) -> &SensorCapabilities {
        &self.caps
    }
    fn sample_rate(&self) -> Hz {
        self.sample_rate
    }

    fn tune(&mut self, center_hz: Hz) -> Result<(), SensorError> {
        if !self.caps.covers(center_hz, self.sample_rate) {
            return Err(SensorError::UnsupportedFreq(center_hz));
        }
        self.dev.set_frequency(RX, CH, center_hz, ()).map_err(io)?;
        self.center = center_hz;
        Ok(())
    }

    fn set_gain(&mut self, db: f32) -> Result<(), SensorError> {
        self.dev.set_gain(RX, CH, db as f64).map_err(io)
    }

    fn read(&mut self, out: &mut [Complex32]) -> Result<usize, SensorError> {
        match self.stream.read(&mut [out], 200_000) {
            Ok(n) => Ok(n),
            // Timeout / overflow are non-fatal: report "no data now", caller retries.
            Err(e) if matches!(e.code, ErrorCode::Timeout | ErrorCode::Overflow) => Ok(0),
            Err(e) => Err(SensorError::Io(e.to_string())),
        }
    }
}

impl Drop for SoapySdrSensor {
    fn drop(&mut self) {
        // Clean teardown — the fix for the v1 mem::forget leak.
        let _ = self.stream.deactivate(None);
    }
}

fn io(e: soapysdr::Error) -> SensorError {
    SensorError::Io(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerate_never_panics_without_hardware() {
        // No dongle in CI → empty; with an RTL-SDR attached → at least one.
        let devices = enumerate();
        println!("soapy rtlsdr devices found: {}", devices.len());
    }
}
