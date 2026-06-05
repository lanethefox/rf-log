use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Severity ────────────────────────────────────────────────

/// OpenTelemetry-aligned severity levels (1-24 scale, we use representative values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Severity {
    Trace = 1,
    Debug = 5,
    Info = 9,
    Notice = 11,
    Warn = 13,
    Error = 17,
    Fatal = 21,
}

impl Severity {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0..=2 => Severity::Trace,
            3..=6 => Severity::Debug,
            7..=10 => Severity::Info,
            11..=12 => Severity::Notice,
            13..=16 => Severity::Warn,
            17..=20 => Severity::Error,
            _ => Severity::Fatal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Trace => "TRACE",
            Severity::Debug => "DEBUG",
            Severity::Info => "INFO",
            Severity::Notice => "NOTICE",
            Severity::Warn => "WARN",
            Severity::Error => "ERROR",
            Severity::Fatal => "FATAL",
        }
    }
}

// ── Event Source ─────────────────────────────────────────────

/// The origin subsystem that produced this event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EventSource {
    /// FFT/PSD signal detections, spectrum data
    Spectrum = 0,
    /// P25, TSBK, RDS, SAME protocol decode
    Protocol = 1,
    /// SIGEX intelligence: fingerprints, traffic analysis, crypto
    Sigex = 2,
    /// System health: SDR status, GPS, mode changes, recordings
    System = 3,
    /// Audio pipeline events
    Audio = 4,
    /// User-defined custom/derived events
    Custom = 5,
}

impl EventSource {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => EventSource::Spectrum,
            1 => EventSource::Protocol,
            2 => EventSource::Sigex,
            3 => EventSource::System,
            4 => EventSource::Audio,
            5 => EventSource::Custom,
            _ => EventSource::System,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EventSource::Spectrum => "SPECTRUM",
            EventSource::Protocol => "PROTOCOL",
            EventSource::Sigex => "SIGEX",
            EventSource::System => "SYSTEM",
            EventSource::Audio => "AUDIO",
            EventSource::Custom => "CUSTOM",
        }
    }
}

// ── Attribute Value ─────────────────────────────────────────

/// Typed attribute value for structured metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    StringArray(Vec<String>),
    IntArray(Vec<i64>),
}

impl AttributeValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttributeValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            AttributeValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            AttributeValue::Float(f) => Some(*f),
            AttributeValue::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            AttributeValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl std::fmt::Display for AttributeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributeValue::String(s) => write!(f, "{s}"),
            AttributeValue::Int(n) => write!(f, "{n}"),
            AttributeValue::Float(v) => write!(f, "{v}"),
            AttributeValue::Bool(b) => write!(f, "{b}"),
            AttributeValue::StringArray(arr) => write!(f, "{arr:?}"),
            AttributeValue::IntArray(arr) => write!(f, "{arr:?}"),
        }
    }
}

// ── LogRecord ───────────────────────────────────────────────

/// The universal event record. Every event in RF-LOG — raw or derived — is a LogRecord.
///
/// Design follows the OpenTelemetry Log Data Model with RF-specific indexed columns
/// for fast faceted queries without requiring JSON extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    /// Database row ID (0 until persisted).
    pub id: u64,

    /// Nanoseconds since Unix epoch.
    pub timestamp_ns: u64,

    /// 30-second bucket for fast time-range indexing.
    /// Computed as `timestamp_ns / 30_000_000_000 * 30_000_000_000`.
    pub ts_bucket: u64,

    // ── Classification ──

    pub severity: Severity,
    pub source: EventSource,

    /// Hierarchical event type using dot notation.
    /// Examples: "protocol.p25.grant", "sigex.emitter.new", "custom.fedl_burst"
    pub event_type: String,

    /// Human-readable summary of the event.
    pub body: String,

    // ── Indexed columns (fast faceted search) ──

    pub freq_mhz: Option<f64>,
    pub talkgroup: Option<u32>,
    pub source_unit: Option<u32>,
    pub nac: Option<u32>,
    pub encrypted: Option<bool>,
    pub band: Option<String>,
    pub device_key: Option<String>,
    pub classification: Option<String>,

    // ── Correlation ──

    /// Transmission session ID (links grant → voice → recording → fingerprint).
    pub trace_id: Option<u64>,
    /// Sub-event within a transmission session.
    pub span_id: Option<u64>,
    /// Active operation context.
    pub operation_id: Option<i64>,
    /// Active site session (geofence) context.
    pub site_session_id: Option<i64>,

    // ── Location ──

    pub receiver_lat: Option<f64>,
    pub receiver_lon: Option<f64>,

    // ── Extended attributes ──

    /// Arbitrary key-value metadata. Stored as JSON in SQLite,
    /// queryable via `json_extract()`.
    pub attributes: BTreeMap<String, AttributeValue>,
}

