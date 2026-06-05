use serde::{Deserialize, Serialize};

/// A band definition for scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandDef {
    pub key: String,
    pub start: f64,
    pub end: f64,
    pub label: String,
    pub color: String,
}

/// Load band definitions matching the frontend BANDS constant.
pub fn default_bands() -> Vec<BandDef> {
    vec![
        BandDef {
            key: "AM".into(),
            start: 0.530,
            end: 1.700,
            label: "AM 530-1700k".into(),
            color: "#d4a843".into(),
        },
        BandDef {
            key: "HF".into(),
            start: 3.0,
            end: 30.0,
            label: "HF 3-30".into(),
            color: "#cc66ff".into(),
        },
        BandDef {
            key: "FM".into(),
            start: 88.0,
            end: 108.0,
            label: "FM 88-108".into(),
            color: "#44aadd".into(),
        },
        BandDef {
            key: "VHF".into(),
            start: 136.0,
            end: 174.0,
            label: "VHF 136-174".into(),
            color: "#33cc33".into(),
        },
        BandDef {
            key: "FEDV".into(),
            start: 163.0,
            end: 173.0,
            label: "FEDV 163-173".into(),
            color: "#ff6699".into(),
        },
        BandDef {
            key: "BIII".into(),
            start: 174.0,
            end: 230.0,
            label: "BIII 174-230".into(),
            color: "#00cc99".into(),
        },
        BandDef {
            key: "UHF".into(),
            start: 400.0,
            end: 512.0,
            label: "UHF 400-512".into(),
            color: "#c8a000".into(),
        },
        BandDef {
            key: "GMRS".into(),
            start: 462.0,
            end: 467.7125,
            label: "GMRS 462-467".into(),
            color: "#ff9933".into(),
        },
        BandDef {
            key: "P25".into(),
            start: 769.0,
            end: 860.0,
            label: "P25 769-860".into(),
            color: "#ff3333".into(),
        },
    ]
}
