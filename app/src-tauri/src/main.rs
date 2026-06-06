// Hide the extra console window on Windows release builds; harmless elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rf_bus::Bus;
use rf_catalog::Catalog;
use rf_mission::{MissionConfig, MissionManager, spawn_detection_writer};
use rf_types::{Band, BusEvent, Detection, MissionId};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use tauri::{Emitter, Manager};

struct AppState {
    mgr: Arc<MissionManager>,
}

#[derive(Serialize)]
struct MissionDto {
    id: i64,
    name: String,
    phase: String,
    created_ns: i64,
    bands: Vec<Band>,
}

#[derive(Serialize)]
struct StatusDto {
    active_mission: Option<i64>,
}

#[tauri::command]
fn create_mission(
    state: tauri::State<AppState>,
    name: String,
    bands: Vec<Band>,
) -> Result<i64, String> {
    state.mgr.create_mission(&name, bands).map(|m| m.0)
}

#[tauri::command]
fn start_mission(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: i64,
) -> Result<(), String> {
    // Opening real SDRs can take several seconds — run it off the command thread so the
    // UI stays responsive. Progress/readiness is reported via sensor_info/sensor_status
    // events; failures come back as a mission_error event.
    let mgr = state.mgr.clone();
    std::thread::spawn(move || {
        if let Err(e) = mgr.start(MissionId(id)) {
            let _ = app.emit("mission_error", e);
        }
    });
    Ok(())
}

#[tauri::command]
fn stop_mission(state: tauri::State<AppState>) -> Result<(), String> {
    state.mgr.stop()
}

#[tauri::command]
fn list_missions(state: tauri::State<AppState>) -> Result<Vec<MissionDto>, String> {
    let rows = state
        .mgr
        .catalog()
        .list_missions()
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|m| MissionDto {
            id: m.id.0,
            name: m.name,
            phase: format!("{:?}", m.phase),
            created_ns: m.created_ns,
            bands: m.bands,
        })
        .collect())
}

#[tauri::command]
fn list_detections(
    state: tauri::State<AppState>,
    id: i64,
    limit: usize,
) -> Result<Vec<Detection>, String> {
    state
        .mgr
        .catalog()
        .list_detections(MissionId(id), limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> StatusDto {
    StatusDto {
        active_mission: state.mgr.active_mission().map(|m| m.0),
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Catalog lives under the app data dir.
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&dir).ok();
            let catalog = Arc::new(Catalog::open(&dir.join("rf-log.db")).expect("open catalog"));

            let (bus, det_rx) = rf_bus::channel(512);
            let active = Arc::new(AtomicI64::new(-1));
            spawn_detection_writer(catalog.clone(), active.clone(), det_rx);

            let mgr = Arc::new(MissionManager::new(
                catalog,
                bus.clone(),
                active,
                MissionConfig::default(),
            ));
            app.manage(AppState { mgr });

            spawn_event_bridge(app.handle().clone(), bus);

            eprintln!("[rf-log] setup complete; window should load the embedded UI");
            #[cfg(debug_assertions)]
            if let Some(w) = app.get_webview_window("main") {
                w.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_mission,
            start_mission,
            stop_mission,
            list_missions,
            list_detections,
            get_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running RF-LOG");
}

/// Forward bus events to the webview. PSD telemetry is throttled (~25 Hz) so the UI
/// isn't flooded; detections and state changes pass through immediately.
fn spawn_event_bridge(app: tauri::AppHandle, bus: Bus) {
    tauri::async_runtime::spawn(async move {
        let mut rx = bus.subscribe();
        let mut last_psd = std::time::Instant::now();
        loop {
            match rx.recv().await {
                Ok(BusEvent::Psd(f)) => {
                    if last_psd.elapsed().as_millis() >= 40 {
                        last_psd = std::time::Instant::now();
                        let _ = app.emit("psd", f);
                    }
                }
                Ok(BusEvent::Detection(d)) => {
                    let _ = app.emit("detection", d);
                }
                Ok(BusEvent::SensorInfo { id, label }) => {
                    let _ = app.emit("sensor_info", (id, label));
                }
                Ok(BusEvent::SensorStatus { id, state }) => {
                    let _ = app.emit("sensor_status", (id, state));
                }
                Ok(BusEvent::MissionState { id, phase }) => {
                    let _ = app.emit("mission_state", (id, phase));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}
