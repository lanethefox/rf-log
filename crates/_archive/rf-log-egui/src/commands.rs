//! Direct backend command dispatch.
//!
//! Instead of Tauri IPC, we call `state.update_config()` directly.
//! The existing `config_poller` detects changes and dispatches DspCommands
//! to the hardware/DSP pipeline.

use rf_web::AppState;

pub fn set_mode(state: &AppState, mode: &str) {
    state.update_config(|c| {
        c.mode = mode.to_string();
    });
    tracing::debug!("Mode set to: {}", mode);
}

pub fn set_freq(state: &AppState, freq_mhz: f64) {
    state.update_config(|c| {
        c.freq = freq_mhz;
    });
    tracing::debug!("Frequency set to: {:.4} MHz", freq_mhz);
}

pub fn set_gain(state: &AppState, gain: f64) {
    state.update_config(|c| {
        c.gain = gain;
    });
    tracing::debug!("Gain set to: {:.1} dB", gain);
}

pub fn set_modulation(state: &AppState, modulation: &str) {
    state.update_config(|c| {
        c.modulation = modulation.to_string();
    });
    tracing::debug!("Modulation set to: {}", modulation);
}

pub fn set_squelch(state: &AppState, level: f64) {
    state.update_config(|c| {
        c.squelch = level;
    });
    tracing::debug!("Squelch set to: {:.0} dB", level);
}

pub fn set_volume(state: &AppState, volume: u8, muted: bool) {
    state.update_config(|c| {
        c.volume = volume;
        c.muted = muted;
    });
    tracing::debug!("Volume: {}, Muted: {}", volume, muted);
}

pub fn set_threshold(state: &AppState, threshold: f64) {
    state.update_config(|c| {
        c.threshold = threshold;
    });
    tracing::debug!("Threshold set to: {:.1} dBFS", threshold);
}

pub fn set_bands(state: &AppState, bands: Vec<String>) {
    state.update_config(|c| {
        c.bands = bands.clone();
    });
    tracing::debug!("Bands set to: {:?}", bands);
}

pub fn set_scanning(state: &AppState, scanning: bool) {
    state.update_config(|c| {
        c.scanning = scanning;
    });
    tracing::debug!("Scanning: {}", scanning);
}

pub fn set_snr_margin(state: &AppState, margin: f64) {
    state.update_config(|c| {
        c.snr_margin = margin;
    });
    tracing::debug!("SNR margin set to: {:.1} dB", margin);
}

pub fn set_debug_logging(state: &AppState, enabled: bool) {
    state.update_config(|c| {
        c.debug_logging = enabled;
    });
    tracing::debug!("Debug logging: {}", enabled);
}

pub fn set_network_scan(state: &AppState, active: bool) {
    state.update_config(|c| {
        c.network_scan_active = active;
    });
    tracing::debug!("Network scan: {}", active);
}

pub fn set_network_scan_mode(state: &AppState, mode: &str) {
    state.update_config(|c| {
        c.network_scan_mode = mode.to_string();
    });
    tracing::debug!("Network scan mode: {}", mode);
}

pub fn set_dept_hold(state: &AppState, dept: Option<String>) {
    state.update_config(|c| {
        c.network_scan_dept_hold = dept.clone();
    });
    tracing::debug!("Department hold: {:?}", dept);
}

/// Set GPS source (external, simulation, fixed, none)
pub fn set_gps_source(state: &AppState, source: &str) {
    state.update_config(|c| {
        c.gps_source = source.to_string();
    });
    tracing::info!("GPS source set to: {}", source);
}

/// Set fixed GPS position
pub fn set_gps_fixed(state: &AppState, lat: f64, lon: f64) {
    state.update_config(|c| {
        c.gps_source = "fixed".to_string();
        c.fixed_lat = Some(lat);
        c.fixed_lon = Some(lon);
    });
    tracing::info!("GPS fixed position set to: {:.6}, {:.6}", lat, lon);
}

/// Infer demod mode from frequency band
fn infer_modulation(freq_mhz: f64) -> &'static str {
    if freq_mhz >= 87.5 && freq_mhz <= 108.0 {
        "WFM"
    } else if freq_mhz >= 0.5 && freq_mhz <= 1.7 {
        "AM"
    } else if freq_mhz >= 108.0 && freq_mhz <= 137.0 {
        "AM" // aviation
    } else {
        "NFM"
    }
}

/// Click-to-monitor: tune to frequency, infer modulation, switch to monitor mode
pub fn monitor_signal(state: &AppState, freq_mhz: f64) {
    let modulation = infer_modulation(freq_mhz).to_string();
    state.update_config(|c| {
        c.freq = freq_mhz;
        c.modulation = modulation.clone();
        c.mode = "monitor".to_string();
        c.scanning = true;
    });
    tracing::info!("Monitor signal: {:.4} MHz ({})", freq_mhz, modulation);
}

/// Set CC frequency for network scanner
pub fn set_cc_freq(state: &AppState, cc_freq: f64) {
    state.update_config(|c| {
        c.network_scan_cc_freq = Some(cc_freq);
        // Find matching index in cc_list
        if let Some(idx) = c.network_scan_cc_list.iter().position(|&f| (f - cc_freq).abs() < 0.001) {
            c.network_scan_cc_index = idx;
        }
    });
    tracing::info!("CC frequency set to: {:.5} MHz", cc_freq);
}

/// Set CC dwell time
pub fn set_cc_dwell(state: &AppState, dwell_secs: f64) {
    let clamped = dwell_secs.clamp(1.5, 30.0);
    state.update_config(|c| {
        c.network_scan_cc_dwell = clamped;
    });
    tracing::debug!("CC dwell set to: {:.1}s", clamped);
}