impl LogRecord {
    /// Create a new LogRecord with the current timestamp.
    pub fn new(
        source: EventSource,
        severity: Severity,
        event_type: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        let ts = now_ns();
        Self {
            id: 0,
            timestamp_ns: ts,
            ts_bucket: bucket_30s(ts),
            severity,
            source,
            event_type: event_type.into(),
            body: body.into(),
            freq_mhz: None,
            talkgroup: None,
            source_unit: None,
            nac: None,
            encrypted: None,
            band: None,
            device_key: None,
            classification: None,
            trace_id: None,
            span_id: None,
            operation_id: None,
            site_session_id: None,
            receiver_lat: None,
            receiver_lon: None,
            attributes: BTreeMap::new(),
        }
    }

    // ── Builder methods ──

    pub fn with_freq(mut self, freq_mhz: f64) -> Self {
        self.freq_mhz = Some(freq_mhz);
        self
    }

    pub fn with_talkgroup(mut self, tg: u32) -> Self {
        self.talkgroup = Some(tg);
        self
    }

    pub fn with_source_unit(mut self, uid: u32) -> Self {
        self.source_unit = Some(uid);
        self
    }

    pub fn with_nac(mut self, nac: u32) -> Self {
        self.nac = Some(nac);
        self
    }

    pub fn with_encrypted(mut self, enc: bool) -> Self {
        self.encrypted = Some(enc);
        self
    }

    pub fn with_band(mut self, band: impl Into<String>) -> Self {
        self.band = Some(band.into());
        self
    }

    pub fn with_device(mut self, key: impl Into<String>) -> Self {
        self.device_key = Some(key.into());
        self
    }

    pub fn with_classification(mut self, cls: impl Into<String>) -> Self {
        self.classification = Some(cls.into());
        self
    }

    pub fn with_trace(mut self, trace_id: u64) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    pub fn with_span(mut self, span_id: u64) -> Self {
        self.span_id = Some(span_id);
        self
    }

    pub fn with_operation(mut self, op_id: i64) -> Self {
        self.operation_id = Some(op_id);
        self
    }

    pub fn with_site_session(mut self, ss_id: i64) -> Self {
        self.site_session_id = Some(ss_id);
        self
    }

    pub fn with_location(mut self, lat: f64, lon: f64) -> Self {
        self.receiver_lat = Some(lat);
        self.receiver_lon = Some(lon);
        self
    }

    pub fn with_attr(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// Serialize attributes to JSON string for SQLite storage.
    pub fn attributes_json(&self) -> String {
        serde_json::to_string(&self.attributes).unwrap_or_else(|_| "{}".to_string())
    }
}

// ── Time utilities ──────────────────────────────────────────

const BUCKET_NS: u64 = 30_000_000_000; // 30 seconds in nanoseconds

/// Current time as nanoseconds since Unix epoch.
pub fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Compute the 30-second bucket for a given nanosecond timestamp.
pub fn bucket_30s(timestamp_ns: u64) -> u64 {
    (timestamp_ns / BUCKET_NS) * BUCKET_NS
}

/// Convert nanosecond timestamp to seconds (f64).
pub fn ns_to_secs(ns: u64) -> f64 {
    ns as f64 / 1_000_000_000.0
}

/// Convert seconds (f64) to nanosecond timestamp.
pub fn secs_to_ns(secs: f64) -> u64 {
    (secs * 1_000_000_000.0) as u64
}

// ── Event Type Constants ────────────────────────────────────

/// Well-known event type prefixes for the dot-notation taxonomy.
pub mod event_types {
    // Spectrum
    pub const SPECTRUM_DETECT: &str = "spectrum.detect";
    pub const SPECTRUM_LOST: &str = "spectrum.lost";
    pub const SPECTRUM_ANOMALY: &str = "spectrum.anomaly";

