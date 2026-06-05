//! Tier 1+2 — Encryption posture tracking and key rotation detection.
//!
//! Consumes P25/DMR protocol metadata to build per-TGID encryption posture
//! maps. Detects key rotation events and encryption status changes.

use rf_db::Db;
use std::collections::HashMap;

/// Known algorithm IDs (P25 ALGID).
pub const ALGID_UNENCRYPTED: i32 = 0x80;
pub const ALGID_DES_OFB: i32 = 0x81;
pub const ALGID_AES_256: i32 = 0x84;
pub const ALGID_ADP: i32 = 0xAA;

/// Map algorithm name strings (from rf-p25) to numeric IDs.
fn algorithm_name_to_id(name: &str) -> i32 {
    match name {
        "Unencrypted" => ALGID_UNENCRYPTED,
        "DES-OFB" => ALGID_DES_OFB,
        "3DES" => 0x83,
        "AES-256" => ALGID_AES_256,
        "Accordion" | "ADP" => ALGID_ADP,
        s if s.starts_with("Unknown(0x") => {
            // Parse "Unknown(0xNN)" format
            s.trim_start_matches("Unknown(0x")
                .trim_end_matches(')')
                .parse::<i32>()
                .ok()
                .map(|v| v)
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Metadata event from P25 or DMR decoder.
#[derive(Debug, Clone)]
pub struct CryptoEvent {
    pub tgid: u32,
    pub source_unit: Option<u32>,
    pub encrypted: bool,
    pub algorithm: Option<String>,
    pub key_id: Option<u16>,
    pub system: String,
    pub freq_mhz: Option<f64>,
}

/// Tracks the last known crypto state per TGID for rotation detection.
struct TgidCryptoState {
    algorithm_id: i32,
    key_id: Option<i32>,
}

/// Tracks encryption posture and detects key rotations across talkgroups.
pub struct EncryptionTracker {
    /// Last known crypto state per TGID key ("system:tgid").
    state: HashMap<String, TgidCryptoState>,
    /// Active operation ID for data scoping.
    operation_id: Option<i64>,
}

fn tgid_key(system: &str, tgid: u32) -> String {
    format!("{}:{}", system, tgid)
}

impl EncryptionTracker {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
            operation_id: None,
        }
    }

    pub fn set_operation_id(&mut self, id: Option<i64>) {
        self.operation_id = id;
    }

    /// Process a crypto metadata event. Returns a description if a key rotation was detected.
    pub fn feed(&mut self, ev: &CryptoEvent, db: &Db) -> Option<String> {
        let alg_id = ev.algorithm.as_ref()
            .map(|name| algorithm_name_to_id(name))
            .unwrap_or(if ev.encrypted { 0 } else { ALGID_UNENCRYPTED });

        let alg_name = ev.algorithm.as_deref().unwrap_or(
            if ev.encrypted { "Unknown" } else { "Unencrypted" }
        );

        let kid = ev.key_id.map(|k| k as i32);
        let tgid = ev.tgid as i32;

        // Update encryption_keys table
        if let Err(e) = db.upsert_encryption_key(
            tgid, &ev.system, Some(alg_id), Some(alg_name), kid,
        ) {
            tracing::warn!("Failed to upsert encryption key: {}", e);
        }

        // Check for key rotation
        let key = tgid_key(&ev.system, ev.tgid);
        let rotation = if let Some(prev) = self.state.get(&key) {
            let alg_changed = prev.algorithm_id != alg_id;
            let key_changed = prev.key_id != kid;

            if alg_changed || key_changed {
                // Key rotation detected
                if let Err(e) = db.insert_key_rotation(
                    tgid, &ev.system,
                    Some(prev.key_id.unwrap_or(-1)), kid,
                    Some(prev.algorithm_id), Some(alg_id),
                ) {
                    tracing::warn!("Failed to insert key rotation: {}", e);
                }

                let summary = if alg_changed && key_changed {
                    format!(
                        "Key rotation on TG {}: {} key={} -> {} key={}",
                        ev.tgid,
                        algorithm_id_to_short(prev.algorithm_id),
                        prev.key_id.map(|k| k.to_string()).unwrap_or("?".into()),
                        algorithm_id_to_short(alg_id),
                        kid.map(|k| k.to_string()).unwrap_or("?".into()),
                    )
                } else if alg_changed {
                    format!(
                        "Algorithm change on TG {}: {} -> {}",
                        ev.tgid,
                        algorithm_id_to_short(prev.algorithm_id),
                        algorithm_id_to_short(alg_id),
                    )
                } else {
                    format!(
                        "Key rotation on TG {}: key {} -> {}",
                        ev.tgid,
                        prev.key_id.map(|k| k.to_string()).unwrap_or("?".into()),
                        kid.map(|k| k.to_string()).unwrap_or("?".into()),
                    )
                };

                // Log as SIGEX event
                if let Err(e) = db.insert_sigex_event(
                    "crypto", "key_rotation",
                    if alg_changed { "warning" } else { "notice" },
                    &summary,
                    None,
                    Some(&ev.system),
                    Some(tgid),
                    ev.source_unit.map(|u| u as i32),
                    ev.freq_mhz,
                    None,
                    self.operation_id,
                ) {
                    tracing::warn!("Failed to insert SIGEX event: {}", e);
                }

                Some(summary)
            } else {
                None
            }
        } else {
            // First observation for this TGID — not a rotation
            None
        };

        // Update state
        self.state.insert(key, TgidCryptoState {
            algorithm_id: alg_id,
            key_id: kid,
        });

        rotation
    }

    /// Number of TGIDs being tracked.
    pub fn tracked_count(&self) -> usize {
        self.state.len()
    }
}

fn algorithm_id_to_short(id: i32) -> &'static str {
    match id {
        0x80 => "CLEAR",
        0x81 => "DES",
        0x83 => "3DES",
        0x84 => "AES",
        0xAA => "ADP",
        _ => "UNK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rotation_detection() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = EncryptionTracker::new();

        // First observation — no rotation
        let ev1 = CryptoEvent {
            tgid: 283,
            source_unit: Some(12345),
            encrypted: true,
            algorithm: Some("AES-256".into()),
            key_id: Some(1),
            system: "Portland P25".into(),
            freq_mhz: Some(770.250),
        };
        assert!(tracker.feed(&ev1, &db).is_none());

        // Same key — no rotation
        let ev2 = CryptoEvent { key_id: Some(1), ..ev1.clone() };
        assert!(tracker.feed(&ev2, &db).is_none());

        // Key changes — rotation!
        let ev3 = CryptoEvent { key_id: Some(2), ..ev1.clone() };
        let rot = tracker.feed(&ev3, &db);
        assert!(rot.is_some());
        assert!(rot.unwrap().contains("key 1 -> 2"));

        // Verify DB
        let rotations = db.list_key_rotations(10, Some(283)).unwrap();
        assert_eq!(rotations.len(), 1);
        assert_eq!(rotations[0].old_key_id, Some(1));
        assert_eq!(rotations[0].new_key_id, Some(2));

        let keys = db.list_encryption_keys(100, None).unwrap();
        assert!(keys.len() >= 2); // At least the AES key with two different key_ids
    }

    #[test]
    fn clear_traffic_tracked() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = EncryptionTracker::new();

        let ev = CryptoEvent {
            tgid: 42001,
            source_unit: None,
            encrypted: false,
            algorithm: Some("Unencrypted".into()),
            key_id: None,
            system: "Portland P25".into(),
            freq_mhz: Some(155.010),
        };
        assert!(tracker.feed(&ev, &db).is_none());

        let keys = db.list_encryption_keys(100, None).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].algorithm_id, Some(0x80));
    }

    #[test]
    fn algorithm_change_detected() {
        let db = Db::open(":memory:").unwrap();
        let mut tracker = EncryptionTracker::new();

        let ev1 = CryptoEvent {
            tgid: 10100,
            source_unit: Some(99),
            encrypted: true,
            algorithm: Some("DES-OFB".into()),
            key_id: Some(5),
            system: "Portland P25".into(),
            freq_mhz: None,
        };
        tracker.feed(&ev1, &db);

        // Switch to AES
        let ev2 = CryptoEvent {
            algorithm: Some("AES-256".into()),
            key_id: Some(5),
            ..ev1.clone()
        };
        let rot = tracker.feed(&ev2, &db);
        assert!(rot.is_some());
        assert!(rot.unwrap().contains("Algorithm change"));
    }
}
