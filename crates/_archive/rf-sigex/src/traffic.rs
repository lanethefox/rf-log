//! Tier 1 — Traffic session reconstruction from signal detections.
//!
//! Groups sequential signal detections on the same frequency into
//! transmission sessions. A gap of >SESSION_GAP_SEC between detections
//! marks a session boundary.

use rf_db::Db;
use std::collections::HashMap;

/// Gap (in seconds) between detections to consider a session ended.
const SESSION_GAP_SEC: f64 = 2.0;

/// A signal detection event fed into the session tracker.
#[derive(Debug, Clone)]
pub struct SignalDetection {
    pub freq_mhz: f64,
    pub power: f64,
    pub channel_id: Option<i64>,
    pub encrypted: bool,
    pub modulation: Option<String>,
    /// ISO 8601 timestamp string
    pub timestamp: String,
    /// Unix epoch seconds for gap computation
    pub epoch_secs: f64,
}

/// An active (open) session being tracked.
struct ActiveSession {
    freq_mhz: f64,
    channel_id: Option<i64>,
    start_time: String,
    start_epoch: f64,
    last_epoch: f64,
    hit_count: i32,
    power_sum: f64,
    encrypted: bool,
    modulation: Option<String>,
}

/// Tracks active sessions and finalizes them to the database when they expire.
pub struct SessionTracker {
    /// Active sessions keyed by rounded frequency (3 decimal places).
    active: HashMap<String, ActiveSession>,
    /// Active operation ID for data scoping.
    operation_id: Option<i64>,
}

fn freq_key(freq: f64) -> String {
    format!("{:.3}", freq)
}

impl SessionTracker {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
            operation_id: None,
        }
    }

    pub fn set_operation_id(&mut self, id: Option<i64>) {
        self.operation_id = id;
    }

    /// Process a new signal detection. Returns a SIGEX event summary if a
    /// session was finalized (closed) as a result.
    pub fn feed(&mut self, det: &SignalDetection, db: &Db) -> Option<SessionEvent> {
        let key = freq_key(det.freq_mhz);

        // Check if there's an active session on this frequency
        if let Some(session) = self.active.get_mut(&key) {
            let gap = det.epoch_secs - session.last_epoch;
            if gap <= SESSION_GAP_SEC {
                // Continue the session
                session.last_epoch = det.epoch_secs;
                session.hit_count += 1;
                session.power_sum += det.power;
                return None;
            } else {
                // Gap exceeded — finalize the old session, start a new one
                let event = self.finalize_session(&key, &det.timestamp, db);
                self.start_session(&key, det);
                return event;
            }
        }

        // No active session — start a new one
        self.start_session(&key, det);
        None
    }

    /// Flush all active sessions (e.g. on shutdown or periodic cleanup).
    /// Returns the number of sessions finalized.
    pub fn flush_all(&mut self, timestamp: &str, db: &Db) -> usize {
        let keys: Vec<String> = self.active.keys().cloned().collect();
        let mut count = 0;
        for key in keys {
            self.finalize_session(&key, timestamp, db);
            count += 1;
        }
        self.active.clear();
        count
    }

    /// Flush sessions that have been idle longer than the gap threshold.
    /// Call periodically (e.g. every second from the heartbeat).
    pub fn flush_expired(&mut self, current_epoch: f64, timestamp: &str, db: &Db) -> Vec<SessionEvent> {
        let expired_keys: Vec<String> = self.active.iter()
            .filter(|(_, s)| current_epoch - s.last_epoch > SESSION_GAP_SEC)
            .map(|(k, _)| k.clone())
            .collect();

        let mut events = Vec::new();
        for key in expired_keys {
            if let Some(ev) = self.finalize_session(&key, timestamp, db) {
                events.push(ev);
            }
        }
        events
    }

    /// Number of currently active sessions.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    fn start_session(&mut self, key: &str, det: &SignalDetection) {
        self.active.insert(key.to_string(), ActiveSession {
            freq_mhz: det.freq_mhz,
            channel_id: det.channel_id,
            start_time: det.timestamp.clone(),
            start_epoch: det.epoch_secs,
            last_epoch: det.epoch_secs,
            hit_count: 1,
            power_sum: det.power,
            encrypted: det.encrypted,
            modulation: det.modulation.clone(),
        });
    }

    fn finalize_session(&mut self, key: &str, end_timestamp: &str, db: &Db) -> Option<SessionEvent> {
        let session = self.active.remove(key)?;

        // Only record sessions with at least 2 hits (filters noise spikes)
        if session.hit_count < 2 {
            return None;
        }

        let avg_signal = session.power_sum / session.hit_count as f64;

        // Compute duration from actual epoch timestamps
        let duration_sec = (session.last_epoch - session.start_epoch).max(0.0);

        // Update channel total_seconds if we have a channel_id
        if let Some(ch_id) = session.channel_id {
            let _ = db.increment_channel_seconds(ch_id, duration_sec);
        }

        if let Err(e) = db.insert_traffic_session(
            session.freq_mhz,
            session.channel_id,
            &session.start_time,
            Some(end_timestamp),
            Some(duration_sec),
            session.hit_count,
            Some(avg_signal),
            session.encrypted,
            session.modulation.as_deref(),
            self.operation_id,
        ) {
            tracing::warn!("Failed to insert traffic session: {}", e);
        }

        // Also log as a SIGEX event
        let summary = format!(
            "Session on {:.3} MHz — {} hits, {:.1}s, avg {:.0} dB",
            session.freq_mhz, session.hit_count, duration_sec, avg_signal
        );
        if let Err(e) = db.insert_sigex_event(
            "traffic",
            "session_end",
            "info",
            &summary,
            None,
            None, // system
            None, // tgid
            None, // uid
            Some(session.freq_mhz),
            session.channel_id,
            self.operation_id,
        ) {
            tracing::warn!("Failed to insert SIGEX event: {}", e);
        }

        Some(SessionEvent {
            freq_mhz: session.freq_mhz,
            channel_id: session.channel_id,
            duration_sec,
            hit_count: session.hit_count,
            avg_signal,
            encrypted: session.encrypted,
        })
    }
}

