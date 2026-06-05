//! E-SIEM-3: Event ingestion pipeline — converts raw signals into LogRecords,
//! enriches them, publishes to EventBus, and persists via batch writer.

use std::sync::Arc;

use rf_events::{
    EventBus, EventSource, LogRecord, Severity,
    event::event_types,
    pipeline::{Enricher, GpsEnricher, IngestionPipeline, OperationEnricher, SessionEnricher},
};

// ── Batch Writer ───────────────────────────────────────────────

/// Background task that subscribes to EventBus and batch-writes to SQLite.
/// Flushes every 200ms or when buffer hits 100 events, whichever is first.
pub async fn batch_writer_task(bus: EventBus, db: rf_db::Db) {
    const FLUSH_INTERVAL_MS: u64 = 200;
    const FLUSH_THRESHOLD: usize = 100;

    let mut rx = bus.subscribe();
    let mut buffer: Vec<LogRecord> = Vec::with_capacity(FLUSH_THRESHOLD * 2);
    let mut flush_timer = tokio::time::interval(
        std::time::Duration::from_millis(FLUSH_INTERVAL_MS),
    );
    flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    tracing::info!("SIEM batch writer started (flush: {}ms / {} events)", FLUSH_INTERVAL_MS, FLUSH_THRESHOLD);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(record) => {
                        buffer.push((*record).clone());
                        if buffer.len() >= FLUSH_THRESHOLD {
                            flush_batch(&db, &mut buffer);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("SIEM batch writer lagged — dropped {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("SIEM batch writer: EventBus closed, flushing final batch");
                        if !buffer.is_empty() {
                            flush_batch(&db, &mut buffer);
                        }
                        break;
                    }
                }
            }
            _ = flush_timer.tick() => {
                if !buffer.is_empty() {
                    flush_batch(&db, &mut buffer);
                }
            }
        }
    }
}

fn flush_batch(db: &rf_db::Db, buffer: &mut Vec<LogRecord>) {
    let count = buffer.len();
    match db.batch_insert_events(buffer) {
        Ok(_) => {
            tracing::debug!("SIEM batch writer: flushed {} events", count);
        }
        Err(e) => {
            tracing::error!("SIEM batch writer: flush failed ({} events): {}", count, e);
        }
    }
    buffer.clear();
}

// ── Pipeline Factory ───────────────────────────────────────────

/// Session gap timeout: finalize a transmission session after 5 seconds of silence.
const SESSION_GAP_TIMEOUT_SEC: u64 = 5;

/// Create the ingestion pipeline with GPS, Operation, and Session enrichers.
/// Returns the pipeline and handles to update enricher state.
///
/// Enricher order: GPS → Operation → Session Correlator.
/// The session correlator runs last so that trace_id/span_id are assigned
/// after GPS and operation context are already stamped.
pub fn create_pipeline(
    bus: EventBus,
) -> (IngestionPipeline, Arc<GpsEnricher>, Arc<OperationEnricher>, Arc<SessionEnricher>) {
    let gps = Arc::new(GpsEnricher::new());
    let ops = Arc::new(OperationEnricher::new());
    let sessions = Arc::new(SessionEnricher::new(SESSION_GAP_TIMEOUT_SEC));

    let mut pipeline = IngestionPipeline::new(bus);
    pipeline.add_enricher(Box::new(ArcEnricher(gps.clone())));
    pipeline.add_enricher(Box::new(ArcEnricher(ops.clone())));
    pipeline.add_enricher(Box::new(ArcEnricher(sessions.clone())));

    (pipeline, gps, ops, sessions)
}

/// Wrapper to use Arc<T: Enricher> as Box<dyn Enricher>.
struct ArcEnricher<T: Enricher>(Arc<T>);

impl<T: Enricher> Enricher for ArcEnricher<T> {
    fn enrich(&self, record: &mut LogRecord) -> bool {
        self.0.enrich(record)
    }
}

// ── LogRecord Converters ───────────────────────────────────────

