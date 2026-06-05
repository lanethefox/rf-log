use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use crate::AppState;

#[derive(Serialize)]
pub struct PdwRow {
    pub id: i64,
    pub emitter_id: Option<i64>,
    pub toa: f64,
    pub pw_us: f64,
    pub freq_mhz: f64,
    pub amplitude_dbfs: Option<f64>,
    pub pri_us: Option<f64>,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<PdwRow>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT id, emitter_id, toa, pw_us, freq_mhz, amplitude_dbfs, pri_us \
         FROM pdw_log ORDER BY toa DESC LIMIT 500"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows: Vec<PdwRow> = stmt.query_map([], |r| Ok(PdwRow {
        id: r.get(0)?,
        emitter_id: r.get(1)?,
        toa: r.get(2)?,
        pw_us: r.get(3)?,
        freq_mhz: r.get(4)?,
        amplitude_dbfs: r.get(5)?,
        pri_us: r.get(6)?,
    }))
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();
    Ok(Json(rows))
}

#[derive(Serialize)]
pub struct PriStats {
    pub freq_mhz: f64,
    pub count: i64,
    pub mean_pri_us: f64,
    pub min_pri_us: f64,
    pub max_pri_us: f64,
    pub pattern: String,
}

/// Summarize PRI statistics per frequency bucket.
pub async fn pri_stats(State(state): State<AppState>) -> Result<Json<Vec<PriStats>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT ROUND(freq_mhz, 1) as f, COUNT(*) as cnt, \
               AVG(pri_us) as avg_pri, MIN(pri_us) as min_pri, MAX(pri_us) as max_pri \
         FROM pdw_log WHERE pri_us IS NOT NULL \
         GROUP BY ROUND(freq_mhz, 1) \
         ORDER BY cnt DESC LIMIT 50"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows: Vec<PriStats> = stmt.query_map([], |r| {
        let min_pri: f64 = r.get(3)?;
        let max_pri: f64 = r.get(4)?;
        let avg_pri: f64 = r.get(2)?;
        // Simple PRI classification
        let spread = (max_pri - min_pri) / avg_pri;
        let pattern = if spread < 0.05 {
            "Stable"
        } else if spread < 0.20 {
            "Stagger"
        } else {
            "Jitter"
        }.to_string();
        Ok(PriStats {
            freq_mhz: r.get(0)?,
            count: r.get(1)?,
            mean_pri_us: avg_pri,
            min_pri_us: min_pri,
            max_pri_us: max_pri,
            pattern,
        })
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();
    Ok(Json(rows))
}
