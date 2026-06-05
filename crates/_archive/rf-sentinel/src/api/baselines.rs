use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use rf_masint::baseline::{BaselineAccumulator, BinStats};
use crate::{AppState, CaptureSession};

#[derive(Debug, Serialize, Deserialize)]
pub struct BaselineSummary {
    pub id: i64,
    pub name: String,
    pub location: Option<String>,
    pub captured_at: i64,
    pub freq_start_mhz: f64,
    pub freq_end_mhz: f64,
    pub bin_count: i64,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<BaselineSummary>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db
        .prepare("SELECT id, name, location, captured_at, freq_start_mhz, freq_end_mhz, bin_count FROM baselines ORDER BY captured_at DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt
        .query_map([], |r| Ok(BaselineSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            location: r.get(2)?,
            captured_at: r.get(3)?,
            freq_start_mhz: r.get(4)?,
            freq_end_mhz: r.get(5)?,
            bin_count: r.get(6)?,
        }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<BaselineSummary>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.query_row(
        "SELECT id, name, location, captured_at, freq_start_mhz, freq_end_mhz, bin_count FROM baselines WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(BaselineSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            location: r.get(2)?,
            captured_at: r.get(3)?,
            freq_start_mhz: r.get(4)?,
            freq_end_mhz: r.get(5)?,
            bin_count: r.get(6)?,
        }),
    )
    .map(Json)
    .map_err(|_| StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
pub struct CreateBaseline {
    pub name: String,
    pub location: Option<String>,
    pub freq_start_mhz: f64,
    pub freq_end_mhz: f64,
    pub bin_count: i64,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateBaseline>,
) -> Result<Json<BaselineSummary>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO baselines (name, location, captured_at, freq_start_mhz, freq_end_mhz, bin_count, lat, lon) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![body.name, body.location, now, body.freq_start_mhz, body.freq_end_mhz, body.bin_count, body.lat, body.lon],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id = db.last_insert_rowid();
    db.query_row(
        "SELECT id, name, location, captured_at, freq_start_mhz, freq_end_mhz, bin_count FROM baselines WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(BaselineSummary {
            id: r.get(0)?, name: r.get(1)?, location: r.get(2)?,
            captured_at: r.get(3)?, freq_start_mhz: r.get(4)?,
            freq_end_mhz: r.get(5)?, bin_count: r.get(6)?,
        }),
    ).map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// --- Capture endpoints ---

#[derive(Deserialize)]
pub struct StartCaptureBody {
    pub name: String,
    pub location: Option<String>,
    /// Default: current sim range (430–450 MHz) with 512 bins
    pub freq_start_mhz: Option<f64>,
    pub freq_end_mhz: Option<f64>,
    pub bin_count: Option<usize>,
}

pub async fn capture_start(
    State(state): State<AppState>,
    Json(body): Json<StartCaptureBody>,
) -> StatusCode {
    let freq_start = body.freq_start_mhz.unwrap_or(430.0);
    let freq_end   = body.freq_end_mhz.unwrap_or(450.0);
    let bin_count  = body.bin_count.unwrap_or(512);
    let step = (freq_end - freq_start) / bin_count as f64;

    let session = CaptureSession {
        name: body.name,
        location: body.location,
        accumulator: BaselineAccumulator::new(bin_count, freq_start, step),
        started_at: chrono::Utc::now().timestamp(),
        freq_start_mhz: freq_start,
        freq_end_mhz: freq_end,
    };

    match state.capture.lock() {
        Ok(mut cap) => { *cap = Some(session); StatusCode::OK }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Serialize)]
pub struct CaptureStatus {
    pub active: bool,
    pub name: Option<String>,
    pub started_at: Option<i64>,
    pub sample_count: Option<u64>,
    pub elapsed_secs: Option<i64>,
}

