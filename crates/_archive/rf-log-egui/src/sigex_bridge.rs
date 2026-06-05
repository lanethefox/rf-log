//! SIGEX tracker bridge — 3-tier passive intelligence processing.
//!
//! Ported from `src-tauri/src/sigex_bridge.rs` for the egui app, with
//! SIEM pipeline integration so tracker discoveries flow through the
//! unified event model (LogRecords → EventBus → AlertEngine).
//!
//! Tier 1: Traffic session tracking from signal detections
//! Tier 2: Encryption + protocol metadata from P25 events
//! Tier 3: P25 control channel network mapping from TSBK messages

use std::sync::Arc;

use rf_events::pipeline::IngestionPipeline;
use rf_web::AppState;

use crate::event_ingestion;

/// Spawn all 3 SIGEX tiers as background tasks on the given runtime.
pub fn spawn_all(
    rt: &tokio::runtime::Runtime,
    state: AppState,
    pipeline: Arc<IngestionPipeline>,
) {
    spawn_tier1(rt, state.clone(), pipeline.clone());
    spawn_tier2(rt, state.clone(), pipeline.clone());
    spawn_tier3(rt, state, pipeline);
    tracing::info!("SIGEX bridge: all 3 tiers spawned");
}

/// Tier 1 — passive traffic analysis on signal detections.
///
/// Subscribes to spectrum broadcast, feeds signal detections into
/// `SessionTracker`, which groups them into transmission sessions.
/// When a session closes, emits a `sigex.traffic.session` LogRecord.
fn spawn_tier1(
    rt: &tokio::runtime::Runtime,
    state: AppState,
    pipeline: Arc<IngestionPipeline>,
) {
    rt.spawn(async move {
        let mut rx = state.subscribe_spectrum();
        let mut tracker = rf_sigex::SessionTracker::new();
        let mut last_flush = std::time::Instant::now();

        tracing::info!("SIGEX Tier 1 (SessionTracker) started");

        loop {
            if state.is_shutdown() { break; }
            match rx.recv().await {
                Ok(msg) => {
                    if state.is_shutdown() { break; }

                    // Sync operation context
                    tracker.set_operation_id(state.config().active_operation_id);

                    let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if msg_type == "signal" {
                        let freq = msg.get("freq").and_then(|f| f.as_f64()).unwrap_or(0.0);
                        let power = msg.get("power").and_then(|p| p.as_f64()).unwrap_or(-100.0);
                        let channel_id = msg.get("channel_id").and_then(|c| c.as_i64());
                        let encrypted = msg.get("encryption_seen")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                        let epoch = epoch_now();
                        let timestamp = chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string();

                        let det = rf_sigex::SignalDetection {
                            freq_mhz: freq,
                            power,
                            channel_id,
                            encrypted,
                            modulation: msg.get("mode")
                                .and_then(|m| m.as_str())
                                .map(String::from),
                            timestamp,
                            epoch_secs: epoch,
                        };

                        if let Some(session_evt) = tracker.feed(&det, state.db()) {
                            // Session closed — emit SIEM event
                            let body = format!(
                                "Session closed: {:.4} MHz, {:.0}s, {} hits, avg {:.0} dBm{}",
                                session_evt.freq_mhz,
                                session_evt.duration_sec,
                                session_evt.hit_count,
                                session_evt.avg_signal,
                                if session_evt.encrypted { " [ENC]" } else { "" },
                            );
                            let rec = event_ingestion::sigex_to_log_record(
                                "traffic_session",
                                &body,
                                Some(session_evt.freq_mhz),
                                None,
                            );
                            pipeline.ingest(rec);
                        }
                    }

                    // Flush expired sessions every 2 seconds
                    if last_flush.elapsed() >= std::time::Duration::from_secs(2) {
                        let epoch = epoch_now();
                        let ts = chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string();
                        let expired = tracker.flush_expired(epoch, &ts, state.db());
                        for session_evt in expired {
                            let body = format!(
                                "Session expired: {:.4} MHz, {:.0}s, {} hits, avg {:.0} dBm{}",
                                session_evt.freq_mhz,
                                session_evt.duration_sec,
                                session_evt.hit_count,
                                session_evt.avg_signal,
                                if session_evt.encrypted { " [ENC]" } else { "" },
                            );
                            let rec = event_ingestion::sigex_to_log_record(
                                "traffic_session",
                                &body,
                                Some(session_evt.freq_mhz),
                                None,
                            );
                            pipeline.ingest(rec);
                        }
                        last_flush = std::time::Instant::now();
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("SIGEX T1 bridge lagged by {} messages", n);
                }
                Err(_) => break,
            }
        }
        tracing::info!("SIGEX Tier 1 (SessionTracker) stopped");
    });
}

/// Tier 2 — encryption & protocol metadata from P25 voice events.
///
/// Subscribes to protocol broadcast, feeds P25 metadata into
/// `EncryptionTracker` (key rotation detection) and `ProtocolTracker`
/// (new UID detection). Emits SIEM events on discoveries.
fn spawn_tier2(
    rt: &tokio::runtime::Runtime,
    state: AppState,
    pipeline: Arc<IngestionPipeline>,
) {
    rt.spawn(async move {
        let mut rx = state.subscribe_protocol();
        let mut enc_tracker = rf_sigex::EncryptionTracker::new();
        let mut proto_tracker = rf_sigex::ProtocolTracker::new();

        tracing::info!("SIGEX Tier 2 (Encryption + Protocol) started");

        loop {
            if state.is_shutdown() { break; }
            match rx.recv().await {
                Ok(msg) => {
                    if state.is_shutdown() { break; }

                    // Sync operation context
                    let op_id = state.config().active_operation_id;
                    enc_tracker.set_operation_id(op_id);
                    proto_tracker.set_operation_id(op_id);

                    let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    if msg_type == "p25" {
                        let tgid = msg.get("talkgroup")
                            .and_then(|t| t.as_u64())
                            .unwrap_or(0) as u32;
                        let source_unit = msg.get("source_unit")
                            .and_then(|s| s.as_u64())
                            .map(|s| s as u32);
                        let encrypted = msg.get("encrypted")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                        let algorithm = msg.get("algorithm")
                            .and_then(|a| a.as_str())
                            .map(String::from);
                        let key_id = msg.get("key_id")
                            .and_then(|k| k.as_u64())
                            .map(|k| k as u16);
                        let nac = msg.get("nac")
                            .and_then(|n| n.as_u64())
                            .map(|n| n as u16);
                        let freq_mhz = msg.get("freq_mhz")
                            .and_then(|f| f.as_f64());

                        if tgid > 0 {
                            // Feed encryption tracker
                            let crypto_ev = rf_sigex::CryptoEvent {
                                tgid,
                                source_unit,
                                encrypted,
                                algorithm,
                                key_id,
                                system: "Portland".into(),
                                freq_mhz,
                            };
                            if let Some(desc) = enc_tracker.feed(&crypto_ev, state.db()) {
                                // Key rotation detected
                                let body = format!("TG:{} — {}", tgid, desc);
                                let rec = event_ingestion::sigex_to_log_record(
                                    "crypto_rotation",
                                    &body,
                                    freq_mhz,
                                    source_unit,
                                );
                                pipeline.ingest(rec);
                            }

                            // Feed protocol tracker
                            let proto_ev = rf_sigex::ProtocolEvent {
                                protocol: "P25".into(),
                                tgid: Some(tgid),
                                source_unit,
                                nac,
                                system: "Portland".into(),
                                freq_mhz,
                            };
                            if let Some(desc) = proto_tracker.feed(&proto_ev, state.db()) {
                                // New UID detected
                                let body = format!("TG:{} — {}", tgid, desc);
                                let rec = event_ingestion::sigex_to_log_record(
                                    "new_uid",
                                    &body,
                                    freq_mhz,
                                    source_unit,
                                );
                                pipeline.ingest(rec);
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("SIGEX T2 bridge lagged by {} messages", n);
                }
                Err(_) => break,
            }
        }
        tracing::info!("SIGEX Tier 2 (Encryption + Protocol) stopped");
    });
}

/// Tier 3 — P25 control channel network mapping from TSBK messages.
///
/// Subscribes to protocol broadcast, parses TSBK payloads into
/// `NetworkTracker` for channel param resolution, site topology,
/// voice grants, affiliations, and registrations. Cross-feeds
/// encryption data to a local `EncryptionTracker` for grant-level
/// crypto posture.
fn spawn_tier3(
    rt: &tokio::runtime::Runtime,
    state: AppState,
    pipeline: Arc<IngestionPipeline>,
) {
    rt.spawn(async move {
        let mut rx = state.subscribe_protocol();
        let mut net_tracker = rf_sigex::NetworkTracker::new();
        let mut enc_tracker = rf_sigex::EncryptionTracker::new();

        tracing::info!("SIGEX Tier 3 (NetworkTracker) started");

        loop {
            if state.is_shutdown() { break; }
            match rx.recv().await {
                Ok(msg) => {
                    if state.is_shutdown() { break; }

                    // Sync operation context
                    let op_id = state.config().active_operation_id;
                    net_tracker.set_operation_id(op_id);
                    enc_tracker.set_operation_id(op_id);

                    if msg.get("type").and_then(|t| t.as_str()) == Some("tsbk") {
                        if let Some(payload_val) = msg.get("payload") {
                            match serde_json::from_value::<rf_p25::TsbkData>(
                                payload_val.clone(),
                            ) {
                                Ok(payload) => {
                                    let desc = net_tracker.feed_with_enc(
                                        &payload,
                                        state.db(),
                                        Some(&mut enc_tracker),
                                    );

                                    if let Some(ref desc) = desc {
                                        // Notable network event — emit SIEM event
                                        // Determine subtype from description
                                        let subtype = if desc.contains("EMERGENCY") {
                                            "emergency_grant"
                                        } else if desc.contains("registered") {
                                            "new_uid"
                                        } else if desc.contains("denied") {
                                            "access_deny"
                                        } else {
                                            "network_event"
                                        };

                                        let severity_rec = event_ingestion::sigex_to_log_record(
                                            subtype,
                                            desc,
                                            None,
                                            None,
                                        );
                                        pipeline.ingest(severity_rec);
                                    }
                                }
                                Err(e) => {
                                    let opcode = msg.get("opcode")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("?");
                                    tracing::warn!(
                                        "SIGEX T3: failed to deserialize TSBK op={}: {}",
                                        opcode, e
                                    );
                                }
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("SIGEX T3 bridge lagged by {} messages", n);
                }
                Err(_) => break,
            }
        }
        tracing::info!("SIGEX Tier 3 (NetworkTracker) stopped");
    });
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
