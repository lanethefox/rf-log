use std::collections::HashMap;

use crate::event::{now_ns, EventSource, LogRecord, Severity};

// ── Transmission Session ────────────────────────────────────

/// A transmission session correlates related events into a logical "trace":
/// TSBK grant → P25 voice frames → TLC → RF fingerprint → recording.
///
/// Each session gets a unique `trace_id`; sub-events get sequential `span_id`s.
#[derive(Debug, Clone)]
pub struct TransmissionSession {
    /// Unique session ID (used as trace_id on all correlated events).
    pub trace_id: u64,

    /// Nanosecond timestamp when the session started (first event).
    pub start_ns: u64,

    /// Nanosecond timestamp of the most recent event.
    pub last_event_ns: u64,

    /// Next span_id to assign.
    pub next_span: u64,

    /// Talkgroup (if known).
    pub talkgroup: Option<u32>,

    /// Source unit / radio ID (if known).
    pub source_unit: Option<u32>,

    /// NAC (if known).
    pub nac: Option<u32>,

    /// Voice frequency in MHz.
    pub freq_mhz: Option<f64>,

    /// Whether encrypted traffic was detected.
    pub encrypted: bool,

    /// Total events in this session.
    pub event_count: u32,

    /// The event_log ID of the initial grant event (if any).
    pub grant_event_id: Option<u64>,

    /// Associated recording ID (if any, linked when recording finalizes).
    pub recording_id: Option<i64>,

    /// Associated fingerprint ID (if any, linked when fingerprint finalizes).
    pub fingerprint_id: Option<i64>,

    /// Operation context.
    pub operation_id: Option<i64>,

    /// Site session context.
    pub site_session_id: Option<i64>,
}

impl TransmissionSession {
    fn new(trace_id: u64, now: u64) -> Self {
        Self {
            trace_id,
            start_ns: now,
            last_event_ns: now,
            next_span: 1,
            talkgroup: None,
            source_unit: None,
            nac: None,
            freq_mhz: None,
            encrypted: false,
            event_count: 0,
            grant_event_id: None,
            recording_id: None,
            fingerprint_id: None,
            operation_id: None,
            site_session_id: None,
        }
    }

    /// Assign the next span_id and advance the counter.
    pub fn next_span_id(&mut self) -> u64 {
        let span = self.next_span;
        self.next_span += 1;
        self.event_count += 1;
        span
    }

    /// Duration in seconds.
    pub fn duration_sec(&self) -> f64 {
        (self.last_event_ns.saturating_sub(self.start_ns)) as f64 / 1_000_000_000.0
    }
}

// ── Session Key ─────────────────────────────────────────────

/// Key for identifying an active session. Sessions are keyed by
/// (talkgroup, freq) so that concurrent transmissions on different
/// frequencies/TGs are tracked independently.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SessionKey {
    talkgroup: Option<u32>,
    freq_mhz_x1000: Option<i64>, // freq * 1000 as integer for hashing
}

impl SessionKey {
    fn new(talkgroup: Option<u32>, freq_mhz: Option<f64>) -> Self {
        Self {
            talkgroup,
            freq_mhz_x1000: freq_mhz.map(|f| (f * 1000.0) as i64),
        }
    }
}

// ── Session Correlator ──────────────────────────────────────

/// Manages active transmission sessions, correlating events into traces.
///
/// Events are fed in; the correlator assigns `trace_id` and `span_id`,
/// and finalizes sessions after a configurable gap timeout.
pub struct SessionCorrelator {
    /// Active sessions, keyed by (talkgroup, freq).
    active: HashMap<SessionKey, TransmissionSession>,

    /// Gap timeout: if no event arrives for this duration, the session is finalized.
    gap_timeout_ns: u64,

    /// Next trace_id to assign (monotonically increasing).
    next_trace_id: u64,

    /// Finalized sessions waiting to be persisted.
    finalized: Vec<TransmissionSession>,
}

impl SessionCorrelator {
    /// Create a new correlator with the given gap timeout in seconds.
    pub fn new(gap_timeout_sec: u64) -> Self {
        // Seed trace_id from current time to avoid collisions across restarts
        let seed = (now_ns() / 1_000_000) & 0x0000_FFFF_FFFF_FFFF;
        Self {
            active: HashMap::new(),
            gap_timeout_ns: gap_timeout_sec * 1_000_000_000,
            next_trace_id: seed,
            finalized: Vec::new(),
        }
    }

