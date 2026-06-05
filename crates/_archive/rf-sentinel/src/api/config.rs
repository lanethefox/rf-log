use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentinelConfig {
    pub anomaly_z_threshold: f32,
    pub drone_alert_mode: String, // "always" | "unknown_only" | "never"
    pub scan_bands: Vec<ScanBand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanBand {
    pub name: String,
    pub freq_start_mhz: f64,
    pub freq_end_mhz: f64,
    pub enabled: bool,
}

impl Default for SentinelConfig {
    fn default() -> Self {
        Self {
            anomaly_z_threshold: 3.0,
            drone_alert_mode: "unknown_only".into(),
            scan_bands: vec![
                ScanBand { name: "VHF Low".into(),   freq_start_mhz: 25.0,   freq_end_mhz: 88.0,   enabled: true },
                ScanBand { name: "FM Broadcast".into(), freq_start_mhz: 88.0, freq_end_mhz: 108.0, enabled: true },
                ScanBand { name: "VHF High".into(),  freq_start_mhz: 108.0,  freq_end_mhz: 174.0,  enabled: true },
                ScanBand { name: "UHF".into(),        freq_start_mhz: 400.0,  freq_end_mhz: 512.0,  enabled: true },
                ScanBand { name: "700/800 MHz".into(), freq_start_mhz: 700.0, freq_end_mhz: 900.0,  enabled: true },
                ScanBand { name: "L-Band".into(),     freq_start_mhz: 1215.0, freq_end_mhz: 1400.0, enabled: true },
                ScanBand { name: "2.4 GHz ISM".into(), freq_start_mhz: 2400.0, freq_end_mhz: 2484.0, enabled: true },
                ScanBand { name: "S-Band Radar".into(), freq_start_mhz: 2700.0, freq_end_mhz: 3100.0, enabled: true },
                ScanBand { name: "5.8 GHz ISM".into(), freq_start_mhz: 5725.0, freq_end_mhz: 5850.0, enabled: true },
            ],
        }
    }
}

pub async fn get_config(State(state): State<AppState>) -> Json<SentinelConfig> {
    let cfg = state.config.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Json(cfg)
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(body): Json<SentinelConfig>,
) -> StatusCode {
    let mut cfg = state.config.lock().unwrap_or_else(|p| p.into_inner());
    *cfg = body;
    StatusCode::OK
}
