use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub id: i64,
    pub template: String,
    pub title: String,
    pub location: Option<String>,
    pub created_at: i64,
    pub author: Option<String>,
    pub export_hash: Option<String>,
}

/// Full report document returned from create (includes all data sections).
#[derive(Serialize)]
pub struct ReportDoc {
    #[serde(flatten)]
    pub meta: Report,
    pub content: serde_json::Value,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Report>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db
        .prepare("SELECT id, template, title, location, created_at, author, export_hash FROM reports ORDER BY created_at DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt
        .query_map([], |r| Ok(Report {
            id: r.get(0)?,
            template: r.get(1)?,
            title: r.get(2)?,
            location: r.get(3)?,
            created_at: r.get(4)?,
            author: r.get(5)?,
            export_hash: r.get(6)?,
        }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (meta, content_json): (Report, Option<String>) = db.query_row(
        "SELECT id, template, title, location, created_at, author, export_hash, content_json FROM reports WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok((Report {
            id: r.get(0)?, template: r.get(1)?, title: r.get(2)?,
            location: r.get(3)?, created_at: r.get(4)?,
            author: r.get(5)?, export_hash: r.get(6)?,
        }, r.get(7)?)),
    ).map_err(|_| StatusCode::NOT_FOUND)?;

    let content: serde_json::Value = content_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    let mut doc = serde_json::to_value(&meta).unwrap_or_default();
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("content".into(), content);
    }
    Ok(Json(doc))
}

#[derive(Deserialize)]
pub struct CreateReport {
    pub template: String,
    pub title: String,
    pub location: Option<String>,
    pub author: Option<String>,
    pub notes: Option<String>,
    pub sections: Option<Vec<String>>,
    pub format: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateReport>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let sections = body.sections.unwrap_or_else(|| vec![
        "summary".into(), "emitters".into(), "anomalies".into(), "drones".into(), "recommend".into(),
    ]);

    // Build report content from DB
    let content = {
        let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        build_content(&db, &sections, body.notes.as_deref(), now)?
    };

    // Compute a simple hash for chain-of-custody
    let content_str = content.to_string();
    let hash = simple_hash(&content_str);
    let content_json = content_str.clone();

