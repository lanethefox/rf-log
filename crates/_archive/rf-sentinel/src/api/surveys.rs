use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Survey {
    pub id: i64,
    pub name: String,
    pub location: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub operator: Option<String>,
    pub status: String,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Survey>>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db
        .prepare("SELECT id, name, location, started_at, ended_at, operator, status FROM surveys ORDER BY started_at DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt
        .query_map([], |r| Ok(Survey {
            id: r.get(0)?,
            name: r.get(1)?,
            location: r.get(2)?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            operator: r.get(5)?,
            status: r.get(6)?,
        }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(rows))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Survey>, StatusCode> {
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.query_row(
        "SELECT id, name, location, started_at, ended_at, operator, status FROM surveys WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(Survey {
            id: r.get(0)?,
            name: r.get(1)?,
            location: r.get(2)?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            operator: r.get(5)?,
            status: r.get(6)?,
        }),
    )
    .map(Json)
    .map_err(|_| StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
pub struct CreateSurvey {
    pub name: String,
    pub location: Option<String>,
    pub operator: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateSurvey>,
) -> Result<Json<Survey>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO surveys (name, location, started_at, operator, status) VALUES (?1,?2,?3,?4,'IN_PROGRESS')",
        rusqlite::params![body.name, body.location, now, body.operator],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id = db.last_insert_rowid();
    db.query_row(
        "SELECT id, name, location, started_at, ended_at, operator, status FROM surveys WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok(Survey {
            id: r.get(0)?, name: r.get(1)?, location: r.get(2)?,
            started_at: r.get(3)?, ended_at: r.get(4)?,
            operator: r.get(5)?, status: r.get(6)?,
        }),
    ).map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
