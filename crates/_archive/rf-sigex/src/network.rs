//! Tier 3 — P25 control channel (TSBK) processing and network mapping.
//!
//! Consumes parsed TSBK payloads from rf-p25 and maintains:
//!   - Channel parameter tables for frequency resolution
//!   - Network site topology (WACN, system, RFSS, site)
//!   - Talkgroup dispatch log (voice grants)
//!   - Radio affiliation/registration tracking

use rf_db::Db;
use rf_p25::TsbkData;
use crate::{CryptoEvent, EncryptionTracker};

/// Cached channel parameters for frequency resolution.
#[derive(Clone, Debug)]
struct ChannelParamsEntry {
    base_freq_hz: u32,
    spacing_hz: u32,
}

/// Known system identity from NetworkStatusBroadcast.
#[derive(Clone, Debug)]
struct SystemIdentity {
    wacn: u32,
    system_id: u16,
}

/// Processes parsed TSBK payloads, maintains in-memory state, and persists to DB.
pub struct NetworkTracker {
    /// Channel ID → ChannelParams mapping (up to 16 per site).
    channel_params: [Option<ChannelParamsEntry>; 16],
    /// Known system identity (from NetworkStatusBroadcast).
    system_identity: Option<SystemIdentity>,
    /// Default system name for DB records.
    system_name: String,
    /// Total TSBKs processed.
    tsbk_count: u64,
    /// Total grants processed.
    grants_processed: u64,
    /// Active operation ID for data scoping.
    operation_id: Option<i64>,
}

impl NetworkTracker {
    pub fn new() -> Self {
        Self {
            channel_params: Default::default(),
            system_identity: None,
            system_name: "Portland".into(),
            tsbk_count: 0,
            grants_processed: 0,
            operation_id: None,
        }
    }

    pub fn set_operation_id(&mut self, id: Option<i64>) {
        self.operation_id = id;
    }

    /// Total TSBK packets processed.
    pub fn tsbk_count(&self) -> u64 {
        self.tsbk_count
    }

    /// Total voice grants processed.
    pub fn grants_processed(&self) -> u64 {
        self.grants_processed
    }

    /// Current system identity if known.
    pub fn system_identity(&self) -> Option<(u32, u16)> {
        self.system_identity.as_ref().map(|si| (si.wacn, si.system_id))
    }

    /// Resolve a (channel_id, channel_number) pair to a frequency in MHz.
    pub fn resolve_freq(&self, channel_id: u8, channel_num: u16) -> Option<f64> {
        if (channel_id as usize) >= self.channel_params.len() {
            return None;
        }
        self.channel_params[channel_id as usize].as_ref().map(|p| {
            let freq_hz = p.base_freq_hz as u64 + p.spacing_hz as u64 * channel_num as u64;
            freq_hz as f64 / 1_000_000.0
        })
    }

    /// Process a parsed TSBK payload. Returns an optional description for logging.
    pub fn feed(&mut self, data: &TsbkData, db: &Db) -> Option<String> {
        self.feed_with_enc(data, db, None)
    }

