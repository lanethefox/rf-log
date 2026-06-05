use axum::{extract::{State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct DroneDetectionRow {
    pub id: i64,
    pub detected_at: i64,
    pub detection_method: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f64>,
    pub signal_dbm: Option<f64>,
    pub freq_mhz: Option<f64>,
    pub sensor_id: String,
    pub track_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DroneTrack {
    pub id: i64,
    pub first_seen: i64,
    pub last_seen: i64,
    pub detection_methods: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub whitelisted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteIdRow {
    pub id: i64,
    pub received_at: i64,
    pub ua_type: Option<String>,
    pub serial_number: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub altitude_m: Option<f64>,
    pub speed_ms: Option<f64>,
    pub heading_deg: Option<f64>,
    pub operator_lat: Option<f64>,
    pub operator_lon: Option<f64>,
    pub sensor_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignatureRow {
    pub id: i64,
    pub manufacturer: String,
    pub model: String,
    pub freq_ranges_json: String,
    pub bandwidth_mhz: Option<f64>,
    pub notes: Option<String>,
    pub builtin: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WhitelistEntry {
    pub id: i64,
    pub serial_number: String,
    pub owner: Option<String>,
    pub purpose: Option<String>,
    pub added_at: i64,
}

#[derive(Deserialize)]
pub struct AddWhitelist {
    pub serial_number: String,
    pub owner: Option<String>,
    pub purpose: Option<String>,
    pub notes: Option<String>,
}

pub async fn list_detections(State(state): State<AppState>) -> Result<Json<Vec<DroneDetectionRow>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, detected_at, detection_method, manufacturer, model, confidence, signal_dbm, freq_mhz, sensor_id, track_id FROM drone_detections ORDER BY detected_at DESC LIMIT 500"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt.query_map([], |r| Ok(DroneDetectionRow {
        id: r.get(0)?, detected_at: r.get(1)?, detection_method: r.get(2)?,
        manufacturer: r.get(3)?, model: r.get(4)?, confidence: r.get(5)?,
        signal_dbm: r.get(6)?, freq_mhz: r.get(7)?, sensor_id: r.get(8)?, track_id: r.get(9)?,
    })).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.filter_map(|r| r.ok()).collect();
    Ok(Json(rows))
}

pub async fn list_tracks(State(state): State<AppState>) -> Result<Json<Vec<DroneTrack>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, first_seen, last_seen, detection_methods, manufacturer, model, serial_number, whitelisted FROM drone_tracks ORDER BY last_seen DESC"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt.query_map([], |r| Ok(DroneTrack {
        id: r.get(0)?, first_seen: r.get(1)?, last_seen: r.get(2)?,
        detection_methods: r.get(3)?, manufacturer: r.get(4)?, model: r.get(5)?,
        serial_number: r.get(6)?, whitelisted: r.get::<_, i64>(7)? != 0,
    })).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.filter_map(|r| r.ok()).collect();
    Ok(Json(rows))
}

pub async fn list_remote_id(State(state): State<AppState>) -> Result<Json<Vec<RemoteIdRow>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, received_at, ua_type, serial_number, lat, lon, altitude_m, speed_ms, heading_deg, operator_lat, operator_lon, sensor_id FROM drone_remote_id ORDER BY received_at DESC LIMIT 200"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt.query_map([], |r| Ok(RemoteIdRow {
        id: r.get(0)?, received_at: r.get(1)?, ua_type: r.get(2)?, serial_number: r.get(3)?,
        lat: r.get(4)?, lon: r.get(5)?, altitude_m: r.get(6)?, speed_ms: r.get(7)?,
        heading_deg: r.get(8)?, operator_lat: r.get(9)?, operator_lon: r.get(10)?, sensor_id: r.get(11)?,
    })).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.filter_map(|r| r.ok()).collect();
    Ok(Json(rows))
}

pub async fn list_signatures(State(state): State<AppState>) -> Result<Json<Vec<SignatureRow>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, manufacturer, model, freq_ranges_json, bandwidth_mhz, notes, builtin FROM drone_signatures ORDER BY manufacturer, model"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt.query_map([], |r| Ok(SignatureRow {
        id: r.get(0)?, manufacturer: r.get(1)?, model: r.get(2)?, freq_ranges_json: r.get(3)?,
        bandwidth_mhz: r.get(4)?, notes: r.get(5)?, builtin: r.get::<_, i64>(6)? != 0,
    })).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.filter_map(|r| r.ok()).collect();
    Ok(Json(rows))
}

pub async fn list_whitelist(State(state): State<AppState>) -> Result<Json<Vec<WhitelistEntry>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, serial_number, owner, purpose, added_at FROM drone_whitelist ORDER BY added_at DESC"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt.query_map([], |r| Ok(WhitelistEntry {
        id: r.get(0)?, serial_number: r.get(1)?, owner: r.get(2)?,
        purpose: r.get(3)?, added_at: r.get(4)?,
    })).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.filter_map(|r| r.ok()).collect();
    Ok(Json(rows))
}

pub async fn add_whitelist(
    State(state): State<AppState>,
    Json(body): Json<AddWhitelist>,
) -> Result<StatusCode, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT OR IGNORE INTO drone_whitelist (serial_number, owner, purpose, notes, added_at) VALUES (?1,?2,?3,?4,?5)",
        rusqlite::params![body.serial_number, body.owner, body.purpose, body.notes, now],
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}