    /// Feed an event into the correlator. If it belongs to an active session,
    /// assigns trace_id and span_id. Returns true if the event was correlated.
    pub fn correlate(&mut self, event: &mut LogRecord) -> bool {
        // Only correlate protocol events with a talkgroup or frequency
        if event.talkgroup.is_none() && event.freq_mhz.is_none() {
            return false;
        }

        let key = SessionKey::new(event.talkgroup, event.freq_mhz);
        let now = event.timestamp_ns;

        // Check if this is a grant (opens a new session)
        let is_grant = event.event_type == "protocol.p25.grant";
        let is_terminator = event.event_type == "protocol.p25.voice"
            && event.attributes.get("duid")
                .and_then(|v| v.as_str())
                .is_some_and(|d| d == "TLC" || d == "SimpleTerm");

        if is_grant {
            // Close any existing session for this key
            if let Some(old) = self.active.remove(&key) {
                self.finalized.push(old);
            }
            // Open new session
            let trace_id = self.next_trace_id;
            self.next_trace_id += 1;
            let mut session = TransmissionSession::new(trace_id, now);
            session.talkgroup = event.talkgroup;
            session.source_unit = event.source_unit;
            session.nac = event.nac;
            session.freq_mhz = event.freq_mhz;
            session.encrypted = event.encrypted.unwrap_or(false);
            session.operation_id = event.operation_id;
            session.site_session_id = event.site_session_id;
            session.grant_event_id = if event.id > 0 { Some(event.id) } else { None };

            let span_id = session.next_span_id();
            event.trace_id = Some(trace_id);
            event.span_id = Some(span_id);

            self.active.insert(key, session);
            return true;
        }

        // Try to match to an existing session
        if let Some(session) = self.active.get_mut(&key) {
            session.last_event_ns = now;
            if event.encrypted.unwrap_or(false) {
                session.encrypted = true;
            }
            if session.source_unit.is_none() {
                session.source_unit = event.source_unit;
            }

            let span_id = session.next_span_id();
            event.trace_id = Some(session.trace_id);
            event.span_id = Some(span_id);

            // If this is a terminator, finalize the session
            if is_terminator {
                if let Some(session) = self.active.remove(&key) {
                    self.finalized.push(session);
                }
            }

            return true;
        }

        false
    }

    /// Check for timed-out sessions and move them to finalized.
    /// Call this periodically (e.g., every second).
    pub fn sweep(&mut self) {
        let now = now_ns();
        let timeout = self.gap_timeout_ns;

        let expired_keys: Vec<SessionKey> = self.active
            .iter()
            .filter(|(_, s)| now.saturating_sub(s.last_event_ns) > timeout)
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired_keys {
            if let Some(session) = self.active.remove(&key) {
                self.finalized.push(session);
            }
        }
    }

    /// Drain finalized sessions (caller persists them to DB).
    pub fn drain_finalized(&mut self) -> Vec<TransmissionSession> {
        std::mem::take(&mut self.finalized)
    }

    /// Number of currently active sessions.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Link a recording to the active session for the given talkgroup/freq.
    pub fn link_recording(&mut self, talkgroup: Option<u32>, freq_mhz: Option<f64>, recording_id: i64) {
        let key = SessionKey::new(talkgroup, freq_mhz);
        if let Some(session) = self.active.get_mut(&key) {
            session.recording_id = Some(recording_id);
        }
    }

    /// Link a fingerprint to the active session for the given talkgroup/freq.
    pub fn link_fingerprint(&mut self, talkgroup: Option<u32>, freq_mhz: Option<f64>, fingerprint_id: i64) {
        let key = SessionKey::new(talkgroup, freq_mhz);
        if let Some(session) = self.active.get_mut(&key) {
            session.fingerprint_id = Some(fingerprint_id);
        }
    }

    /// Produce a finalization LogRecord for a completed session.
    pub fn session_to_event(session: &TransmissionSession) -> LogRecord {
        let duration = session.duration_sec();
        let body = format!(
            "Session {} — TG:{} UID:{} {:.4}MHz {}events {:.1}s{}",
            session.trace_id,
            session.talkgroup.map_or("?".to_string(), |t| t.to_string()),
            session.source_unit.map_or("?".to_string(), |u| u.to_string()),
            session.freq_mhz.unwrap_or(0.0),
            session.event_count,
            duration,
            if session.encrypted { " ENC" } else { "" },
        );

        let mut record = LogRecord::new(
            EventSource::Sigex,
            Severity::Info,
            "sigex.traffic.session",
            body,
        );

        record.trace_id = Some(session.trace_id);
        record.talkgroup = session.talkgroup;
        record.source_unit = session.source_unit;
        record.nac = session.nac;
        record.freq_mhz = session.freq_mhz;
        record.encrypted = Some(session.encrypted);
        record.operation_id = session.operation_id;
        record.site_session_id = session.site_session_id;

        record = record
            .with_attr("duration_sec".to_string(), crate::event::AttributeValue::Float(duration))
            .with_attr("event_count".to_string(), crate::event::AttributeValue::Int(session.event_count as i64));

        if let Some(rec_id) = session.recording_id {
            record = record.with_attr("recording_id".to_string(), crate::event::AttributeValue::Int(rec_id));
        }
        if let Some(fp_id) = session.fingerprint_id {
            record = record.with_attr("fingerprint_id".to_string(), crate::event::AttributeValue::Int(fp_id));
        }

        record
    }
}
