use rusqlite::params;

use rf_events::{
    AlertFiring, AlertRule, CustomEventRule, EventQuery, LogRecord, ParamValue,
    alert::{AlertAction, AlertCondition, AlertPriority},
    custom::CustomEventCondition,
    event::{Severity, EventSource},
    query::Filter,
    session::TransmissionSession,
};

use crate::Db;

/// Convert rf-events ParamValue list into rusqlite-bindable values.
fn to_rusqlite_params(params: &[ParamValue]) -> Vec<rusqlite::types::Value> {
    params.iter().map(|p| match p {
        ParamValue::Null => rusqlite::types::Value::Null,
        ParamValue::Int(n) => rusqlite::types::Value::Integer(*n),
        ParamValue::Float(f) => rusqlite::types::Value::Real(*f),
        ParamValue::Text(s) => rusqlite::types::Value::Text(s.clone()),
    }).collect()
}

// ── Event Log Writes ────────────────────────────────────────

impl Db {
    /// Insert a single event into event_log. Returns the row id.
    pub fn insert_event(&self, evt: &LogRecord) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO event_log (
                timestamp_ns, ts_bucket, severity, source, event_type, body,
                freq_mhz, talkgroup, source_unit, nac, encrypted,
                band, device_key, classification,
                trace_id, span_id, operation_id, site_session_id,
                receiver_lat, receiver_lon, attributes
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14,
                ?15, ?16, ?17, ?18,
                ?19, ?20, ?21
            )",
            params![
                evt.timestamp_ns as i64,
                evt.ts_bucket as i64,
                evt.severity.as_u8(),
                evt.source.as_u8(),
                evt.event_type,
                evt.body,
                evt.freq_mhz,
                evt.talkgroup.map(|t| t as i64),
                evt.source_unit.map(|u| u as i64),
                evt.nac.map(|n| n as i64),
                evt.encrypted.map(|e| e as i32),
                evt.band,
                evt.device_key,
                evt.classification,
                evt.trace_id.map(|t| t as i64),
                evt.span_id.map(|s| s as i64),
                evt.operation_id,
                evt.site_session_id,
                evt.receiver_lat,
                evt.receiver_lon,
                evt.attributes_json(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Batch insert events. Much faster than individual inserts.
    /// Returns the number of events inserted.
    pub fn batch_insert_events(&self, events: &[LogRecord]) -> Result<usize, rusqlite::Error> {
        if events.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO event_log (
                    timestamp_ns, ts_bucket, severity, source, event_type, body,
                    freq_mhz, talkgroup, source_unit, nac, encrypted,
                    band, device_key, classification,
                    trace_id, span_id, operation_id, site_session_id,
                    receiver_lat, receiver_lon, attributes
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, ?9, ?10, ?11,
                    ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18,
                    ?19, ?20, ?21
                )",
            )?;
            for evt in events {
                stmt.execute(params![
                    evt.timestamp_ns as i64,
                    evt.ts_bucket as i64,
                    evt.severity.as_u8(),
                    evt.source.as_u8(),
                    evt.event_type,
                    evt.body,
                    evt.freq_mhz,
                    evt.talkgroup.map(|t| t as i64),
                    evt.source_unit.map(|u| u as i64),
                    evt.nac.map(|n| n as i64),
                    evt.encrypted.map(|e| e as i32),
                    evt.band,
                    evt.device_key,
                    evt.classification,
                    evt.trace_id.map(|t| t as i64),
                    evt.span_id.map(|s| s as i64),
                    evt.operation_id,
                    evt.site_session_id,
                    evt.receiver_lat,
                    evt.receiver_lon,
                    evt.attributes_json(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(events.len())
    }

    /// Delete events older than the given nanosecond timestamp.
    /// Returns the number of rows deleted.
    pub fn purge_events_before(&self, before_ns: u64) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let bucket = rf_events::event::bucket_30s(before_ns);
        conn.execute(
            "DELETE FROM event_log WHERE ts_bucket < ?1 AND timestamp_ns < ?2",
            params![bucket as i64, before_ns as i64],
        )?;
        Ok(conn.changes() as usize)
    }

    /// Count total events in the log.
    pub fn event_count(&self) -> Result<u64, rusqlite::Error> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0))?;
        Ok(count as u64)
    }
}

// ── Event Log Reads (Query Engine) ──────────────────────────

impl Db {
    /// Execute an EventQuery and return matching LogRecords.
    /// Uses parameterized queries to prevent SQL injection.
    pub fn query_events(&self, query: &EventQuery) -> Result<Vec<LogRecord>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params) = query.to_param_sql();
        tracing::debug!("event query: {} (params: {})", query.to_sql(), params.len());

