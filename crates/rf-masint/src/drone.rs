use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DroneDetectionMethod {
    RfSignature,
    RemoteId,
    WifiSsid,
    BleMac,
    WifiMac,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroneDetection {
    pub timestamp: f64,
    pub method: DroneDetectionMethod,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub confidence: f32,
    pub signal_strength_dbm: Option<f32>,
    pub freq_mhz: Option<f64>,
    /// Source sensor ID (workstation, Pi Zero, ESP32 node name)
    pub sensor_id: String,
}

/// A known drone RF signature for pattern matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroneSignature {
    pub manufacturer: String,
    pub model: String,
    /// Control link frequency ranges in MHz (min, max)
    pub freq_ranges_mhz: Vec<(f64, f64)>,
    /// Approximate occupied bandwidth in MHz
    pub bandwidth_mhz: f64,
    pub notes: Option<String>,
}

/// Built-in drone signature library (P1 seed data).
pub fn builtin_signatures() -> Vec<DroneSignature> {
    vec![
        DroneSignature {
            manufacturer: "DJI".into(),
            model: "OcuSync 2 (Mavic 2/Mini 2/Air 2)".into(),
            freq_ranges_mhz: vec![(2400.0, 2483.5), (5725.0, 5850.0)],
            bandwidth_mhz: 10.0,
            notes: Some("OFDM hopping, DroneID frames ~600ms interval on 2.4 GHz".into()),
        },
        DroneSignature {
            manufacturer: "DJI".into(),
            model: "OcuSync 3 (Mini 3/Air 3/Mavic 3)".into(),
            freq_ranges_mhz: vec![(2400.0, 2483.5), (5725.0, 5850.0)],
            bandwidth_mhz: 10.0,
            notes: Some("Similar to OcuSync 2, enhanced anti-interference".into()),
        },
        DroneSignature {
            manufacturer: "DJI".into(),
            model: "FPV (analog 5.8 GHz)".into(),
            freq_ranges_mhz: vec![(5645.0, 5945.0)],
            bandwidth_mhz: 20.0,
            notes: Some("Analog video downlink, wideband".into()),
        },
        DroneSignature {
            manufacturer: "Autel".into(),
            model: "EVO II series".into(),
            freq_ranges_mhz: vec![(2400.0, 2483.5), (5725.0, 5850.0)],
            bandwidth_mhz: 10.0,
            notes: None,
        },
        DroneSignature {
            manufacturer: "FPV/DIY".into(),
            model: "ExpressLRS (ELRS 900 MHz)".into(),
            freq_ranges_mhz: vec![(902.0, 928.0)],
            bandwidth_mhz: 0.5,
            notes: Some("Sub-GHz control link, LoRa-based".into()),
        },
        DroneSignature {
            manufacturer: "FPV/DIY".into(),
            model: "ExpressLRS (ELRS 2.4 GHz)".into(),
            freq_ranges_mhz: vec![(2400.0, 2483.5)],
            bandwidth_mhz: 1.0,
            notes: Some("2.4 GHz FHSS control link".into()),
        },
    ]
}

/// Match detected spectrum activity against the drone signature library.
/// Returns a match if any signature's frequency range overlaps the detected band.
pub fn match_signature(
    freq_mhz: f64,
    bandwidth_mhz: f64,
    library: &[DroneSignature],
) -> Option<(&DroneSignature, f32)> {
    let freq_lo = freq_mhz - bandwidth_mhz / 2.0;
    let freq_hi = freq_mhz + bandwidth_mhz / 2.0;

    library.iter().find_map(|sig| {
        let matched = sig.freq_ranges_mhz.iter().any(|(lo, hi)| {
            freq_lo <= *hi && freq_hi >= *lo
        });
        if matched {
            // Confidence based on bandwidth match
            let bw_ratio = (bandwidth_mhz / sig.bandwidth_mhz).min(sig.bandwidth_mhz / bandwidth_mhz);
            let confidence = 0.5 + 0.5 * bw_ratio as f32;
            Some((sig, confidence.min(1.0)))
        } else {
            None
        }
    })
}