    /// Process a parsed TSBK payload, optionally feeding encryption data to an EncryptionTracker.
    pub fn feed_with_enc(&mut self, data: &TsbkData, db: &Db, enc: Option<&mut EncryptionTracker>) -> Option<String> {
        self.tsbk_count += 1;
        let sys = &self.system_name;

        match data {
            TsbkData::ChannelParamsUpdate { id, base_freq_hz, spacing_hz, .. } => {
                let idx = *id as usize;
                if idx < 16 {
                    self.channel_params[idx] = Some(ChannelParamsEntry {
                        base_freq_hz: *base_freq_hz,
                        spacing_hz: *spacing_hz,
                    });
                    tracing::debug!("Channel params [{}]: base={}Hz spacing={}Hz", id, base_freq_hz, spacing_hz);
                }
                None
            }

            TsbkData::NetworkStatusBroadcast { wacn, system, channel_id, channel_num } => {
                self.system_identity = Some(SystemIdentity {
                    wacn: *wacn,
                    system_id: *system,
                });
                let cc_freq = self.resolve_freq(*channel_id, *channel_num);
                if let Err(e) = db.upsert_network_site(
                    sys, Some(*wacn as i64), Some(*system as i64),
                    None, None, cc_freq, None, None, None, None,
                ) {
                    tracing::warn!("Failed to upsert network site: {}", e);
                }
                let summary = format!(
                    "Network identified: WACN 0x{:05X} System 0x{:03X}",
                    wacn, system
                );
                let _ = db.insert_sigex_event(
                    "network", "site_identified", "info",
                    &summary, None, Some(sys), None, None, cc_freq, None, self.operation_id,
                );
                Some(summary)
            }

            TsbkData::RfssStatusBroadcast { system, rfss, site, channel_id, channel_num, networked } => {
                let cc_freq = self.resolve_freq(*channel_id, *channel_num);
                let wacn = self.system_identity.as_ref().map(|si| si.wacn as i64);
                if let Err(e) = db.upsert_network_site(
                    sys, wacn, Some(*system as i64),
                    Some(*rfss as i64), Some(*site as i64),
                    cc_freq, None, None, None, None,
                ) {
                    tracing::warn!("Failed to upsert RFSS status: {}", e);
                }
                tracing::debug!(
                    "RFSS status: sys=0x{:03X} rfss={} site={} networked={}",
                    system, rfss, site, networked
                );
                None
            }

            TsbkData::AdjacentSite { system, rfss, site, channel_id, channel_num } => {
                let adj_freq = self.resolve_freq(*channel_id, *channel_num);
                let adj_json = serde_json::json!({
                    "system": system, "rfss": rfss, "site": site,
                    "freq_mhz": adj_freq,
                }).to_string();
                // Upsert the adjacent site as a separate entry
                if let Err(e) = db.upsert_network_site(
                    sys, None, Some(*system as i64),
                    Some(*rfss as i64), Some(*site as i64),
                    adj_freq, None, None, None, None,
                ) {
                    tracing::warn!("Failed to upsert adjacent site: {}", e);
                }
                tracing::debug!(
                    "Adjacent site: sys=0x{:03X} rfss={} site={} freq={:?}MHz",
                    system, rfss, site, adj_freq
                );
                let _ = adj_json; // used in upsert above
                None
            }

            TsbkData::AltControlChannel { rfss, site, channels } => {
                let alt_json: Vec<serde_json::Value> = channels.iter().map(|ch| {
                    let freq = self.resolve_freq(ch.channel_id, ch.channel_num);
                    serde_json::json!({ "channel_id": ch.channel_id, "channel_num": ch.channel_num, "freq_mhz": freq })
                }).collect();
                let alt_str = serde_json::to_string(&alt_json).unwrap_or_default();
                // Update the site's alt_control field
                let wacn = self.system_identity.as_ref().map(|si| si.wacn as i64);
                let sys_id = self.system_identity.as_ref().map(|si| si.system_id as i64);
                if let Err(e) = db.upsert_network_site(
                    sys, wacn, sys_id,
                    Some(*rfss as i64), Some(*site as i64),
                    None, Some(&alt_str), None, None, None,
                ) {
                    tracing::warn!("Failed to update alt control channels: {}", e);
                }
                None
            }

            TsbkData::GroupVoiceGrant { talkgroup, src_unit, channel_id, channel_num, emergency, encrypted } => {
                self.grants_processed += 1;
                let voice_freq = self.resolve_freq(*channel_id, *channel_num);
                if let Err(e) = db.insert_channel_grant(
                    sys, *talkgroup as i32, Some(*src_unit as i32), voice_freq, Some("group"), self.operation_id,
                ) {
                    tracing::warn!("Failed to insert channel grant: {}", e);
                }
                if let Err(e) = db.upsert_network_talkgroup(
                    sys, *talkgroup as i32, *encrypted, None,
                ) {
                    tracing::warn!("Failed to upsert network talkgroup: {}", e);
                }

                // Feed encryption tracker so encryption_keys table gets populated
                if let Some(enc_tracker) = enc {
                    let crypto_ev = CryptoEvent {
                        tgid: *talkgroup,
                        source_unit: Some(*src_unit),
                        encrypted: *encrypted,
                        algorithm: None,
                        key_id: None,
                        system: sys.to_string(),
                        freq_mhz: voice_freq,
                    };
                    enc_tracker.feed(&crypto_ev, db);
                }

                if *emergency {
                    let summary = format!(
                        "EMERGENCY grant: TG {} UID {} freq {:?}MHz",
                        talkgroup, src_unit, voice_freq
                    );
                    let _ = db.insert_sigex_event(
                        "network", "emergency_grant", "critical",
                        &summary, None, Some(sys),
                        Some(*talkgroup as i32), Some(*src_unit as i32),
                        voice_freq, None, self.operation_id,
                    );
                    return Some(summary);
                }
                None
            }

            TsbkData::GroupVoiceUpdate { updates } => {
                for upd in updates {
                    self.grants_processed += 1;
                    let voice_freq = self.resolve_freq(upd.channel_id, upd.channel_num);
                    let _ = db.insert_channel_grant(
                        sys, upd.talkgroup as i32, None, voice_freq, Some("group_update"), self.operation_id,
                    );
                    let _ = db.upsert_network_talkgroup(
                        sys, upd.talkgroup as i32, false, None,
                    );
                }
                None
            }

            TsbkData::UnitVoiceGrant { src_unit, dest_unit, channel_id, channel_num } => {
                self.grants_processed += 1;
                let voice_freq = self.resolve_freq(*channel_id, *channel_num);
                if let Err(e) = db.insert_channel_grant(
                    sys, 0, Some(*src_unit as i32), voice_freq, Some("unit"), self.operation_id,
                ) {
                    tracing::warn!("Failed to insert unit grant: {}", e);
                }
                tracing::debug!(
                    "Unit voice grant: src={} dest={} freq={:?}MHz",
                    src_unit, dest_unit, voice_freq
                );
                None
            }

            TsbkData::GroupAffiliation { talkgroup, src_unit } => {
                if let Err(e) = db.insert_radio_affiliation(
                    sys, *src_unit as i32, *talkgroup as i32, "affiliation",
                ) {
                    tracing::warn!("Failed to insert affiliation: {}", e);
                }
                // Also record radio ID sighting
                let _ = db.upsert_radio_id_sighting(
                    *src_unit as i32, Some(*talkgroup as i32), Some(sys), None,
                );
                None
            }

            TsbkData::UnitRegistration { system: sys_id, src_unit } => {
                if let Err(e) = db.insert_radio_affiliation(
                    sys, *src_unit as i32, 0, "registration",
                ) {
                    tracing::warn!("Failed to insert registration: {}", e);
                }
                let _ = db.upsert_radio_id_sighting(
                    *src_unit as i32, None, Some(sys), None,
                );
                let summary = format!("Unit {} registered on system 0x{:03X}", src_unit, sys_id);
                let _ = db.insert_sigex_event(
                    "network", "unit_registration", "info",
                    &summary, None, Some(sys), None, Some(*src_unit as i32), None, None, self.operation_id,
                );
                Some(summary)
            }

            TsbkData::UnitDeregistration { wacn, system: sys_id, src_unit } => {
                if let Err(e) = db.insert_radio_affiliation(
                    sys, *src_unit as i32, 0, "deregistration",
                ) {
                    tracing::warn!("Failed to insert deregistration: {}", e);
                }
                tracing::debug!(
                    "Unit {} deregistered from WACN 0x{:05X} system 0x{:03X}",
                    src_unit, wacn, sys_id
                );
                None
            }

            TsbkData::DenyResponse { src_unit } => {
                let summary = format!("Request denied for UID {}", src_unit);
                let _ = db.insert_sigex_event(
                    "network", "request_denied", "notice",
                    &summary, None, Some(sys), None, Some(*src_unit as i32), None, None, self.operation_id,
                );
                Some(summary)
            }

            TsbkData::Other { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        Db::open(":memory:").unwrap()
    }

    #[test]
    fn test_channel_params_resolution() {
        let mut tracker = NetworkTracker::new();

        // Simulate ChannelParamsUpdate for Portland P25
        // Base: 851.0125 MHz = 851_012_500 Hz, Spacing: 12500 Hz
        let data = TsbkData::ChannelParamsUpdate {
            id: 2,
            bandwidth_hz: 12_500,
            base_freq_hz: 851_012_500,
            spacing_hz: 12_500,
        };
        let db = test_db();
        tracker.feed(&data, &db);

        // Channel num 9 → 851_012_500 + 12_500 * 9 = 851_125_000 Hz = 851.125 MHz
        let freq = tracker.resolve_freq(2, 9);
        assert!(freq.is_some());
        let f = freq.unwrap();
        assert!((f - 851.125).abs() < 0.001, "Expected 851.125 MHz, got {}", f);

        // Unknown channel ID → None
        assert!(tracker.resolve_freq(5, 9).is_none());
    }

    #[test]
    fn test_grant_processing() {
        let db = test_db();
        let mut tracker = NetworkTracker::new();

        // Set up channel params first
        tracker.feed(&TsbkData::ChannelParamsUpdate {
            id: 1,
            bandwidth_hz: 12_500,
            base_freq_hz: 851_000_000,
            spacing_hz: 12_500,
        }, &db);

        // Process a group voice grant
        let grant = TsbkData::GroupVoiceGrant {
            talkgroup: 283,
            src_unit: 12345,
            channel_id: 1,
            channel_num: 20,
            emergency: false,
            encrypted: false,
        };
        let result = tracker.feed(&grant, &db);
        assert!(result.is_none()); // Non-emergency → no summary

        assert_eq!(tracker.grants_processed(), 1);

        // Verify DB insertion
        let grants = db.list_channel_grants(None, None, None, 10).unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].tgid, 283);
        assert_eq!(grants[0].uid, Some(12345));
        // 851_000_000 + 12_500 * 20 = 851_250_000 Hz = 851.25 MHz
        assert!((grants[0].voice_freq.unwrap() - 851.25).abs() < 0.001);

        let tgs = db.list_network_talkgroups(None, 10).unwrap();
        assert_eq!(tgs.len(), 1);
        assert_eq!(tgs[0].tgid, 283);
    }