/// Returned when a session is finalized.
#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub freq_mhz: f64,
    pub channel_id: Option<i64>,
    pub duration_sec: f64,
    pub hit_count: i32,
    pub avg_signal: f64,
    pub encrypted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_reconstruction() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = SessionTracker::new();

        // Three detections within 2s gap
        let det = |epoch: f64| SignalDetection {
            freq_mhz: 155.010,
            power: -60.0,
            channel_id: None,
            encrypted: false,
            modulation: Some("NFM".into()),
            timestamp: format!("2026-02-15T12:00:{:02}", epoch as i32),
            epoch_secs: epoch,
        };

        assert!(tracker.feed(&det(0.0), &db).is_none());
        assert!(tracker.feed(&det(1.0), &db).is_none());
        assert!(tracker.feed(&det(2.0), &db).is_none());

        // Gap of 3s triggers session end on next detection
        let ev = tracker.feed(&det(5.0), &db);
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.hit_count, 3);
        assert!((ev.duration_sec - 2.0).abs() < 0.01);

        // Verify DB has the session
        let sessions = db.list_traffic_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].hit_count, 3);
    }

    #[test]
    fn flush_expired() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = SessionTracker::new();

        let det = SignalDetection {
            freq_mhz: 462.5625,
            power: -55.0,
            channel_id: None,
            encrypted: false,
            modulation: Some("NFM".into()),
            timestamp: "2026-02-15T12:00:00".into(),
            epoch_secs: 0.0,
        };
        tracker.feed(&det, &db);
        let det2 = SignalDetection {
            epoch_secs: 1.0,
            timestamp: "2026-02-15T12:00:01".into(),
            ..det.clone()
        };
        tracker.feed(&det2, &db);

        assert_eq!(tracker.active_count(), 1);

        // Flush at epoch 5.0 — 4s gap, should expire
        let events = tracker.flush_expired(5.0, "2026-02-15T12:00:05", &db);
        assert_eq!(events.len(), 1);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn single_hit_not_recorded() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = SessionTracker::new();

        let det = SignalDetection {
            freq_mhz: 155.010,
            power: -70.0,
            channel_id: None,
            encrypted: false,
            modulation: None,
            timestamp: "2026-02-15T12:00:00".into(),
            epoch_secs: 0.0,
        };
        tracker.feed(&det, &db);

        // Single hit session should not produce a DB record
        let events = tracker.flush_expired(5.0, "2026-02-15T12:00:05", &db);
        assert!(events.is_empty());

        let sessions = db.list_traffic_sessions(10).unwrap();
        assert!(sessions.is_empty());
    }
}
