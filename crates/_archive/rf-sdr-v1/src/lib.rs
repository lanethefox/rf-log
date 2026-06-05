mod ring_buffer;
mod simulated;
mod reader;
mod soapy;

pub use ring_buffer::{IqRingBuffer, IqProducer, IqConsumer};
pub use simulated::SimulatedSdr;
pub use soapy::SoapySdr;
pub use reader::{spawn_sdr_reader, spawn_sdr_reader_with_iq_mirror};

use num_complex::Complex32;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One IQ sample
pub type IqSample = Complex32;

/// SDR hardware status (for UI display) — primary device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdrStatus {
    pub detected: bool,
    pub driver: String,
    pub serial: String,
    pub sample_rate: f64,
}

/// Info about a single detected SDR device (for multi-SDR enumeration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdrDeviceInfo {
    pub driver: String,
    pub label: String,
    pub serial: String,
    /// All SoapySDR-reported properties for this device.
    pub args: HashMap<String, String>,
}

/// SDR configuration parameters
#[derive(Debug, Clone)]
pub struct SdrConfig {
    pub freq: f64,
    pub sample_rate: f64,
    pub gain: f64,
    pub bandwidth: f64,
}

impl Default for SdrConfig {
    fn default() -> Self {
        Self {
            freq: 146.52e6,
            sample_rate: 2_400_000.0,
            gain: 40.0,
            bandwidth: 2_400_000.0,
        }
    }
}

/// Commands sent to the SDR reader thread
#[derive(Debug)]
pub enum SdrCommand {
    SetFreq(f64),
    SetGain(f64),
    /// Enable/disable automatic gain control.
    SetAgc(bool),
    /// Set frequency correction in PPM.
    SetPpm(f64),
    /// Enable/disable offset tuning (avoids DC spike).
    SetOffsetTuning(bool),
    Stop,
}

/// Trait for SDR device backends
pub trait SdrDevice: Send {
    fn set_freq(&mut self, freq: f64) -> Result<(), String>;
    fn set_gain(&mut self, gain: f64) -> Result<(), String>;
    fn read_iq(&mut self, buf: &mut [IqSample]) -> Result<usize, String>;
    /// Returns true if this is a simulation backend (no real hardware).
    fn is_simulated(&self) -> bool { false }
    /// Enable/disable automatic gain control. Default: no-op.
    fn set_agc(&mut self, _enabled: bool) -> Result<(), String> { Ok(()) }
    /// Set frequency correction in PPM. Default: no-op.
    fn set_ppm(&mut self, _ppm: f64) -> Result<(), String> { Ok(()) }
    /// Enable/disable offset tuning. Default: no-op.
    fn set_offset_tuning(&mut self, _enabled: bool) -> Result<(), String> { Ok(()) }
}

/// Enumerate all SDR devices via SoapySDR.
pub fn enumerate_all() -> Vec<SdrDeviceInfo> {
    soapy::enumerate_soapy()
        .into_iter()
        .enumerate()
        .map(|(idx, d)| {
            tracing::info!("SDR detected: [{}] {} (serial: {}) — {:?}", idx, d.label, d.serial, d.args);
            let mut args: HashMap<String, String> = d.args.into_iter().collect();
            // Inject the driver-specific device index if not already present.
            // SoapyRTLSDR uses "rtl=N" to target a specific device by index.
            let driver = d.driver.clone();
            let idx_key = match driver.as_str() {
                "rtlsdr" => "rtl",
                _ => "index",
            };
            args.entry(idx_key.to_string()).or_insert_with(|| idx.to_string());
            SdrDeviceInfo {
                driver,
                label: d.label,
                serial: d.serial,
                args,
            }
        })
        .collect()
}

/// Detect SDR hardware via SoapySDR.
/// Returns a status struct for the primary device; detected=false means simulation mode.
pub fn enumerate() -> SdrStatus {
    let devices = soapy::enumerate_soapy();
    if let Some(dev) = devices.first() {
        tracing::info!("SDR detected via SoapySDR: {} (serial: {})", dev.label, dev.serial);
        SdrStatus {
            detected: true,
            driver: dev.label.clone(),
            serial: dev.serial.clone(),
            sample_rate: 2_400_000.0,
        }
    } else {
        SdrStatus {
            detected: false,
            driver: String::new(),
            serial: String::new(),
            sample_rate: 2_400_000.0,
        }
    }
}
