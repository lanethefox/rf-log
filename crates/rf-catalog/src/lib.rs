//! RF-LOG v2 catalog — SQLite source of truth for the survey.
//!
//! Schema v1 (P0): `missions`, `sensors`, `detections`. WAL + foreign keys; schema
//! version tracked via `PRAGMA user_version`. The emitter/fingerprint/classification
//! tables arrive in later phases. The connection is guarded by a mutex (survey-rate
//! writes are low-contention); WAL still allows readers alongside the writer.

use rf_types::{Band, Detection, MissionId, MissionPhase, SensorId};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;
use thiserror::Error;

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

type Result<T> = std::result::Result<T, CatalogError>;

/// A persisted mission row.
#[derive(Debug, Clone)]
pub struct MissionRow {
    pub id: MissionId,
    pub name: String,
    pub phase: MissionPhase,
    pub bands: Vec<Band>,
    pub created_ns: i64,
}

pub struct Catalog {
    conn: Mutex<Connection>,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE missions (
                    id         INTEGER PRIMARY KEY,
                    name       TEXT NOT NULL,
                    phase      TEXT NOT NULL,
                    bands_json TEXT NOT NULL,
                    created_ns INTEGER NOT NULL
                 );
                 CREATE TABLE sensors (
                    mission_id INTEGER NOT NULL REFERENCES missions(id),
                    sensor_id  INTEGER NOT NULL,
                    label      TEXT,
                    PRIMARY KEY (mission_id, sensor_id)
                 );
                 CREATE TABLE detections (
                    id             INTEGER PRIMARY KEY,
                    mission_id     INTEGER NOT NULL REFERENCES missions(id),
                    t_unix_ns      INTEGER NOT NULL,
                    center_hz      REAL NOT NULL,
                    bandwidth_hz   REAL NOT NULL,
                    power_dbfs     REAL NOT NULL,
                    snr_db         REAL NOT NULL,
                    tile_center_hz REAL NOT NULL,
                    sensor_id      INTEGER NOT NULL
                 );
                 CREATE INDEX idx_det_mission_t ON detections(mission_id, t_unix_ns);",
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_mission(&self, name: &str, bands: &[Band], created_ns: i64) -> Result<MissionId> {
        let bands_json = serde_json::to_string(bands)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO missions (name, phase, bands_json, created_ns) VALUES (?1, ?2, ?3, ?4)",
            params![
                name,
                phase_str(MissionPhase::Created),
                bands_json,
                created_ns
            ],
        )?;
        Ok(MissionId(conn.last_insert_rowid()))
    }

    pub fn set_mission_phase(&self, id: MissionId, phase: MissionPhase) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE missions SET phase = ?1 WHERE id = ?2",
            params![phase_str(phase), id.0],
        )?;
        Ok(())
    }

    pub fn get_mission(&self, id: MissionId) -> Result<Option<MissionRow>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT id, name, phase, bands_json, created_ns FROM missions WHERE id = ?1",
                params![id.0],
                map_mission,
            )
            .optional()?)
    }

    pub fn list_missions(&self) -> Result<Vec<MissionRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, phase, bands_json, created_ns FROM missions ORDER BY created_ns DESC",
        )?;
        let rows = stmt.query_map([], map_mission)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn upsert_sensor(&self, mission: MissionId, sensor: SensorId, label: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sensors (mission_id, sensor_id, label) VALUES (?1, ?2, ?3)
             ON CONFLICT(mission_id, sensor_id) DO UPDATE SET label = excluded.label",
            params![mission.0, sensor.0, label],
        )?;
        Ok(())
    }

    pub fn insert_detection(&self, mission: MissionId, d: &Detection) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO detections
               (mission_id, t_unix_ns, center_hz, bandwidth_hz, power_dbfs, snr_db, tile_center_hz, sensor_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                mission.0,
                d.t_unix_ns,
                d.center_hz,
                d.bandwidth_hz,
                d.power_dbfs,
                d.snr_db,
                d.tile_center_hz,
                d.sensor.0
            ],
        )?;
        Ok(())
    }

    pub fn list_detections(&self, mission: MissionId, limit: usize) -> Result<Vec<Detection>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t_unix_ns, center_hz, bandwidth_hz, power_dbfs, snr_db, tile_center_hz, sensor_id
             FROM detections WHERE mission_id = ?1 ORDER BY t_unix_ns DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![mission.0, limit as i64], |r| {
            Ok(Detection {
                t_unix_ns: r.get(0)?,
                center_hz: r.get(1)?,
                bandwidth_hz: r.get(2)?,
                power_dbfs: r.get(3)?,
                snr_db: r.get(4)?,
                tile_center_hz: r.get(5)?,
                sensor: SensorId(r.get::<_, i64>(6)? as u32),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn detection_count(&self, mission: MissionId) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM detections WHERE mission_id = ?1",
            params![mission.0],
            |r| r.get(0),
        )?)
    }
}

