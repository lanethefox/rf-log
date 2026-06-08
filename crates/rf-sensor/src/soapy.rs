//! Real SDR hardware backend via SoapySDR (RTL-SDR today; HackRF/Airspy later through
//! the same factory). Gated behind the `soapy` feature.
//!
//! Two stages so the [`DeviceManager`](crate::DeviceManager) can hold a device "Ready"
//! (USB claimed) without streaming, and only activate the RX stream when a mission
//! allocates it — no open/close churn:
//! - [`SoapyDevice`] — opened device, no active stream.
//! - [`SoapySdrSensor`] — activated stream, implements [`IqSensor`].
//!
//! Teardown is RAII (the v1 `mem::forget` leak is gone); [`Drop`] deactivates the stream.

use crate::{IqSensor, SensorError};
use num_complex::Complex32;
use rf_types::{Hz, SampleFormat, SensorCapabilities, SensorId};
use soapysdr::{Device, Direction, ErrorCode, RxStream};

const RX: Direction = Direction::Rx;
const CH: usize = 0;

/// A discovered SoapySDR device (from enumeration).
#[derive(Debug, Clone)]
pub struct SoapyDeviceInfo {
    pub args: String,
    pub serial: String,
    pub driver: String,
    pub label: String,
}

/// Enumerate RTL-SDR devices. Returns empty (not an error) when none are attached.
pub fn enumerate() -> Vec<SoapyDeviceInfo> {
    soapysdr::enumerate("driver=rtlsdr")
        .unwrap_or_default()
        .into_iter()
        .map(|args| {
            let get = |k: &str| args.get(k).map(|s| s.to_string()).unwrap_or_default();
            SoapyDeviceInfo {
                serial: get("serial"),
                driver: get("driver"),
                label: get("label"),
                args: args.to_string(),
            }
        })
        .collect()
}

/// An opened device with its USB handle claimed but **no active RX stream** — the idle
/// "Ready" state the manager parks devices in.
pub struct SoapyDevice {
    id: SensorId,
    caps: SensorCapabilities,
    sample_rate: f64,
    dev: Device,
}

impl SoapyDevice {
    /// Open `args`, set the sample rate, and read capabilities. Does not stream.
    pub fn open(id: SensorId, args: &str, sample_rate: f64) -> Result<Self, SensorError> {
        let dev = Device::new(args).map_err(io)?;
        dev.set_sample_rate(RX, CH, sample_rate).map_err(io)?;
        let (fmin, fmax) = match dev.frequency_range(RX, CH) {
            Ok(r) if !r.is_empty() => (
                r.iter().map(|x| x.minimum).fold(f64::INFINITY, f64::min),
                r.iter()
                    .map(|x| x.maximum)
                    .fold(f64::NEG_INFINITY, f64::max),
            ),
            _ => (24e6, 1.766e9), // RTL-SDR R820T2 fallback
        };
        let (gmin, gmax) = match dev.gain_range(RX, CH) {
            Ok(r) => (r.minimum as f32, r.maximum as f32),
            _ => (0.0, 49.6),
        };
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
            dev,
        })
    }

    pub fn capabilities(&self) -> &SensorCapabilities {
        &self.caps
    }

    /// Apply gain config: automatic AGC, or a manual dB value.
    pub fn set_gain_config(&self, auto: bool, gain_db: f32) -> Result<(), SensorError> {
        self.dev.set_gain_mode(RX, CH, auto).map_err(io)?;
        if !auto {
            self.dev.set_gain(RX, CH, gain_db as f64).map_err(io)?;
        }
        Ok(())
    }

    /// Activate the RX stream, turning this into a streaming [`SoapySdrSensor`].
    pub fn activate(self) -> Result<SoapySdrSensor, SensorError> {
        let mut stream = self.dev.rx_stream::<Complex32>(&[CH]).map_err(io)?;
        stream.activate(None).map_err(io)?;
        let center = (self.caps.freq_min_hz + self.caps.freq_max_hz) / 2.0;
        Ok(SoapySdrSensor {
            id: self.id,
            caps: self.caps,
            sample_rate: self.sample_rate,
            center,
            dev: self.dev,
            stream,
        })
    }
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
        let devices = enumerate();
        println!("soapy rtlsdr devices found: {}", devices.len());
    }
}
