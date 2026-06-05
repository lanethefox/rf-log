use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Emitter {
    pub id: i64,
    pub freq_mhz: f64,
    pub emitter_type: Option<String>,
    pub id_match: Option<String>,
    pub confidence: f64,
    pub first_seen: i64,
    pub last_seen: i64,
    pub status: String,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEmitter {
    pub freq_mhz: f64,
    pub emitter_type: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Emitter>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db
        .prepare("SELECT id, freq_mhz, emitter_type, id_match, confidence, first_seen, last_seen, status, notes FROM emitters ORDER BY last_seen DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Emitter {
                id: r.get(0)?,
                freq_mhz: r.get(1)?,
                emitter_type: r.get(2)?,
                id_match: r.get(3)?,
                confidence: r.get(4)?,
                first_seen: r.get(5)?,
                last_seen: r.get(6)?,
                status: r.get(7)?,
                notes: r.get(8)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Emitter>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.query_row(
        "SELECT id, freq_mhz, emitter_type, id_match, confidence, first_seen, last_seen, status, notes FROM emitters WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(Emitter {
            id: r.get(0)?,
            freq_mhz: r.get(1)?,
            emitter_type: r.get(2)?,
            id_match: r.get(3)?,
            confidence: r.get(4)?,
            first_seen: r.get(5)?,
            last_seen: r.get(6)?,
            status: r.get(7)?,
            notes: r.get(8)?,
        }),
    )
    .map(Json)
    .map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateEmitter>,
) -> Result<Json<Emitter>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO emitters (freq_mhz, emitter_type, first_seen, last_seen, status, notes) VALUES (?1, ?2, ?3, ?3, 'NEW', ?4)",
        rusqlite::params![body.freq_mhz, body.emitter_type, now, body.notes],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id = db.last_insert_rowid();
    db.query_row(
        "SELECT id, freq_mhz, emitter_type, id_match, confidence, first_seen, last_seen, status, notes FROM emitters WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(Emitter {
            id: r.get(0)?, freq_mhz: r.get(1)?, emitter_type: r.get(2)?,
            id_match: r.get(3)?, confidence: r.get(4)?, first_seen: r.get(5)?,
            last_seen: r.get(6)?, status: r.get(7)?, notes: r.get(8)?,
        }),
    ).map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub struct UpdateEmitter {
    pub status: Option<String>,
    pub notes: Option<String>,
    pub emitter_type: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateEmitter>,
) -> Result<Json<Emitter>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(ref s) = body.status {
        let _ = db.execute("UPDATE emitters SET status = ?1 WHERE id = ?2", rusqlite::params![s, id]);
    }
    if let Some(ref n) = body.notes {
        let _ = db.execute("UPDATE emitters SET notes = ?1 WHERE id = ?2", rusqlite::params![n, id]);
    }
    if let Some(ref t) = body.emitter_type {
        let _ = db.execute("UPDATE emitters SET emitter_type = ?1 WHERE id = ?2", rusqlite::params![t, id]);
    }
    db.query_row(
        "SELECT id, freq_mhz, emitter_type, id_match, confidence, first_seen, last_seen, status, notes FROM emitters WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(Emitter { id: r.get(0)?, freq_mhz: r.get(1)?, emitter_type: r.get(2)?, id_match: r.get(3)?, confidence: r.get(4)?, first_seen: r.get(5)?, last_seen: r.get(6)?, status: r.get(7)?, notes: r.get(8)? }),
    ).map(Json).map_err(|_| StatusCode::NOT_FOUND)
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute("DELETE FROM emitters WHERE id = ?1", rusqlite::params![id])
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
