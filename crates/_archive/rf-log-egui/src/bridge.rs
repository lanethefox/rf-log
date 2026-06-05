use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use serde_json::Value;
use tokio::sync::broadcast;

/// Parsed spectrum frame from broadcast channel
#[derive(Clone, Debug)]
pub struct SpectrumFrame {
    pub band: String,
    pub freqs: Vec<f64>,
    pub powers: Vec<f64>,
    pub noise_floor: f64,
    pub signals: Vec<DetectedSignal>,
}

/// A detected signal from spectrum scan
#[derive(Clone, Debug)]
pub struct DetectedSignal {
    pub freq_mhz: f64,
    pub power_db: f64,
    pub classification: String,
    pub name: String,
    pub mode: String,
}

/// A parsed protocol event (P25, TSBK, RDS, etc.)
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ProtocolEvent {
    pub event_type: String,
    pub timestamp: f64,
    pub raw: Arc<Value>,
    // P25-specific
    pub nac: Option<u32>,
    pub talkgroup: Option<u32>,
    pub source_unit: Option<u32>,
    pub encrypted: Option<bool>,
    // TSBK-specific
    pub opcode: Option<String>,
    // RDS-specific
    pub rds_ps: Option<String>,
    pub rds_pi: Option<u32>,
}

/// Cached backend state for the egui frame loop.
/// Updated each frame by draining broadcast receivers.
pub struct UiBridge {
    heartbeat_rx: broadcast::Receiver<Arc<Value>>,
    spectrum_rx: broadcast::Receiver<Arc<Value>>,
    protocol_rx: broadcast::Receiver<Arc<Value>>,

    /// Latest heartbeat payload (1 Hz)
    pub heartbeat: Option<Arc<Value>>,
    /// Parsed spectrum data per band
    pub spectrum: HashMap<String, SpectrumFrame>,
    /// Parsed protocol events (newest first)
    pub protocol_log: VecDeque<ProtocolEvent>,
    /// Max protocol log entries to keep
    protocol_log_max: usize,
}

impl UiBridge {
    pub fn new(
        heartbeat_rx: broadcast::Receiver<Arc<Value>>,
        spectrum_rx: broadcast::Receiver<Arc<Value>>,
        protocol_rx: broadcast::Receiver<Arc<Value>>,
    ) -> Self {
        Self {
            heartbeat_rx,
            spectrum_rx,
            protocol_rx,
            heartbeat: None,
            spectrum: HashMap::new(),
            protocol_log: VecDeque::new(),
            protocol_log_max: 200,
        }
    }

    /// Drain all broadcast channels. Call at the start of each egui frame.
    pub fn poll(&mut self) {
        // Heartbeat — keep only latest
        while let Ok(hb) = self.heartbeat_rx.try_recv() {
            self.heartbeat = Some(hb);
        }

        // Spectrum — parse into typed structs, keep latest per band
        while let Ok(sp) = self.spectrum_rx.try_recv() {
            if let Some(frame) = Self::parse_spectrum(&sp) {
                self.spectrum.insert(frame.band.clone(), frame);
            }
        }

        // Protocol — parse and prepend (newest first)
        // Drop cqpsk_status (debug-only) to reduce UI load
        while let Ok(msg) = self.protocol_rx.try_recv() {
            if msg.get("type").and_then(|t| t.as_str()) == Some("cqpsk_status") {
                continue;
            }
            if let Some(evt) = Self::parse_protocol(&msg) {
                self.protocol_log.push_front(evt);
            }
        }
        while self.protocol_log.len() > self.protocol_log_max {
            self.protocol_log.pop_back();
        }
    }

    fn parse_spectrum(v: &Value) -> Option<SpectrumFrame> {
        let band = v.get("band")?.as_str()?.to_string();
        let freqs_arr = v.get("freqs")?.as_array()?;
        let powers_arr = v.get("powers")?.as_array()?;

        let freqs: Vec<f64> = freqs_arr.iter().filter_map(|x| x.as_f64()).collect();
        let powers: Vec<f64> = powers_arr.iter().filter_map(|x| x.as_f64()).collect();

        if freqs.len() < 2 || freqs.len() != powers.len() {
            return None;
        }

        let noise_floor = v
            .get("noise_floor")
            .and_then(|n| n.as_f64())
            .unwrap_or(-100.0);

        let signals = v
            .get("signals")
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        Some(DetectedSignal {
                            freq_mhz: s.get("freq")?.as_f64()?,
                            power_db: s.get("power")?.as_f64().unwrap_or(0.0),
                            classification: s
                                .get("cls")
                                .and_then(|c| c.as_str())
                                .unwrap_or("UNK")
                                .to_string(),
                            name: s
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string(),
                            mode: s
                                .get("mode")
                                .and_then(|m| m.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(SpectrumFrame {
            band,
            freqs,
            powers,
            noise_floor,
            signals,
        })
    }

    fn parse_protocol(v: &Value) -> Option<ProtocolEvent> {
        let event_type = v.get("type")?.as_str()?.to_string();

        Some(ProtocolEvent {
            event_type: event_type.clone(),
            timestamp: v.get("timestamp").and_then(|t| t.as_f64()).unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0)
            }),
            raw: Arc::new(v.clone()),
            nac: v.get("nac").and_then(|n| n.as_u64()).map(|n| n as u32),
            talkgroup: v.get("talkgroup").and_then(|t| t.as_u64()).map(|t| t as u32),
            source_unit: v
                .get("source_unit")
                .and_then(|s| s.as_u64())
                .map(|s| s as u32),
            encrypted: v.get("encrypted").and_then(|e| e.as_bool()),
            opcode: match event_type.as_str() {
                "tsbk" => v.get("opcode").and_then(|o| o.as_str()).map(|s| s.to_string()),
                _ => None,
            },
            rds_ps: match event_type.as_str() {
                "rds" => v.get("ps").and_then(|p| p.as_str()).map(|s| s.to_string()),
                _ => None,
            },
            rds_pi: match event_type.as_str() {
                "rds" => v.get("pi").and_then(|p| p.as_u64()).map(|p| p as u32),
                _ => None,
            },
        })
    }

    /// Extract a string field from the heartbeat
    pub fn hb_str(&self, key: &str) -> &str {
        self.heartbeat
            .as_ref()
            .and_then(|hb| hb.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("--")
    }

    /// Extract a bool field from the heartbeat
    pub fn hb_bool(&self, key: &str) -> bool {
        self.heartbeat
            .as_ref()
            .and_then(|hb| hb.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Extract a f64 field from the heartbeat
    pub fn hb_f64(&self, key: &str) -> f64 {
        self.heartbeat
            .as_ref()
            .and_then(|hb| hb.get(key))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    }

    /// Extract a u64 field from the heartbeat
    #[allow(dead_code)]
    pub fn hb_u64(&self, key: &str) -> u64 {
        self.heartbeat
            .as_ref()
            .and_then(|hb| hb.get(key))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    /// Extract a nested JSON value from the heartbeat
    pub fn hb_nested(&self, key: &str, subkey: &str) -> Option<&Value> {
        self.heartbeat
            .as_ref()
            .and_then(|hb| hb.get(key))
            .and_then(|v| v.get(subkey))
    }

    /// Get list of active bands from heartbeat
    pub fn active_bands(&self) -> Vec<String> {
        self.heartbeat
            .as_ref()
            .and_then(|hb| hb.get("bands"))
            .and_then(|b| b.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}