    let id = {
        let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db.execute(
            "INSERT INTO reports (template, title, location, created_at, author, content_json, export_hash) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![body.template, body.title, body.location, now, body.author, content_json, hash],
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db.last_insert_rowid()
    };

    let mut doc = serde_json::json!({
        "id": id, "template": body.template, "title": body.title,
        "location": body.location, "created_at": now,
        "author": body.author, "export_hash": hash,
        "content": content,
    });
    Ok(Json(doc))
}

/// Pull data from DB and assemble JSON report content.
fn build_content(
    db: &rusqlite::Connection,
    sections: &[String],
    notes: Option<&str>,
    now: i64,
) -> Result<serde_json::Value, StatusCode> {
    let mut content = serde_json::json!({
        "generated_at": now,
        "notes": notes,
        "sections": {},
    });
    let obj = content["sections"].as_object_mut().unwrap();

    for section in sections {
        match section.as_str() {
            "summary" => {
                let emitter_count: i64 = db.query_row("SELECT COUNT(*) FROM emitters", [], |r| r.get(0)).unwrap_or(0);
                let unknown: i64 = db.query_row("SELECT COUNT(*) FROM emitters WHERE status='UNKNOWN' OR status='NEW'", [], |r| r.get(0)).unwrap_or(0);
                let anomaly_count: i64 = db.query_row("SELECT COUNT(*) FROM anomalies WHERE acknowledged=0", [], |r| r.get(0)).unwrap_or(0);
                let drone_count: i64 = db.query_row("SELECT COUNT(*) FROM drone_detections WHERE detected_at > ?1", rusqlite::params![now - 86400], |r| r.get(0)).unwrap_or(0);
                obj.insert("summary".into(), serde_json::json!({
                    "emitter_count": emitter_count,
                    "unknown_emitters": unknown,
                    "unacknowledged_anomalies": anomaly_count,
                    "drone_detections_24h": drone_count,
                }));
            }
            "emitters" => {
                let mut stmt = db.prepare(
                    "SELECT freq_mhz, emitter_type, id_match, confidence, status, first_seen, last_seen FROM emitters ORDER BY last_seen DESC LIMIT 100"
                ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let rows: Vec<serde_json::Value> = stmt.query_map([], |r| Ok(serde_json::json!({
                    "freq_mhz": r.get::<_,f64>(0)?, "emitter_type": r.get::<_,Option<String>>(1)?,
                    "id_match": r.get::<_,Option<String>>(2)?, "confidence": r.get::<_,f64>(3)?,
                    "status": r.get::<_,String>(4)?, "first_seen": r.get::<_,i64>(5)?, "last_seen": r.get::<_,i64>(6)?,
                }))).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok()).collect();
                obj.insert("emitters".into(), serde_json::Value::Array(rows));
            }
            "anomalies" => {
                let mut stmt = db.prepare(
                    "SELECT freq_mhz, kind, delta_db, z_score, severity, detected_at FROM anomalies ORDER BY detected_at DESC LIMIT 100"
                ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let rows: Vec<serde_json::Value> = stmt.query_map([], |r| Ok(serde_json::json!({
                    "freq_mhz": r.get::<_,f64>(0)?, "kind": r.get::<_,String>(1)?,
                    "delta_db": r.get::<_,Option<f64>>(2)?, "z_score": r.get::<_,Option<f64>>(3)?,
                    "severity": r.get::<_,String>(4)?, "detected_at": r.get::<_,i64>(5)?,
                }))).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok()).collect();
                obj.insert("anomalies".into(), serde_json::Value::Array(rows));
            }
            "drones" => {
                let mut stmt = db.prepare(
                    "SELECT manufacturer, model, detection_method, confidence, signal_dbm, detected_at FROM drone_detections ORDER BY detected_at DESC LIMIT 50"
                ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let rows: Vec<serde_json::Value> = stmt.query_map([], |r| Ok(serde_json::json!({
                    "manufacturer": r.get::<_,Option<String>>(0)?, "model": r.get::<_,Option<String>>(1)?,
                    "method": r.get::<_,String>(2)?, "confidence": r.get::<_,Option<f64>>(3)?,
                    "signal_dbm": r.get::<_,Option<f64>>(4)?, "detected_at": r.get::<_,i64>(5)?,
                }))).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok()).collect();
                obj.insert("drones".into(), serde_json::Value::Array(rows));
            }
            "recommend" => {
                // Simple rule-based recommendations
                let unknown: i64 = db.query_row("SELECT COUNT(*) FROM emitters WHERE status='UNKNOWN' OR status='NEW'", [], |r| r.get(0)).unwrap_or(0);
                let mut recs = Vec::new();
                if unknown > 0 { recs.push(format!("Investigate {unknown} unknown/new emitters in catalog")); }
                let crit: i64 = db.query_row("SELECT COUNT(*) FROM anomalies WHERE severity='CRITICAL' AND acknowledged=0", [], |r| r.get(0)).unwrap_or(0);
                if crit > 0 { recs.push(format!("Acknowledge and investigate {crit} CRITICAL anomalies")); }
                let drones: i64 = db.query_row("SELECT COUNT(DISTINCT track_id) FROM drone_detections WHERE detected_at > ?1", rusqlite::params![now - 86400], |r| r.get(0)).unwrap_or(0);
                if drones > 0 { recs.push(format!("Review {drones} drone tracks from last 24h for intent/authorization")); }
                obj.insert("recommendations".into(), serde_json::Value::Array(recs.into_iter().map(serde_json::Value::String).collect()));
            }
            _ => {}
        }
    }

    Ok(content)
}

/// Produce a hex-like fingerprint of the content for chain-of-custody.
fn simple_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}