    #[test]
    fn test_site_identification() {
        let db = test_db();
        let mut tracker = NetworkTracker::new();

        let data = TsbkData::NetworkStatusBroadcast {
            wacn: 0xBEE00,
            system: 0x3CC,
            channel_id: 0,
            channel_num: 0,
        };
        let result = tracker.feed(&data, &db);
        assert!(result.is_some());
        assert!(result.unwrap().contains("WACN 0xBEE00"));

        let identity = tracker.system_identity();
        assert!(identity.is_some());
        let (wacn, sys) = identity.unwrap();
        assert_eq!(wacn, 0xBEE00);
        assert_eq!(sys, 0x3CC);

        let sites = db.list_network_sites(None, 10).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].wacn, Some(0xBEE00));
        assert_eq!(sites[0].system_id, Some(0x3CC));
    }

    #[test]
    fn test_affiliation_tracking() {
        let db = test_db();
        let mut tracker = NetworkTracker::new();

        let data = TsbkData::GroupAffiliation {
            talkgroup: 500,
            src_unit: 67890,
        };
        tracker.feed(&data, &db);

        let affiliations = db.list_radio_affiliations(None, None, 10).unwrap();
        assert_eq!(affiliations.len(), 1);
        assert_eq!(affiliations[0].uid, 67890);
        assert_eq!(affiliations[0].tgid, 500);
        assert_eq!(affiliations[0].event_type, "affiliation");
    }

    #[test]
    fn test_emergency_grant_fires_event() {
        let db = test_db();
        let mut tracker = NetworkTracker::new();

        let grant = TsbkData::GroupVoiceGrant {
            talkgroup: 100,
            src_unit: 999,
            channel_id: 0,
            channel_num: 0,
            emergency: true,
            encrypted: false,
        };
        let result = tracker.feed(&grant, &db);
        assert!(result.is_some());
        assert!(result.unwrap().contains("EMERGENCY"));

        let events = db.list_sigex_events(10, Some("network"), None).unwrap();
        assert!(events.iter().any(|e| e.event_type == "emergency_grant"));
    }
}
