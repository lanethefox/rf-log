use crate::{IqSample, SdrDevice};
use soapysdr::{Device, Direction, RxStream};
use num_complex::Complex32;
use std::collections::HashMap;

/// SoapySDR device wrapper implementing the SdrDevice trait.
pub struct SoapySdr {
    device: Device,
    stream: RxStream<Complex32>,
    #[allow(dead_code)]
    sample_rate: f64,
}

/// Information about a detected SoapySDR device.
pub struct SoapyDeviceInfo {
    pub driver: String,
    pub label: String,
    pub serial: String,
    /// All key-value pairs reported by SoapySDR for this device.
    pub args: Vec<(String, String)>,
}

/// Drivers to query during enumeration.
/// Explicitly list supported drivers to avoid probing broken/missing ones
/// (e.g. sdrplay_api crashes with a structured exception when the service isn't running).
const ENUMERATE_DRIVERS: &[&str] = &["rtlsdr"];

/// Enumerate SDR devices via SoapySDR.
///
/// First tries normal enumeration. If that returns zero devices (common when
/// `rtlsdr_get_device_usb_strings()` fails due to stale WinUSB ghost entries),
/// falls back to probing by index — opens each device directly with `rtl=N`
/// and queries its hardware info from the opened handle.
pub fn enumerate_soapy() -> Vec<SoapyDeviceInfo> {
    let mut all_args = Vec::new();
    for driver in ENUMERATE_DRIVERS {
        let query = format!("driver={}", driver);
        match soapysdr::enumerate(query.as_str()) {
            Ok(found) => {
                tracing::info!("SoapySDR enumerate(driver={}) returned {} device(s)", driver, found.len());
                all_args.extend(found);
            }
            Err(e) => {
                tracing::warn!("SoapySDR enumerate(driver={}) failed: {}", driver, e);
            }
        }
    }
    let mut devices = Vec::new();

    for args in &all_args {
        let driver = args.get("driver").unwrap_or("").to_string();
        let label = args.get("label").unwrap_or("").to_string();
        let serial = args.get("serial").unwrap_or("").to_string();

        // Capture all SoapySDR-reported properties
        let all_args: Vec<(String, String)> = args.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        devices.push(SoapyDeviceInfo {
            driver,
            label,
            serial,
            args: all_args,
        });
    }

    if devices.is_empty() {
        tracing::warn!("Normal enumeration found 0 devices — trying index-based probe");
        devices = probe_by_index();
    }

    devices
}

/// Fallback: probe RTL-SDR devices by opening them directly via index.
///
/// When `rtlsdr_get_device_usb_strings()` fails (stale WinUSB ghost entries on
/// Windows), SoapySDR's enumerate returns nothing even though the devices are
/// physically present and functional. This bypasses enumeration entirely by
/// attempting to open `driver=rtlsdr,rtl=N` for N=0..MAX and querying hardware
/// info from the opened device handle.
fn probe_by_index() -> Vec<SoapyDeviceInfo> {
    const MAX_PROBE: usize = 8;
    let mut devices = Vec::new();

    for idx in 0..MAX_PROBE {
        let query = format!("driver=rtlsdr,rtl={}", idx);
        tracing::info!("Probing SDR by index: {}", query);

        match Device::new(query.as_str()) {
            Ok(device) => {
                // Query info from the opened device
                let driver_key = device.driver_key().unwrap_or_else(|_| "rtlsdr".into());
                let hw_key = device.hardware_key().unwrap_or_else(|_| "unknown".into());
                let hw_info = device.hardware_info().unwrap_or_default();

                let serial = hw_info.get("serial")
                    .map(String::from)
                    .unwrap_or_default();
                let product = hw_info.get("product")
                    .map(String::from)
                    .unwrap_or_else(|| hw_key.clone());
                let tuner = hw_info.get("tuner")
                    .map(String::from)
                    .unwrap_or_default();
                let manufacturer = hw_info.get("manufacturer")
                    .map(String::from)
                    .unwrap_or_default();

                let label = if !product.is_empty() {
                    format!("{} :: {}", product, serial)
                } else {
                    format!("RTL-SDR #{}", idx)
                };

                tracing::info!(
                    "Probe hit: [{}] driver={}, hw={}, serial={}, tuner={}",
                    idx, driver_key, hw_key, serial, tuner,
                );

                let mut args = Vec::new();
                args.push(("driver".into(), driver_key.clone()));
                args.push(("rtl".into(), idx.to_string()));
                args.push(("label".into(), label.clone()));
                if !serial.is_empty() {
                    args.push(("serial".into(), serial.clone()));
                }
                if !product.is_empty() {
                    args.push(("product".into(), product));
                }
                if !manufacturer.is_empty() {
                    args.push(("manufacturer".into(), manufacturer));
                }
                if !tuner.is_empty() {
                    args.push(("tuner".into(), tuner));
                }

                devices.push(SoapyDeviceInfo {
                    driver: driver_key,
                    label,
                    serial,
                    args,
                });

                // Device is dropped here — closes the handle so pool can reopen it
            }
            Err(e) => {
                tracing::info!("Probe index {} failed (no more devices): {}", idx, e);
                break;
            }
        }
    }

    if !devices.is_empty() {
        tracing::info!(
            "Index-based probe found {} device(s) (bypassed broken USB string enumeration)",
            devices.len()
        );
    }

    devices
}