pub async fn capture_status(State(state): State<AppState>) -> Json<CaptureStatus> {
    let cap = state.capture.lock();
    match cap {
        Err(_) => Json(CaptureStatus { active: false, name: None, started_at: None, sample_count: None, elapsed_secs: None }),
        Ok(guard) => match guard.as_ref() {
            None => Json(CaptureStatus { active: false, name: None, started_at: None, sample_count: None, elapsed_secs: None }),
            Some(s) => Json(CaptureStatus {
                active: true,
                name: Some(s.name.clone()),
                started_at: Some(s.started_at),
                sample_count: Some(s.accumulator.sample_count()),
                elapsed_secs: Some(chrono::Utc::now().timestamp() - s.started_at),
            }),
        },
    }
}

pub async fn capture_stop(
    State(state): State<AppState>,
) -> Result<Json<BaselineSummary>, StatusCode> {
    // Take the session
    let session = {
        let mut cap = state.capture.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        cap.take().ok_or(StatusCode::CONFLICT)?
    };

    let bins = session.accumulator.finalize();
    let bin_count = bins.len() as i64;

    let (baseline_id, summary) = {
        let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db.execute(
            "INSERT INTO baselines (name, location, captured_at, freq_start_mhz, freq_end_mhz, bin_count) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![session.name, session.location, session.started_at, session.freq_start_mhz, session.freq_end_mhz, bin_count],
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let id = db.last_insert_rowid();

        // Bulk-insert bins
        for (i, bin) in bins.iter().enumerate() {
            db.execute(
                "INSERT INTO baseline_bins (baseline_id, bin_index, freq_mhz, mean, std_dev, min_val, max_val, sample_count) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![id, i as i64, bin.freq_mhz, bin.mean as f64, bin.std_dev as f64, bin.min as f64, bin.max as f64, bin.sample_count as i64],
            ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }

        let s = BaselineSummary {
            id,
            name: session.name,
            location: session.location,
            captured_at: session.started_at,
            freq_start_mhz: session.freq_start_mhz,
            freq_end_mhz: session.freq_end_mhz,
            bin_count,
        };
        (id, s)
    };

    // Auto-activate this baseline for anomaly detection
    if let Ok(mut active) = state.active_baseline.lock() {
        *active = Some(bins);
    }

    tracing::info!("Baseline {} '{}' saved with {} bins", baseline_id, summary.name, bin_count);
    Ok(Json(summary))
}

// --- Bins + activate ---

pub async fn get_bins(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<BinStats>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare(
        "SELECT freq_mhz, mean, std_dev, min_val, max_val, sample_count FROM baseline_bins WHERE baseline_id = ?1 ORDER BY bin_index"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bins: Vec<BinStats> = stmt.query_map(rusqlite::params![id], |r| Ok(BinStats {
        freq_mhz: r.get(0)?,
        mean: r.get::<_, f64>(1)? as f32,
        std_dev: r.get::<_, f64>(2)? as f32,
        min: r.get::<_, f64>(3)? as f32,
        max: r.get::<_, f64>(4)? as f32,
        sample_count: r.get::<_, i64>(5)? as u64,
    }))
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();

    if bins.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(bins))
}

pub async fn activate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let bins: Vec<BinStats> = {
        let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut stmt = db.prepare(
            "SELECT freq_mhz, mean, std_dev, min_val, max_val, sample_count FROM baseline_bins WHERE baseline_id = ?1 ORDER BY bin_index"
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map(rusqlite::params![id], |r| Ok(BinStats {
            freq_mhz: r.get(0)?,
            mean: r.get::<_, f64>(1)? as f32,
            std_dev: r.get::<_, f64>(2)? as f32,
            min: r.get::<_, f64>(3)? as f32,
            max: r.get::<_, f64>(4)? as f32,
            sample_count: r.get::<_, i64>(5)? as u64,
        }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    };

    if bins.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }
    if let Ok(mut active) = state.active_baseline.lock() {
        *active = Some(bins);
    }
    tracing::info!("Activated baseline {id} for anomaly detection");
    Ok(StatusCode::OK)
}