/// Convert a spectrum signal detection into a LogRecord.
pub fn signal_to_log_record(
    sig: &rf_scan::ScanDetection,
    band: &str,
    device_key: &str,
) -> LogRecord {
    LogRecord::new(
        EventSource::Spectrum,
        Severity::Info,
        event_types::SPECTRUM_DETECT,
        format!("{:.4} MHz {} {:.0} dBm", sig.freq, sig.cls, sig.power),
    )
    .with_freq(sig.freq)
    .with_band(band)
    .with_device(device_key)
    .with_classification(&sig.cls)
}

/// Convert a P25 TSBK message into a LogRecord.
pub fn tsbk_to_log_record(msg: &serde_json::Value) -> Option<LogRecord> {
    let payload = msg.get("payload")?;
    let ptype = payload.get("type")?.as_str()?;

    let (event_type, severity, body) = match ptype {
        "GroupVoiceGrant" | "UnitVoiceGrant" => {
            let tg = payload.get("talkgroup").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let uid = payload.get("source").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let freq = payload.get("voice_freq").and_then(|v| v.as_f64());
            let event_type = event_types::P25_GRANT;
            let body = format!("{} TG:{} UID:{}", ptype, tg, uid);
            let mut rec = LogRecord::new(EventSource::Protocol, Severity::Info, event_type, body);
            rec.talkgroup = Some(tg);
            if uid > 0 { rec.source_unit = Some(uid); }
            rec.freq_mhz = freq;
            if let Some(nac) = msg.get("nac").and_then(|v| v.as_u64()) {
                rec.nac = Some(nac as u32);
            }
            if let Some(enc) = payload.get("encrypted").and_then(|v| v.as_bool()) {
                rec.encrypted = Some(enc);
            }
            return Some(rec);
        }
        "GroupVoiceUpdate" => {
            let updates = payload.get("updates").and_then(|u| u.as_array());
            let count = updates.map(|a| a.len()).unwrap_or(0);
            (event_types::P25_UPDATE, Severity::Debug,
             format!("GroupVoiceUpdate ({} channels)", count))
        }
        "UnitRegistrationResponse" => {
            let uid = payload.get("source").and_then(|v| v.as_u64()).unwrap_or(0);
            (event_types::P25_REGISTER, Severity::Notice,
             format!("UnitRegistration UID:{}", uid))
        }
        "UnitDeregistration" => {
            let uid = payload.get("source").and_then(|v| v.as_u64()).unwrap_or(0);
            (event_types::P25_DEREGISTER, Severity::Notice,
             format!("UnitDeregistration UID:{}", uid))
        }
        "GroupAffiliationResponse" => {
            let tg = payload.get("talkgroup").and_then(|v| v.as_u64()).unwrap_or(0);
            let uid = payload.get("source").and_then(|v| v.as_u64()).unwrap_or(0);
            (event_types::P25_AFFILIATION, Severity::Debug,
             format!("Affiliation TG:{} UID:{}", tg, uid))
        }
        "DenyResponse" => {
            let uid = payload.get("target").and_then(|v| v.as_u64()).unwrap_or(0);
            (event_types::P25_DENY, Severity::Warn,
             format!("DenyResponse target UID:{}", uid))
        }
        "AdjacentStatusBroadcast" => {
            (event_types::P25_ADJACENT, Severity::Debug,
             "AdjacentStatusBroadcast".to_string())
        }
        "NetworkStatusBroadcast" => {
            let wacn = payload.get("wacn").and_then(|v| v.as_u64()).unwrap_or(0);
            let sys = payload.get("system_id").and_then(|v| v.as_u64()).unwrap_or(0);
            (event_types::P25_NET_STATUS, Severity::Debug,
             format!("NetworkStatus WACN:0x{:X} SYS:0x{:X}", wacn, sys))
        }
        "RfssStatusBroadcast" => {
            (event_types::P25_RFSS_STATUS, Severity::Debug,
             "RfssStatusBroadcast".to_string())
        }
        "ChannelParamsUpdate" => {
            let id = payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            (event_types::P25_CHAN_PARAMS, Severity::Debug,
             format!("ChannelParamsUpdate id:{}", id))
        }
        _ => {
            // Unknown/reserved TSBK — still log it
            (event_types::P25_CC_STATUS, Severity::Trace,
             format!("TSBK {}", ptype))
        }
    };

    let mut rec = LogRecord::new(EventSource::Protocol, severity, event_type, body);

    // Extract common fields from payload
    if let Some(tg) = payload.get("talkgroup").and_then(|v| v.as_u64()) {
        rec.talkgroup = Some(tg as u32);
    }
    if let Some(uid) = payload.get("source").and_then(|v| v.as_u64()) {
        if uid > 0 { rec.source_unit = Some(uid as u32); }
    }
    if let Some(nac) = msg.get("nac").and_then(|v| v.as_u64()) {
        rec.nac = Some(nac as u32);
    }
    if let Some(enc) = payload.get("encrypted").and_then(|v| v.as_bool()) {
        rec.encrypted = Some(enc);
    }

    Some(rec)
}