fn map_mission(r: &rusqlite::Row) -> rusqlite::Result<MissionRow> {
    let bands_json: String = r.get(3)?;
    let phase_text: String = r.get(2)?;
    let bands = serde_json::from_str(&bands_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(MissionRow {
        id: MissionId(r.get(0)?),
        name: r.get(1)?,
        phase: parse_phase(&phase_text),
        bands,
        created_ns: r.get(4)?,
    })
}

fn phase_str(p: MissionPhase) -> &'static str {
    match p {
        MissionPhase::Created => "Created",
        MissionPhase::Running => "Running",
        MissionPhase::Paused => "Paused",
        MissionPhase::Stopped => "Stopped",
        MissionPhase::Complete => "Complete",
    }
}

fn parse_phase(s: &str) -> MissionPhase {
    match s {
        "Running" => MissionPhase::Running,
        "Paused" => MissionPhase::Paused,
        "Stopped" => MissionPhase::Stopped,
        "Complete" => MissionPhase::Complete,
        _ => MissionPhase::Created,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(center: f64, t: i64) -> Detection {
        Detection {
            center_hz: center,
            bandwidth_hz: 12.5e3,
            power_dbfs: -40.0,
            snr_db: 20.0,
            t_unix_ns: t,
            tile_center_hz: 162.0e6,
            sensor: SensorId(2),
        }
    }

    #[test]
    fn mission_lifecycle_and_detections_round_trip() {
        let cat = Catalog::open_in_memory().unwrap();
        let bands = vec![Band {
            name: "WX".into(),
            low_hz: 162e6,
            high_hz: 163e6,
        }];
        let id = cat.create_mission("test", &bands, 1000).unwrap();

        // created phase + bands persisted
        let m = cat.get_mission(id).unwrap().unwrap();
        assert_eq!(m.name, "test");
        assert!(matches!(m.phase, MissionPhase::Created));
        assert_eq!(m.bands.len(), 1);

        // phase transition
        cat.set_mission_phase(id, MissionPhase::Running).unwrap();
        assert!(matches!(
            cat.get_mission(id).unwrap().unwrap().phase,
            MissionPhase::Running
        ));

        // detections persist and read back newest-first
        cat.insert_detection(id, &det(162.40e6, 10)).unwrap();
        cat.insert_detection(id, &det(162.55e6, 20)).unwrap();
        assert_eq!(cat.detection_count(id).unwrap(), 2);
        let list = cat.list_detections(id, 10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].t_unix_ns, 20); // newest first
        assert_eq!(list[0].center_hz, 162.55e6);

        // sensors upsert
        cat.upsert_sensor(id, SensorId(2), "sim-0").unwrap();
        cat.upsert_sensor(id, SensorId(2), "sim-0b").unwrap(); // update, no dup

        assert_eq!(cat.list_missions().unwrap().len(), 1);
    }
}