    // Protocol — P25
    pub const P25_VOICE: &str = "protocol.p25.voice";
    pub const P25_GRANT: &str = "protocol.p25.grant";
    pub const P25_UPDATE: &str = "protocol.p25.update";
    pub const P25_REGISTER: &str = "protocol.p25.register";
    pub const P25_DEREGISTER: &str = "protocol.p25.deregister";
    pub const P25_AFFILIATION: &str = "protocol.p25.affiliation";
    pub const P25_DENY: &str = "protocol.p25.deny";
    pub const P25_ADJACENT: &str = "protocol.p25.adjacent";
    pub const P25_CC_STATUS: &str = "protocol.p25.cc_status";
    pub const P25_NET_STATUS: &str = "protocol.p25.net_status";
    pub const P25_RFSS_STATUS: &str = "protocol.p25.rfss_status";
    pub const P25_CHAN_PARAMS: &str = "protocol.p25.chan_params";

    // Protocol — RDS
    pub const RDS_UPDATE: &str = "protocol.rds.update";

    // Protocol — SAME
    pub const SAME_ALERT: &str = "protocol.same.alert";

    // SIGEX
    pub const SIGEX_EMITTER_NEW: &str = "sigex.emitter.new";
    pub const SIGEX_EMITTER_RETURN: &str = "sigex.emitter.return";
    pub const SIGEX_UID_MISMATCH: &str = "sigex.uid.mismatch";
    pub const SIGEX_CRYPTO_ROTATION: &str = "sigex.crypto.rotation";
    pub const SIGEX_TRAFFIC_SESSION: &str = "sigex.traffic.session";
    pub const SIGEX_NEW_UID: &str = "sigex.protocol.new_uid";
    pub const SIGEX_NETWORK_EVENT: &str = "sigex.network.event";
    pub const SIGEX_EMERGENCY: &str = "sigex.network.emergency";
    pub const SIGEX_ACCESS_DENY: &str = "sigex.network.deny";
    pub const SIGEX_ANOMALY: &str = "sigex.anomaly.baseline";

    // System
    pub const SYSTEM_SDR_CONNECT: &str = "system.sdr.connect";
    pub const SYSTEM_SDR_DISCONNECT: &str = "system.sdr.disconnect";
    pub const SYSTEM_SDR_ERROR: &str = "system.sdr.error";
    pub const SYSTEM_GPS_FIX: &str = "system.gps.fix";
    pub const SYSTEM_MODE_CHANGE: &str = "system.mode.change";
    pub const SYSTEM_OP_START: &str = "system.operation.start";
    pub const SYSTEM_OP_STOP: &str = "system.operation.stop";
    pub const SYSTEM_REC_START: &str = "system.recording.start";
    pub const SYSTEM_REC_STOP: &str = "system.recording.stop";
    pub const SYSTEM_CLIP_START: &str = "system.clip.start";
    pub const SYSTEM_CLIP_END: &str = "system.clip.end";
    pub const SYSTEM_SITE_ENTER: &str = "system.site.enter";
    pub const SYSTEM_SITE_EXIT: &str = "system.site.exit";
    pub const SYSTEM_ALERT_FIRED: &str = "system.alert.fired";

    /// Check if an event type is a custom/derived type.
    pub fn is_custom(event_type: &str) -> bool {
        event_type.starts_with("custom.")
    }

    /// Get the top-level category of an event type.
    pub fn category(event_type: &str) -> &str {
        event_type.split('.').next().unwrap_or("unknown")
    }

