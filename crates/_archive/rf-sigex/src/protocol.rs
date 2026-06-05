//! Tier 2 — Protocol metadata extraction and radio ID tracking.
//!
//! Processes P25/DMR protocol metadata to track UID→TGID associations,
//! detect new radio IDs, and catalog radio units across the system.

use rf_db::Db;
use std::collections::HashSet;

/// A protocol metadata event from P25 or DMR decoder.
#[derive(Debug, Clone)]
pub struct ProtocolEvent {
    pub protocol: String,   // "P25" or "DMR"
    pub tgid: Option<u32>,
    pub source_unit: Option<u32>,
    pub nac: Option<u16>,
    pub system: String,
    pub freq_mhz: Option<f64>,
}

/// Tracks radio ID sightings and detects new UIDs.
pub struct ProtocolTracker {
    /// Set of UIDs already seen (to detect new arrivals).
    known_uids: HashSet<String>,
    /// Active operation ID for data scoping.
    operation_id: Option<i64>,
}

fn uid_key(system: &str, uid: u32) -> String {
    format!("{}:{}", system, uid)
}

impl ProtocolTracker {
    pub fn new() -> Self {
        Self {
            known_uids: HashSet::new(),
            operation_id: None,
        }
    }

    pub fn set_operation_id(&mut self, id: Option<i64>) {
        self.operation_id = id;
    }

    /// Process a protocol metadata event. Returns a description if a new UID was detected.
    pub fn feed(&mut self, ev: &ProtocolEvent, db: &Db) -> Option<String> {
        let uid = match ev.source_unit {
            Some(u) if u > 0 => u,
            _ => return None,
        };

        let tgid = ev.tgid.map(|t| t as i32);

        // Record the sighting
        if let Err(e) = db.upsert_radio_id_sighting(
            uid as i32, tgid, Some(&ev.system), ev.freq_mhz,
        ) {
            tracing::warn!("Failed to upsert radio ID sighting: {}", e);
        }

        // Check if this is a new UID
        let key = uid_key(&ev.system, uid);
        if self.known_uids.insert(key) {
            // New UID detected
            let summary = format!(
                "New {} UID {} on TG {} ({})",
                ev.protocol,
                uid,
                ev.tgid.map(|t| t.to_string()).unwrap_or("?".into()),
                ev.system,
            );

            if let Err(e) = db.insert_sigex_event(
                "protocol", "new_uid", "notice",
                &summary, None, Some(&ev.system),
                tgid, Some(uid as i32), ev.freq_mhz, None, self.operation_id,
            ) {
                tracing::warn!("Failed to insert SIGEX event: {}", e);
            }

            Some(summary)
        } else {
            None
        }
    }

    /// Number of unique UIDs tracked.
    pub fn uid_count(&self) -> usize {
        self.known_uids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_uid_detection() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = ProtocolTracker::new();

        let ev = ProtocolEvent {
            protocol: "P25".into(),
            tgid: Some(283),
            source_unit: Some(12345),
            nac: Some(0x3C0),
            system: "Portland P25".into(),
            freq_mhz: Some(770.250),
        };

        // First time — new UID
        let result = tracker.feed(&ev, &db);
        assert!(result.is_some());
        assert!(result.unwrap().contains("New P25 UID 12345"));

        // Second time — known UID
        let result = tracker.feed(&ev, &db);
        assert!(result.is_none());

        // Different UID — new
        let ev2 = ProtocolEvent { source_unit: Some(67890), ..ev.clone() };
        let result = tracker.feed(&ev2, &db);
        assert!(result.is_some());

        assert_eq!(tracker.uid_count(), 2);

        // Verify DB sightings
        let sightings = db.list_radio_id_sightings(100).unwrap();
        assert_eq!(sightings.len(), 2);
    }

    #[test]
    fn zero_uid_ignored() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = ProtocolTracker::new();

        let ev = ProtocolEvent {
            protocol: "P25".into(),
            tgid: Some(283),
            source_unit: Some(0),
            nac: None,
            system: "Portland P25".into(),
            freq_mhz: None,
        };

        assert!(tracker.feed(&ev, &db).is_none());
        assert_eq!(tracker.uid_count(), 0);
    }

    #[test]
    fn no_source_unit_ignored() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = ProtocolTracker::new();

        let ev = ProtocolEvent {
            protocol: "DMR".into(),
            tgid: Some(1),
            source_unit: None,
            nac: None,
            system: "Local DMR".into(),
            freq_mhz: None,
        };

        assert!(tracker.feed(&ev, &db).is_none());
    }
}