/// Convert a P25 voice/metadata event (`"type": "p25"` from dsp_bridge) into a LogRecord.
/// These messages carry duid, talkgroup, source_unit, nac, encrypted — all needed
/// for session correlation (trace_id assignment via SessionCorrelator).
pub fn p25_metadata_to_log_record(msg: &serde_json::Value) -> LogRecord {
    let duid = msg.get("duid").and_then(|v| v.as_str()).unwrap_or("");
    let tg = msg.get("talkgroup").and_then(|v| v.as_u64());
    let uid = msg.get("source_unit").and_then(|v| v.as_u64());
    let nac = msg.get("nac").and_then(|v| v.as_u64());
    let enc = msg.get("encrypted").and_then(|v| v.as_bool());
    let freq = msg.get("freq_mhz").and_then(|v| v.as_f64());

    let body = format!("P25 {} TG:{} UID:{}",
        duid,
        tg.unwrap_or(0),
        uid.unwrap_or(0),
    );

    let mut rec = LogRecord::new(
        EventSource::Protocol,
        Severity::Trace,
        event_types::P25_VOICE,
        body,
    );
    rec.freq_mhz = freq;
    rec.talkgroup = tg.map(|t| t as u32);
    if let Some(u) = uid {
        if u > 0 { rec.source_unit = Some(u as u32); }
    }
    rec.nac = nac.map(|n| n as u32);
    rec.encrypted = enc;
    if !duid.is_empty() {
        rec.attributes.insert("duid".into(), rf_events::AttributeValue::String(duid.to_string()));
    }
    rec
}

/// Convert an RDS metadata update into a LogRecord.
pub fn rds_to_log_record(rds: &serde_json::Value) -> LogRecord {
    let ps = rds.get("ps").and_then(|v| v.as_str()).unwrap_or("");
    let rt = rds.get("rt").and_then(|v| v.as_str()).unwrap_or("");
    let freq = rds.get("freq_mhz").and_then(|v| v.as_f64());
    let pi = rds.get("pi").and_then(|v| v.as_str()).unwrap_or("");

    let body = if !ps.is_empty() && !rt.is_empty() {
        format!("RDS: {} — {}", ps.trim(), rt.trim())
    } else if !ps.is_empty() {
        format!("RDS: {}", ps.trim())
    } else {
        format!("RDS PI:{}", pi)
    };

    let mut rec = LogRecord::new(
        EventSource::Protocol,
        Severity::Info,
        event_types::RDS_UPDATE,
        body,
    );
    rec.freq_mhz = freq;
    if !ps.is_empty() {
        rec.attributes.insert("rds.ps".into(), rf_events::AttributeValue::String(ps.to_string()));
    }
    if !rt.is_empty() {
        rec.attributes.insert("rds.rt".into(), rf_events::AttributeValue::String(rt.to_string()));
    }
    if !pi.is_empty() {
        rec.attributes.insert("rds.pi".into(), rf_events::AttributeValue::String(pi.to_string()));
    }
    rec
}

