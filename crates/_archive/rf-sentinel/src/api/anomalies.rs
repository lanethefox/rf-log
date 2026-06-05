use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use crate::AppState;

#[derive(Serialize)]
pub struct AnomalyRow {
    pub id: i64,
    pub detected_at: i64,
    pub freq_mhz: f64,
    pub kind: String,
    pub delta_db: Option<f64>,
    pub z_score: Option<f64>,
    pub severity: String,
    pub acknowledged: bool,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<AnomalyRow>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, detected_at, freq_mhz, kind, delta_db, z_score, severity, acknowledged \
         FROM anomalies ORDER BY detected_at DESC LIMIT 200"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows: Vec<AnomalyRow> = stmt.query_map([], |r| Ok(AnomalyRow {
        id: r.get(0)?,
        detected_at: r.get(1)?,
        freq_mhz: r.get(2)?,
        kind: r.get(3)?,
        delta_db: r.get(4)?,
        z_score: r.get(5)?,
        severity: r.get(6)?,
        acknowledged: r.get::<_, i64>(7)? != 0,
    }))
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();
    Ok(Json(rows))
}

pub async fn acknowledge(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> StatusCode {
    match state.db.lock() {
        Ok(db) => {
            let _ = db.execute(
                "UPDATE anomalies SET acknowledged = 1 WHERE id = ?1",
                rusqlite::params![id],
            );
            StatusCode::OK
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
