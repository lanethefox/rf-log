use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use crate::AppState;

#[derive(Serialize)]
pub struct HarmonicGroupRow {
    pub id: i64,
    pub detected_at: i64,
    pub fundamental_mhz: f64,
    pub harmonics: Vec<f64>,
    pub source_hypothesis: Option<String>,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<HarmonicGroupRow>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, detected_at, fundamental_mhz, harmonics_json, source_hypothesis \
         FROM harmonic_groups ORDER BY detected_at DESC LIMIT 100"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows: Vec<HarmonicGroupRow> = stmt.query_map([], |r| {
        let harmonics_json: String = r.get(3)?;
        let harmonics: Vec<f64> = serde_json::from_str(&harmonics_json).unwrap_or_default();
        Ok(HarmonicGroupRow {
            id: r.get(0)?,
            detected_at: r.get(1)?,
            fundamental_mhz: r.get(2)?,
            harmonics,
            source_hypothesis: r.get(4)?,
        })
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();
    Ok(Json(rows))
}