impl SoapySdr {
    /// Open the first non-audio SoapySDR device.
    pub fn open(sample_rate: f64) -> Result<Self, String> {
        let mut devices = Vec::new();
        for driver in ENUMERATE_DRIVERS {
            let query = format!("driver={}", driver);
            if let Ok(found) = soapysdr::enumerate(query.as_str()) {
                devices.extend(found);
            }
        }

        let args = devices.into_iter()
            .next()
            .ok_or_else(|| "No SDR devices found via SoapySDR".to_string())?;

        let driver = args.get("driver").unwrap_or("").to_string();
        let label = args.get("label").unwrap_or("").to_string();
        tracing::info!("Opening SoapySDR device: {} ({})", label, driver);

        let device = soapysdr::Device::new(args)
            .map_err(|e| format!("Failed to open SoapySDR device: {}", e))?;

        Self::configure_device(device, sample_rate, true)
    }

    /// Open a specific SDR device identified by its SoapySDR args.
    pub fn open_by_args(device_args: &HashMap<String, String>, sample_rate: f64) -> Result<Self, String> {
        // Only pass identifying keys to SoapySDR — extra metadata (label, product,
        // manufacturer, tuner) in the query can cause open failures with multiple devices.
        // Include driver-specific index keys: "rtl" for SoapyRTLSDR, "index" generic.
        const METADATA_KEYS: &[&str] = &["label", "product", "manufacturer", "tuner"];
        let query: String = device_args.iter()
            .filter(|(k, _)| !METADATA_KEYS.contains(&k.as_str()))
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");

        let label = device_args.get("label").map(|s| s.as_str()).unwrap_or("unknown");
        let driver = device_args.get("driver").map(|s| s.as_str()).unwrap_or("unknown");
        tracing::info!("Opening SoapySDR device by args: {} ({}) — {}", label, driver, query);

        let device = soapysdr::Device::new(query.as_str())
            .map_err(|e| format!("Failed to open SoapySDR device ({}): {}", query, e))?;

        Self::configure_device(device, sample_rate, true)
    }

    /// Open a specific SDR device WITHOUT activating the stream.
    /// Use `activate_stream()` to start IQ data flow after all devices are opened.
    /// This prevents USB bus disruption when opening multiple RTL-SDR devices.
    pub fn open_by_args_deferred(device_args: &HashMap<String, String>, sample_rate: f64) -> Result<Self, String> {
        const METADATA_KEYS: &[&str] = &["label", "product", "manufacturer", "tuner"];
        let query: String = device_args.iter()
            .filter(|(k, _)| !METADATA_KEYS.contains(&k.as_str()))
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");

        let label = device_args.get("label").map(|s| s.as_str()).unwrap_or("unknown");
        let driver = device_args.get("driver").map(|s| s.as_str()).unwrap_or("unknown");
        tracing::info!("Opening SoapySDR device (deferred): {} ({}) — {}", label, driver, query);

        let device = soapysdr::Device::new(query.as_str())
            .map_err(|e| format!("Failed to open SoapySDR device ({}): {}", query, e))?;

        Self::configure_device(device, sample_rate, false)
    }