    /// Human-readable display name for a well-known event type.
    /// Returns `None` for unknown/custom types — caller decides fallback.
    pub fn display_name(event_type: &str) -> Option<&'static str> {
        Some(match event_type {
            // Spectrum
            SPECTRUM_DETECT => "Signal Detected",
            SPECTRUM_LOST => "Signal Lost",
            SPECTRUM_ANOMALY => "Spectrum Anomaly",
            // P25
            P25_VOICE => "P25 Voice",
            P25_GRANT => "P25 Voice Grant",
            P25_UPDATE => "P25 Grant Update",
            P25_REGISTER => "P25 Unit Register",
            P25_DEREGISTER => "P25 Unit Deregister",
            P25_AFFILIATION => "P25 Group Affiliation",
            P25_DENY => "P25 Deny Response",
            P25_ADJACENT => "P25 Adjacent Site",
            P25_CC_STATUS => "P25 CC Status",
            P25_NET_STATUS => "P25 Network Status",
            P25_RFSS_STATUS => "P25 RFSS Status",
            P25_CHAN_PARAMS => "P25 Channel Params",
            // RDS / SAME
            RDS_UPDATE => "RDS Metadata",
            SAME_ALERT => "SAME Weather Alert",
            // SIGEX
            SIGEX_EMITTER_NEW => "New Emitter",
            SIGEX_EMITTER_RETURN => "Returning Emitter",
            SIGEX_UID_MISMATCH => "UID Mismatch",
            SIGEX_CRYPTO_ROTATION => "Crypto Key Rotation",
            SIGEX_TRAFFIC_SESSION => "Traffic Session",
            SIGEX_NEW_UID => "New Radio UID",
            SIGEX_NETWORK_EVENT => "Network Event",
            SIGEX_EMERGENCY => "Emergency Grant",
            SIGEX_ACCESS_DENY => "Access Denied",
            SIGEX_ANOMALY => "Baseline Anomaly",
            // System
            SYSTEM_SDR_CONNECT => "SDR Connected",
            SYSTEM_SDR_DISCONNECT => "SDR Disconnected",
            SYSTEM_SDR_ERROR => "SDR Error",
            SYSTEM_GPS_FIX => "GPS Fix",
            SYSTEM_MODE_CHANGE => "Mode Change",
            SYSTEM_OP_START => "Operation Started",
            SYSTEM_OP_STOP => "Operation Stopped",
            SYSTEM_REC_START => "Recording Started",
            SYSTEM_REC_STOP => "Recording Stopped",
            SYSTEM_CLIP_START => "P25 Clip Started",
            SYSTEM_CLIP_END => "P25 Clip Ended",
            SYSTEM_SITE_ENTER => "Site Entered",
            SYSTEM_SITE_EXIT => "Site Exited",
            SYSTEM_ALERT_FIRED => "Alert Fired",
            _ => return None,
        })
    }

    /// Human-readable name with fallback to raw event type.
    pub fn display_name_or_raw(event_type: &str) -> &str {
        display_name(event_type).unwrap_or(event_type)
    }

    /// All known event types paired with their display names, for UI dropdowns.
    pub const ALL_DISPLAY: &[(&str, &str)] = &[
        (SPECTRUM_DETECT, "Signal Detected"),
        (SPECTRUM_LOST, "Signal Lost"),
        (SPECTRUM_ANOMALY, "Spectrum Anomaly"),
        (P25_VOICE, "P25 Voice"),
        (P25_GRANT, "P25 Voice Grant"),
        (P25_UPDATE, "P25 Grant Update"),
        (P25_REGISTER, "P25 Unit Register"),
        (P25_DEREGISTER, "P25 Unit Deregister"),
        (P25_AFFILIATION, "P25 Group Affiliation"),
        (P25_DENY, "P25 Deny Response"),
        (P25_ADJACENT, "P25 Adjacent Site"),
        (P25_CC_STATUS, "P25 CC Status"),
        (P25_NET_STATUS, "P25 Network Status"),
        (P25_RFSS_STATUS, "P25 RFSS Status"),
        (P25_CHAN_PARAMS, "P25 Channel Params"),
        (RDS_UPDATE, "RDS Metadata"),
        (SAME_ALERT, "SAME Weather Alert"),
        (SIGEX_EMITTER_NEW, "New Emitter"),
        (SIGEX_EMITTER_RETURN, "Returning Emitter"),
        (SIGEX_UID_MISMATCH, "UID Mismatch"),
        (SIGEX_CRYPTO_ROTATION, "Crypto Key Rotation"),
        (SIGEX_TRAFFIC_SESSION, "Traffic Session"),
        (SIGEX_NEW_UID, "New Radio UID"),
        (SIGEX_NETWORK_EVENT, "Network Event"),
        (SIGEX_EMERGENCY, "Emergency Grant"),
        (SIGEX_ACCESS_DENY, "Access Denied"),
        (SIGEX_ANOMALY, "Baseline Anomaly"),
        (SYSTEM_SDR_CONNECT, "SDR Connected"),
        (SYSTEM_SDR_DISCONNECT, "SDR Disconnected"),
        (SYSTEM_SDR_ERROR, "SDR Error"),
        (SYSTEM_GPS_FIX, "GPS Fix"),
        (SYSTEM_MODE_CHANGE, "Mode Change"),
        (SYSTEM_OP_START, "Operation Started"),
        (SYSTEM_OP_STOP, "Operation Stopped"),
        (SYSTEM_REC_START, "Recording Started"),
        (SYSTEM_REC_STOP, "Recording Stopped"),
        (SYSTEM_CLIP_START, "P25 Clip Started"),
        (SYSTEM_CLIP_END, "P25 Clip Ended"),
        (SYSTEM_SITE_ENTER, "Site Entered"),
        (SYSTEM_SITE_EXIT, "Site Exited"),
        (SYSTEM_ALERT_FIRED, "Alert Fired"),
    ];
}