/// Convert a SIGEX event into a LogRecord.
///
/// Subtypes: new_emitter, emitter_reappearance, uid_fp_mismatch,
/// traffic_session, crypto_rotation, new_uid, network_event,
/// emergency_grant, access_deny
pub fn sigex_to_log_record(
    subtype: &str,
    body: &str,
    freq_mhz: Option<f64>,
    uid: Option<u32>,
) -> LogRecord {
    let event_type = match subtype {
        "new_emitter" => event_types::SIGEX_EMITTER_NEW,
        "emitter_reappearance" => event_types::SIGEX_EMITTER_RETURN,
        "uid_fp_mismatch" => event_types::SIGEX_UID_MISMATCH,
        "traffic_session" => event_types::SIGEX_TRAFFIC_SESSION,
        "crypto_rotation" => event_types::SIGEX_CRYPTO_ROTATION,
        "new_uid" => event_types::SIGEX_NEW_UID,
        "emergency_grant" => event_types::SIGEX_EMERGENCY,
        "access_deny" => event_types::SIGEX_ACCESS_DENY,
        "network_event" => event_types::SIGEX_NETWORK_EVENT,
        _ => event_types::SIGEX_ANOMALY,
    };
    let severity = match subtype {
        "uid_fp_mismatch" | "emergency_grant" => Severity::Warn,
        "access_deny" => Severity::Notice,
        "traffic_session" => Severity::Info,
        "crypto_rotation" => Severity::Warn,
        "new_uid" => Severity::Notice,
        _ => Severity::Notice,
    };
    let mut rec = LogRecord::new(EventSource::Sigex, severity, event_type, body);
    rec.freq_mhz = freq_mhz;
    rec.source_unit = uid;
    rec
}

// ── System Events ──────────────────────────────────────────────

/// Emit a system.mode.change event.
pub fn mode_change_record(from: &str, to: &str, freq: Option<f64>) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Notice,
        event_types::SYSTEM_MODE_CHANGE,
        format!("Mode: {} → {}", from, to),
    );
    rec.freq_mhz = freq;
    rec.attributes.insert("from_mode".into(), rf_events::AttributeValue::String(from.to_string()));
    rec.attributes.insert("to_mode".into(), rf_events::AttributeValue::String(to.to_string()));
    rec
}

/// Emit a system.sdr.connect event.
pub fn sdr_connect_record(device_key: &str, driver: &str) -> LogRecord {
    LogRecord::new(
        EventSource::System,
        Severity::Notice,
        event_types::SYSTEM_SDR_CONNECT,
        format!("SDR connected: {} ({})", device_key, driver),
    )
    .with_device(device_key)
}

/// Emit a system.sdr.disconnect (quarantine) event.
pub fn sdr_disconnect_record(device_key: &str) -> LogRecord {
    LogRecord::new(
        EventSource::System,
        Severity::Warn,
        event_types::SYSTEM_SDR_DISCONNECT,
        format!("SDR quarantined: {}", device_key),
    )
    .with_device(device_key)
}

/// Emit a system.recording.start event when a recording slot is created.
#[allow(dead_code)] // Wired in Phase 1 (Recording)
pub fn recording_start_record(
    db_id: i64,
    rec_type: &str,
    freq_mhz: Option<f64>,
) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Info,
        event_types::SYSTEM_REC_START,
        format!("Recording started: id={} type={}", db_id, rec_type),
    );
    rec.attributes.insert("recording_id".into(), rf_events::AttributeValue::Int(db_id));
    rec.attributes.insert("rec_type".into(), rf_events::AttributeValue::String(rec_type.to_string()));
    rec.freq_mhz = freq_mhz;
    rec
}

/// Emit a system.recording.stop event when a recording is finalized.
pub fn recording_stop_record(db_id: i64, duration_sec: f64, file_size: i64) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Info,
        event_types::SYSTEM_REC_STOP,
        format!("Recording finalized: id={} {:.1}s {}KB", db_id, duration_sec, file_size / 1024),
    );
    rec.attributes.insert("recording_id".into(), rf_events::AttributeValue::Int(db_id));
    rec.attributes.insert("duration_sec".into(), rf_events::AttributeValue::Float(duration_sec));
    rec.attributes.insert("file_size_bytes".into(), rf_events::AttributeValue::Int(file_size));
    rec
}

/// Emit a system.clip.start event when a P25 transmission begins (HDU).
pub fn clip_start_record(
    freq_mhz: f64,
    talkgroup: Option<u32>,
    source_unit: Option<u32>,
    encrypted: bool,
) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::Protocol,
        Severity::Info,
        event_types::SYSTEM_CLIP_START,
        format!(
            "P25 TX start: {:.4} MHz TG:{} UID:{}{}",
            freq_mhz,
            talkgroup.unwrap_or(0),
            source_unit.unwrap_or(0),
            if encrypted { " [ENC]" } else { "" },
        ),
    );
    rec.freq_mhz = Some(freq_mhz);
    rec.talkgroup = talkgroup;
    rec.source_unit = source_unit;
    rec.encrypted = Some(encrypted);
    rec
}