    /// Activate the RX stream — starts USB bulk transfers and IQ data flow.
    pub fn activate_stream(&mut self) -> Result<(), String> {
        self.stream.activate(None)
            .map_err(|e| format!("Failed to activate stream: {}", e))?;

        let actual_rate = self.device.sample_rate(Direction::Rx, 0)
            .map_err(|e| format!("Failed to read sample rate: {}", e))?;
        let actual_freq = self.device.frequency(Direction::Rx, 0)
            .map_err(|e| format!("Failed to read frequency: {}", e))?;
        tracing::info!("SoapySDR stream activated: rate={} Hz, freq={} Hz", actual_rate, actual_freq);

        Ok(())
    }

    /// Internal: configure an already-opened Device and wrap into SoapySdr.
    /// If `activate` is true, the stream is activated immediately.
    fn configure_device(device: Device, sample_rate: f64, activate: bool) -> Result<Self, String> {
        device.set_sample_rate(Direction::Rx, 0, sample_rate)
            .map_err(|e| format!("Failed to set sample rate: {}", e))?;

        device.set_gain(Direction::Rx, 0, 20.0)
            .map_err(|e| format!("Failed to set gain: {}", e))?;

        // Set an initial frequency — non-fatal because some tuners (e.g. R828D with
        // older librtlsdr) don't support certain frequency ranges. The scan controller
        // will set the actual frequency immediately when scanning starts.
        let init_freq = 146_520_000.0;
        match device.set_frequency(Direction::Rx, 0, init_freq, ()) {
            Ok(()) => tracing::info!("SoapySDR initial frequency set to {} Hz", init_freq),
            Err(e) => tracing::warn!("SoapySDR initial frequency {} Hz failed (non-fatal, scan will set freq): {}", init_freq, e),
        }

        let stream = device.rx_stream::<Complex32>(&[0])
            .map_err(|e| format!("Failed to create RX stream: {}", e))?;

        let mut sdr = Self { device, stream, sample_rate };

        if activate {
            sdr.stream.activate(None)
                .map_err(|e| format!("Failed to activate stream: {}", e))?;

            let actual_rate = sdr.device.sample_rate(Direction::Rx, 0)
                .map_err(|e| format!("Failed to read sample rate: {}", e))?;
            let actual_freq = sdr.device.frequency(Direction::Rx, 0)
                .map_err(|e| format!("Failed to read frequency: {}", e))?;
            tracing::info!("SoapySDR stream active: rate={} Hz, freq={} Hz", actual_rate, actual_freq);
        }

        Ok(sdr)
    }
}

impl SdrDevice for SoapySdr {
    fn set_freq(&mut self, freq: f64) -> Result<(), String> {
        tracing::info!("SoapySDR set_frequency → {:.4} MHz ({} Hz)", freq / 1e6, freq);
        self.device.set_frequency(Direction::Rx, 0, freq, ())
            .map_err(|e| format!("SoapySDR set_frequency({} Hz / {:.4} MHz) failed: {}", freq, freq / 1e6, e))
    }

    fn set_gain(&mut self, gain: f64) -> Result<(), String> {
        self.device.set_gain(Direction::Rx, 0, gain)
            .map_err(|e| format!("SoapySDR set_gain failed: {}", e))
    }

    fn read_iq(&mut self, buf: &mut [IqSample]) -> Result<usize, String> {
        // Read with a 500ms timeout
        match self.stream.read(&mut [buf], 500_000) {
            Ok(n) => Ok(n),
            Err(soapysdr::Error { code: soapysdr::ErrorCode::Timeout, .. }) => Ok(0),
            Err(e) => Err(format!("SoapySDR read failed: {}", e)),
        }
    }

    fn set_agc(&mut self, enabled: bool) -> Result<(), String> {
        self.device.set_gain_mode(Direction::Rx, 0, enabled)
            .map_err(|e| format!("SoapySDR set_gain_mode({}) failed: {}", enabled, e))
    }

    fn set_ppm(&mut self, ppm: f64) -> Result<(), String> {
        self.device.write_setting("freq_corr", &format!("{}", ppm as i64))
            .map_err(|e| format!("SoapySDR write_setting(freq_corr, {} ppm) failed: {}", ppm, e))
    }

    fn set_offset_tuning(&mut self, enabled: bool) -> Result<(), String> {
        let val = if enabled { "true" } else { "false" };
        self.device.write_setting("offset_tune", val)
            .map_err(|e| format!("SoapySDR write_setting(offset_tune, {}) failed: {}", val, e))
    }
}

impl Drop for SoapySdr {
    fn drop(&mut self) {
        tracing::info!("Closing SoapySDR device");
        let _ = self.stream.deactivate(None);
    }
}