        let rp = to_rusqlite_params(&params);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = rp.iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(row_to_log_record(row))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Execute an aggregation query and return rows as JSON values.
    /// Uses parameterized queries to prevent SQL injection.
    pub fn query_events_aggregate(
        &self,
        query: &EventQuery,
    ) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params) = query.to_param_sql();
        tracing::debug!("event agg query: {} (params: {})", query.to_sql(), params.len());

        let rp = to_rusqlite_params(&params);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = rp.iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();

        let mut stmt = conn.prepare(&sql)?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let mut map = serde_json::Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json_val = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                    rusqlite::types::Value::Real(f) => serde_json::json!(f),
                    rusqlite::types::Value::Text(s) => serde_json::json!(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                };
                map.insert(name.clone(), json_val);
            }
            Ok(serde_json::Value::Object(map))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Count total matching events for pagination.
    /// Uses parameterized queries to prevent SQL injection.
    pub fn count_events(&self, query: &EventQuery) -> Result<u64, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params) = query.to_count_sql();

        let rp = to_rusqlite_params(&params);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = rp.iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();

        let count: i64 = conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Get a single event by ID.
    pub fn get_event(&self, id: u64) -> Result<Option<LogRecord>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT * FROM event_log WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id as i64], |row| Ok(row_to_log_record(row)))?;
        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Get events by trace_id (all events in a transmission session).
    pub fn get_trace_events(&self, trace_id: u64) -> Result<Vec<LogRecord>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM event_log WHERE trace_id = ?1 ORDER BY timestamp_ns ASC",
        )?;
        let rows = stmt.query_map(params![trace_id as i64], |row| Ok(row_to_log_record(row)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get distinct values for a field (for facet building in the UI).
    /// Returns up to `limit` most common values with their counts.
    pub fn event_facets(
        &self,
        field: &str,
        time_start_ns: Option<u64>,
        time_end_ns: Option<u64>,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, rusqlite::Error> {
        let col = rf_events::query::Field::parse(field).to_sql();
        let conn = self.read_conn();

        let mut sql = format!(
            "SELECT {col}, COUNT(*) as cnt FROM event_log WHERE {col} IS NOT NULL"
        );
        let mut bind_params: Vec<rusqlite::types::Value> = Vec::new();
        if let (Some(start), Some(end)) = (time_start_ns, time_end_ns) {
            let bucket_start = rf_events::event::bucket_30s(start) as i64;
            let bucket_end = (rf_events::event::bucket_30s(end) + 30_000_000_000) as i64;
            let pi = bind_params.len();
            bind_params.push(rusqlite::types::Value::Integer(bucket_start));
            bind_params.push(rusqlite::types::Value::Integer(bucket_end));
            bind_params.push(rusqlite::types::Value::Integer(start as i64));
            bind_params.push(rusqlite::types::Value::Integer(end as i64));
            sql.push_str(&format!(
                " AND ts_bucket >= ?{} AND ts_bucket <= ?{} AND timestamp_ns >= ?{} AND timestamp_ns <= ?{}",
                pi + 1, pi + 2, pi + 3, pi + 4
            ));
        }
        sql.push_str(&format!(" GROUP BY {col} ORDER BY cnt DESC LIMIT {limit}"));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = bind_params.iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let val: String = row.get::<_, rusqlite::types::Value>(0)
                .map(|v| match v {
                    rusqlite::types::Value::Text(s) => s,
                    rusqlite::types::Value::Integer(n) => n.to_string(),
                    rusqlite::types::Value::Real(f) => format!("{f:.4}"),
                    _ => "NULL".to_string(),
                })?;
            let cnt: i64 = row.get(1)?;
            Ok((val, cnt as u64))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

// ── Row → LogRecord mapping ─────────────────────────────────

fn row_to_log_record(row: &rusqlite::Row) -> LogRecord {
    // Column indices: id=0, timestamp_ns=1, ts_bucket=2, severity=3, source=4,
    // event_type=5, body=6, freq_mhz=7, talkgroup=8, source_unit=9, nac=10,
    // encrypted=11, band=12, device_key=13, classification=14, trace_id=15,
    // span_id=16, operation_id=17, site_session_id=18, receiver_lat=19,
    // receiver_lon=20, attributes=21
    let attrs_json: String = row.get::<_, Option<String>>(21).unwrap_or(None).unwrap_or_default();
    let attributes = serde_json::from_str(&attrs_json).unwrap_or_default();

    LogRecord {
        id: row.get::<_, i64>(0).unwrap_or(0) as u64,
        timestamp_ns: row.get::<_, i64>(1).unwrap_or(0) as u64,
        ts_bucket: row.get::<_, i64>(2).unwrap_or(0) as u64,
        severity: Severity::from_u8(row.get::<_, u8>(3).unwrap_or(9)),
        source: EventSource::from_u8(row.get::<_, u8>(4).unwrap_or(3)),
        event_type: row.get::<_, String>(5).unwrap_or_default(),
        body: row.get::<_, String>(6).unwrap_or_default(),
        freq_mhz: row.get::<_, Option<f64>>(7).unwrap_or(None),
        talkgroup: row.get::<_, Option<i64>>(8).unwrap_or(None).map(|n| n as u32),
        source_unit: row.get::<_, Option<i64>>(9).unwrap_or(None).map(|n| n as u32),
        nac: row.get::<_, Option<i64>>(10).unwrap_or(None).map(|n| n as u32),
        encrypted: row.get::<_, Option<i32>>(11).unwrap_or(None).map(|n| n != 0),
        band: row.get::<_, Option<String>>(12).unwrap_or(None),
        device_key: row.get::<_, Option<String>>(13).unwrap_or(None),
        classification: row.get::<_, Option<String>>(14).unwrap_or(None),
        trace_id: row.get::<_, Option<i64>>(15).unwrap_or(None).map(|n| n as u64),
        span_id: row.get::<_, Option<i64>>(16).unwrap_or(None).map(|n| n as u64),
        operation_id: row.get::<_, Option<i64>>(17).unwrap_or(None),
        site_session_id: row.get::<_, Option<i64>>(18).unwrap_or(None),
        receiver_lat: row.get::<_, Option<f64>>(19).unwrap_or(None),
        receiver_lon: row.get::<_, Option<f64>>(20).unwrap_or(None),
        attributes,
    }
}

// ── Transmission Sessions ───────────────────────────────────

impl Db {
    /// Insert a finalized transmission session.
    pub fn insert_transmission_session(
        &self,
        session: &TransmissionSession,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO transmission_sessions (
                trace_id, start_ns, end_ns, talkgroup, source_unit, nac,
                freq_mhz, encrypted, event_count,
                grant_event_id, recording_id, fingerprint_id,
                operation_id, site_session_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                session.trace_id as i64,
                session.start_ns as i64,
                if session.last_event_ns > 0 { Some(session.last_event_ns as i64) } else { None },
                session.talkgroup.map(|t| t as i64),
                session.source_unit.map(|u| u as i64),
                session.nac.map(|n| n as i64),
                session.freq_mhz,
                session.encrypted as i32,
                session.event_count as i64,
                session.grant_event_id.map(|id| id as i64),
                session.recording_id,
                session.fingerprint_id,
                session.operation_id,
                session.site_session_id,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a transmission session by trace_id.
    pub fn get_transmission_session(
        &self,
        trace_id: u64,
    ) -> Result<Option<TransmissionSession>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM transmission_sessions WHERE trace_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![trace_id as i64], |row| {
            Ok(TransmissionSession {
                trace_id: row.get::<_, i64>(1).unwrap_or(0) as u64,
                start_ns: row.get::<_, i64>(2).unwrap_or(0) as u64,
                last_event_ns: row.get::<_, Option<i64>>(3).unwrap_or(None).unwrap_or(0) as u64,
                next_span: 0,
                talkgroup: row.get::<_, Option<i64>>(4).unwrap_or(None).map(|n| n as u32),
                source_unit: row.get::<_, Option<i64>>(5).unwrap_or(None).map(|n| n as u32),
                nac: row.get::<_, Option<i64>>(6).unwrap_or(None).map(|n| n as u32),
                freq_mhz: row.get(7).ok().flatten(),
                encrypted: row.get::<_, Option<i32>>(8).unwrap_or(None).unwrap_or(0) != 0,
                event_count: row.get::<_, Option<i64>>(9).unwrap_or(None).unwrap_or(0) as u32,
                grant_event_id: row.get::<_, Option<i64>>(10).unwrap_or(None).map(|n| n as u64),
                recording_id: row.get(11).ok().flatten(),
                fingerprint_id: row.get(12).ok().flatten(),
                operation_id: row.get(13).ok().flatten(),
                site_session_id: row.get(14).ok().flatten(),
            })
        })?;
        match rows.next() {
            Some(Ok(s)) => Ok(Some(s)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// List recent transmission sessions.
    pub fn recent_transmission_sessions(
        &self,
        limit: usize,
    ) -> Result<Vec<TransmissionSession>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM transmission_sessions ORDER BY start_ns DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(TransmissionSession {
                trace_id: row.get::<_, i64>(1).unwrap_or(0) as u64,
                start_ns: row.get::<_, i64>(2).unwrap_or(0) as u64,
                last_event_ns: row.get::<_, Option<i64>>(3).unwrap_or(None).unwrap_or(0) as u64,
                next_span: 0,
                talkgroup: row.get::<_, Option<i64>>(4).unwrap_or(None).map(|n| n as u32),
                source_unit: row.get::<_, Option<i64>>(5).unwrap_or(None).map(|n| n as u32),
                nac: row.get::<_, Option<i64>>(6).unwrap_or(None).map(|n| n as u32),
                freq_mhz: row.get(7).ok().flatten(),
                encrypted: row.get::<_, Option<i32>>(8).unwrap_or(None).unwrap_or(0) != 0,
                event_count: row.get::<_, Option<i64>>(9).unwrap_or(None).unwrap_or(0) as u32,
                grant_event_id: row.get::<_, Option<i64>>(10).unwrap_or(None).map(|n| n as u64),
                recording_id: row.get(11).ok().flatten(),
                fingerprint_id: row.get(12).ok().flatten(),
                operation_id: row.get(13).ok().flatten(),
                site_session_id: row.get(14).ok().flatten(),
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

// ── Spectrum Snapshots ──────────────────────────────────────

/// A downsampled spectrum summary (1 per band per 30-second bucket).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpectrumSnapshot {
    pub id: i64,
    pub ts_bucket: u64,
    pub band: String,
    pub device_key: Option<String>,
    pub noise_floor_db: Option<f64>,
    pub peak_power_db: Option<f64>,
    pub peak_freq_mhz: Option<f64>,
    pub signal_count: Option<i32>,
    pub avg_occupancy: Option<f64>,
    pub operation_id: Option<i64>,
}

impl Db {
    /// Upsert a spectrum snapshot (one per band per 30s bucket).
    pub fn upsert_spectrum_snapshot(
        &self,
        snap: &SpectrumSnapshot,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO spectrum_snapshots (
                ts_bucket, band, device_key,
                noise_floor_db, peak_power_db, peak_freq_mhz,
                signal_count, avg_occupancy, operation_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(ts_bucket, band, device_key) DO UPDATE SET
                noise_floor_db = excluded.noise_floor_db,
                peak_power_db = excluded.peak_power_db,
                peak_freq_mhz = excluded.peak_freq_mhz,
                signal_count = excluded.signal_count,
                avg_occupancy = excluded.avg_occupancy",
            params![
                snap.ts_bucket as i64,
                snap.band,
                snap.device_key.as_deref().unwrap_or(""),
                snap.noise_floor_db,
                snap.peak_power_db,
                snap.peak_freq_mhz,
                snap.signal_count,
                snap.avg_occupancy,
                snap.operation_id,
            ],
        )?;
        Ok(())
    }

    /// Query spectrum snapshots for a time range and optional band filter.
    pub fn query_spectrum_snapshots(
        &self,
        start_bucket: u64,
        end_bucket: u64,
        band: Option<&str>,
    ) -> Result<Vec<SpectrumSnapshot>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut results = Vec::new();

        if let Some(b) = band {
            let mut stmt = conn.prepare(
                "SELECT id, ts_bucket, band, device_key, noise_floor_db, peak_power_db,
                        peak_freq_mhz, signal_count, avg_occupancy, operation_id
                 FROM spectrum_snapshots WHERE ts_bucket >= ?1 AND ts_bucket <= ?2 AND band = ?3
                 ORDER BY ts_bucket ASC",
            )?;
            let rows = stmt.query_map(params![start_bucket as i64, end_bucket as i64, b], |row| {
                Ok(row_to_spectrum_snapshot(row))
            })?;
            for row in rows {
                results.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, ts_bucket, band, device_key, noise_floor_db, peak_power_db,
                        peak_freq_mhz, signal_count, avg_occupancy, operation_id
                 FROM spectrum_snapshots WHERE ts_bucket >= ?1 AND ts_bucket <= ?2
                 ORDER BY ts_bucket ASC",
            )?;
            let rows = stmt.query_map(params![start_bucket as i64, end_bucket as i64], |row| {
                Ok(row_to_spectrum_snapshot(row))
            })?;
            for row in rows {
                results.push(row?);
            }
        }

        Ok(results)
    }
}

fn row_to_spectrum_snapshot(row: &rusqlite::Row) -> SpectrumSnapshot {
    let dk: String = row.get::<_, String>(3).unwrap_or_default();
    SpectrumSnapshot {
        id: row.get(0).unwrap_or(0),
        ts_bucket: row.get::<_, i64>(1).unwrap_or(0) as u64,
        band: row.get(2).unwrap_or_default(),
        device_key: if dk.is_empty() { None } else { Some(dk) },
        noise_floor_db: row.get::<_, Option<f64>>(4).unwrap_or(None),
        peak_power_db: row.get::<_, Option<f64>>(5).unwrap_or(None),
        peak_freq_mhz: row.get::<_, Option<f64>>(6).unwrap_or(None),
        signal_count: row.get::<_, Option<i32>>(7).unwrap_or(None),
        avg_occupancy: row.get::<_, Option<f64>>(8).unwrap_or(None),
        operation_id: row.get::<_, Option<i64>>(9).unwrap_or(None),
    }
}

// ── Alert Rules CRUD ────────────────────────────────────────

impl Db {
    /// Insert a new alert rule. Returns the row id.
    pub fn insert_alert_rule(&self, rule: &AlertRule) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let filter_json = serde_json::to_string(&rule.filter).unwrap_or_default();
        let condition_type = condition_type_str(&rule.condition);
        let condition_json = serde_json::to_string(&rule.condition).unwrap_or_default();
        let actions_json = serde_json::to_string(&rule.actions).unwrap_or_default();

        conn.execute(
            "INSERT INTO alert_rules (name, enabled, priority, filter_json, condition_type, condition_json, actions_json, cooldown_sec)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                rule.name,
                rule.enabled as i32,
                rule.priority.label().to_lowercase(),
                filter_json,
                condition_type,
                condition_json,
                actions_json,
                rule.cooldown_sec as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List all alert rules.
    pub fn list_alert_rules(&self) -> Result<Vec<AlertRule>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, enabled, priority, filter_json, condition_type, condition_json,
                    actions_json, cooldown_sec, last_fired_ns
             FROM alert_rules ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| Ok(row_to_alert_rule(row)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Update an alert rule.
    pub fn update_alert_rule(&self, rule: &AlertRule) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let filter_json = serde_json::to_string(&rule.filter).unwrap_or_default();
        let condition_type = condition_type_str(&rule.condition);
        let condition_json = serde_json::to_string(&rule.condition).unwrap_or_default();
        let actions_json = serde_json::to_string(&rule.actions).unwrap_or_default();

        let changed = conn.execute(
            "UPDATE alert_rules SET
                name = ?1, enabled = ?2, priority = ?3, filter_json = ?4,
                condition_type = ?5, condition_json = ?6, actions_json = ?7,
                cooldown_sec = ?8, updated_at = datetime('now')
             WHERE id = ?9",
            params![
                rule.name,
                rule.enabled as i32,
                rule.priority.label().to_lowercase(),
                filter_json,
                condition_type,
                condition_json,
                actions_json,
                rule.cooldown_sec as i64,
                rule.id,
            ],
        )?;
        Ok(changed > 0)
    }

    /// Delete an alert rule.
    pub fn delete_alert_rule(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changed = conn.execute("DELETE FROM alert_rules WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Update last_fired_ns on an alert rule.
    pub fn mark_alert_fired(&self, rule_id: i64, fired_ns: u64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE alert_rules SET last_fired_ns = ?1 WHERE id = ?2",
            params![fired_ns as i64, rule_id],
        )?;
        Ok(())
    }

    /// Insert an alert firing record.
    pub fn insert_alert_firing(&self, firing: &AlertFiring) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO alert_firings (rule_id, fired_ns, match_count, sample_event_id, acknowledged)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                firing.rule_id,
                firing.fired_ns as i64,
                firing.match_count as i64,
                firing.sample_event_id.map(|id| id as i64),
                firing.acknowledged as i32,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List recent alert firings.
    pub fn recent_alert_firings(&self, limit: usize) -> Result<Vec<AlertFiring>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT af.id, af.rule_id, ar.name, af.fired_ns, af.match_count,
                    af.sample_event_id, af.acknowledged, af.ack_ns
             FROM alert_firings af
             JOIN alert_rules ar ON af.rule_id = ar.id
             ORDER BY af.fired_ns DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(AlertFiring {
                id: row.get(0)?,
                rule_id: row.get(1)?,
                rule_name: row.get(2)?,
                fired_ns: row.get::<_, i64>(3)? as u64,
                match_count: row.get::<_, i64>(4)? as u64,
                sample_event_id: row.get::<_, Option<i64>>(5)?.map(|n| n as u64),
                acknowledged: row.get::<_, i32>(6)? != 0,
                ack_ns: row.get::<_, Option<i64>>(7)?.map(|n| n as u64),
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Acknowledge an alert firing.
    pub fn acknowledge_alert(&self, firing_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let now_ns = rf_events::event::now_ns() as i64;
        let changed = conn.execute(
            "UPDATE alert_firings SET acknowledged = 1, ack_ns = ?1 WHERE id = ?2",
            params![now_ns, firing_id],
        )?;
        Ok(changed > 0)
    }
}

fn condition_type_str(cond: &AlertCondition) -> &'static str {
    match cond {
        AlertCondition::Threshold { .. } => "threshold",
        AlertCondition::Absence { .. } => "absence",
        AlertCondition::RateChange { .. } => "rate_change",
        AlertCondition::FirstOccurrence => "first_occurrence",
    }
}

fn row_to_alert_rule(row: &rusqlite::Row) -> AlertRule {
    let filter: Vec<Filter> = row.get::<_, String>(4)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let condition: AlertCondition = row.get::<_, String>(6)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(AlertCondition::FirstOccurrence);
    let actions: Vec<AlertAction> = row.get::<_, String>(7)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let priority = match row.get::<_, String>(3).unwrap_or_default().as_str() {
        "low" => AlertPriority::Low,
        "high" => AlertPriority::High,
        "critical" => AlertPriority::Critical,
        _ => AlertPriority::Medium,
    };

    AlertRule {
        id: row.get(0).unwrap_or(0),
        name: row.get(1).unwrap_or_default(),
        enabled: row.get::<_, i32>(2).unwrap_or(1) != 0,
        filter,
        condition,
        cooldown_sec: row.get::<_, i64>(8).unwrap_or(60) as u64,
        last_fired_ns: row.get::<_, Option<i64>>(9).unwrap_or(None).map(|n| n as u64),
        actions,
        priority,
    }
}

// ── Custom Event Rules CRUD ─────────────────────────────────

impl Db {
    /// Insert a custom event rule. Returns the row id.
    pub fn insert_custom_event_rule(&self, rule: &CustomEventRule) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let filter_json = serde_json::to_string(&rule.filter).unwrap_or_default();
        let condition_type = custom_condition_type_str(&rule.condition);
        let condition_json = serde_json::to_string(&rule.condition).unwrap_or_default();

        conn.execute(
            "INSERT INTO custom_event_rules (
                name, event_type, description, enabled,
                filter_json, condition_type, condition_json,
                body_template, severity, include_source_events,
                cooldown_sec, chain_depth, max_chain_depth
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rule.name,
                rule.event_type,
                rule.description,
                rule.enabled as i32,
                filter_json,
                condition_type,
                condition_json,
                rule.body_template,
                rule.severity.as_u8(),
                rule.include_source_events as i32,
                rule.cooldown_sec as i64,
                rule.chain_depth as i64,
                rule.max_chain_depth as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List all custom event rules.
    pub fn list_custom_event_rules(&self) -> Result<Vec<CustomEventRule>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, event_type, description, enabled,
                    filter_json, condition_type, condition_json,
                    body_template, severity, include_source_events,
                    cooldown_sec, last_fired_ns, chain_depth, max_chain_depth
             FROM custom_event_rules ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| Ok(row_to_custom_event_rule(row)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Update a custom event rule.
    pub fn update_custom_event_rule(&self, rule: &CustomEventRule) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let filter_json = serde_json::to_string(&rule.filter).unwrap_or_default();
        let condition_type = custom_condition_type_str(&rule.condition);
        let condition_json = serde_json::to_string(&rule.condition).unwrap_or_default();

        let changed = conn.execute(
            "UPDATE custom_event_rules SET
                name = ?1, event_type = ?2, description = ?3, enabled = ?4,
                filter_json = ?5, condition_type = ?6, condition_json = ?7,
                body_template = ?8, severity = ?9, include_source_events = ?10,
                cooldown_sec = ?11, chain_depth = ?12, max_chain_depth = ?13,
                updated_at = datetime('now')
             WHERE id = ?14",
            params![
                rule.name,
                rule.event_type,
                rule.description,
                rule.enabled as i32,
                filter_json,
                condition_type,
                condition_json,
                rule.body_template,
                rule.severity.as_u8(),
                rule.include_source_events as i32,
                rule.cooldown_sec as i64,
                rule.chain_depth as i64,
                rule.max_chain_depth as i64,
                rule.id,
            ],
        )?;
        Ok(changed > 0)
    }

    /// Delete a custom event rule.
    pub fn delete_custom_event_rule(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changed = conn.execute("DELETE FROM custom_event_rules WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Toggle a custom event rule enabled/disabled.
    pub fn toggle_custom_event_rule(&self, id: i64, enabled: bool) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE custom_event_rules SET enabled = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![enabled as i32, id],
        )?;
        Ok(changed > 0)
    }

    /// Update last_fired_ns on a custom event rule.
    pub fn mark_custom_event_fired(&self, rule_id: i64, fired_ns: u64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE custom_event_rules SET last_fired_ns = ?1 WHERE id = ?2",
            params![fired_ns as i64, rule_id],
        )?;
        Ok(())
    }
}

fn custom_condition_type_str(cond: &CustomEventCondition) -> &'static str {
    match cond {
        CustomEventCondition::Threshold { .. } => "threshold",
        CustomEventCondition::Absence { .. } => "absence",
        CustomEventCondition::NewValue { .. } => "new_value",
        CustomEventCondition::Cardinality { .. } => "cardinality",
        CustomEventCondition::RateChange { .. } => "rate_change",
        CustomEventCondition::Correlation { .. } => "correlation",
        CustomEventCondition::Every => "every",
    }
}

fn row_to_custom_event_rule(row: &rusqlite::Row) -> CustomEventRule {
    let filter: Vec<Filter> = row.get::<_, String>(5)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let condition: CustomEventCondition = row.get::<_, String>(7)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(CustomEventCondition::Every);

    CustomEventRule {
        id: row.get(0).unwrap_or(0),
        name: row.get(1).unwrap_or_default(),
        event_type: row.get(2).unwrap_or_default(),
        description: row.get(3).unwrap_or_default(),
        enabled: row.get::<_, i32>(4).unwrap_or(1) != 0,
        filter,
        condition,
        body_template: row.get(8).unwrap_or_default(),
        severity: Severity::from_u8(row.get::<_, u8>(9).unwrap_or(9)),
        include_source_events: row.get::<_, i32>(10).unwrap_or(1) != 0,
        cooldown_sec: row.get::<_, i64>(11).unwrap_or(60) as u64,
        last_fired_ns: row.get::<_, Option<i64>>(12).unwrap_or(None).map(|n| n as u64),
        chain_depth: row.get::<_, i64>(13).unwrap_or(0) as u32,
        max_chain_depth: row.get::<_, i64>(14).unwrap_or(3) as u32,
    }
}

// Saved queries CRUD lives in lib.rs (existing table from earlier migration).
// The v34 migration adds filter_json and view_config_json columns to the existing table.