/// Emit a system.operation.start event when an operation is activated.
pub fn op_start_record(op_id: i64, name: &str, profile: &str) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Notice,
        event_types::SYSTEM_OP_START,
        format!("Operation started: {} (id={}, profile={})", name, op_id, profile),
    );
    rec.attributes.insert("operation_id".into(), rf_events::AttributeValue::Int(op_id));
    rec.attributes.insert("operation_name".into(), rf_events::AttributeValue::String(name.to_string()));
    rec.attributes.insert("profile".into(), rf_events::AttributeValue::String(profile.to_string()));
    rec
}

/// Emit a system.operation.stop event when an operation is stopped/completed.
pub fn op_stop_record(op_id: i64, name: &str) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Notice,
        event_types::SYSTEM_OP_STOP,
        format!("Operation stopped: {} (id={})", name, op_id),
    );
    rec.attributes.insert("operation_id".into(), rf_events::AttributeValue::Int(op_id));
    rec.attributes.insert("operation_name".into(), rf_events::AttributeValue::String(name.to_string()));
    rec
}

/// Emit a system.site.enter event when the operator enters a geofenced site.
pub fn site_enter_record(site_id: i64, site_name: &str, session_id: i64) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Info,
        event_types::SYSTEM_SITE_ENTER,
        format!("Entered site: {} (id={}, session={})", site_name, site_id, session_id),
    );
    rec.attributes.insert("site_id".into(), rf_events::AttributeValue::Int(site_id));
    rec.attributes.insert("site_name".into(), rf_events::AttributeValue::String(site_name.to_string()));
    rec.attributes.insert("session_id".into(), rf_events::AttributeValue::Int(session_id));
    rec
}

/// Emit a system.site.exit event when the operator leaves a geofenced site.
pub fn site_exit_record(site_id: i64, site_name: &str, session_id: i64) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::System,
        Severity::Info,
        event_types::SYSTEM_SITE_EXIT,
        format!("Exited site: {} (id={}, session={})", site_name, site_id, session_id),
    );
    rec.attributes.insert("site_id".into(), rf_events::AttributeValue::Int(site_id));
    rec.attributes.insert("site_name".into(), rf_events::AttributeValue::String(site_name.to_string()));
    rec.attributes.insert("session_id".into(), rf_events::AttributeValue::Int(session_id));
    rec
}

/// Emit a system.gps.fix event when GPS source changes.
pub fn gps_source_change_record(old_source: &str, new_source: &str) -> LogRecord {
    let severity = if new_source == "none" { Severity::Warn } else { Severity::Info };
    let mut rec = LogRecord::new(
        EventSource::System,
        severity,
        event_types::SYSTEM_GPS_FIX,
        format!("GPS source changed: {} → {}", old_source, new_source),
    );
    rec.attributes.insert("old_source".into(), rf_events::AttributeValue::String(old_source.to_string()));
    rec.attributes.insert("new_source".into(), rf_events::AttributeValue::String(new_source.to_string()));
    rec
}

/// Emit a system.clip.end event when a P25 transmission ends (TLC).
pub fn clip_end_record(
    freq_mhz: f64,
    talkgroup: Option<u32>,
    source_unit: Option<u32>,
    encrypted: bool,
) -> LogRecord {
    let mut rec = LogRecord::new(
        EventSource::Protocol,
        Severity::Info,
        event_types::SYSTEM_CLIP_END,
        format!(
            "P25 TX end: {:.4} MHz TG:{} UID:{}{}",
            freq_mhz,
            talkgroup.unwrap_or(0),
            source_unit.unwrap_or(0),
            if encrypted { " [ENC]" } else { "" },
        ),
    );
    rec.freq_mhz = Some(freq_mhz);
    rec.talkgroup = talkgroup;
    rec.source_unit = source_unit;
    rec.encrypted = Some(encrypted);
    rec
}
