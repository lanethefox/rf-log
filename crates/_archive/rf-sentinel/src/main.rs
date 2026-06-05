mod api;
mod ws;
mod db;
mod bridge;

use api::config::SentinelConfig;
use axum::Router;
use bridge::{SentinelBridge, SpectrumFrame};
use rf_masint::baseline::{BaselineAccumulator, BinStats};
use std::sync::{Arc, Mutex};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;

/// In-progress baseline capture session.
pub struct CaptureSession {
    pub name: String,
    pub location: Option<String>,
    pub accumulator: BaselineAccumulator,
    pub started_at: i64,
    pub freq_start_mhz: f64,
    pub freq_end_mhz: f64,
}

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub config: Arc<Mutex<SentinelConfig>>,
    pub bridge: Arc<Mutex<SentinelBridge>>,
    pub event_tx: Arc<tokio::sync::broadcast::Sender<String>>,
    pub capture: Arc<Mutex<Option<CaptureSession>>>,
    pub active_baseline: Arc<Mutex<Option<Vec<BinStats>>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rf_sentinel=info,tower_http=info".into()),
        )
        .init();

    let db_path = std::env::var("SENTINEL_DB").unwrap_or_else(|_| "sentinel.db".into());
    let conn = db::open(&db_path)?;
    info!("Database opened at {db_path}");

    seed_static_data(&conn);

    let (event_tx, _) = tokio::sync::broadcast::channel::<String>(512);
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        config: Arc::new(Mutex::new(SentinelConfig::default())),
        bridge: Arc::new(bridge::SentinelBridge::new()),
        event_tx: Arc::new(event_tx),
        capture: Arc::new(Mutex::new(None)),
        active_baseline: Arc::new(Mutex::new(None)),
    };

    tokio::spawn(sim_spectrum_loop(state.clone()));

    let ui_dir = std::env::var("SENTINEL_UI_DIR").unwrap_or_else(|_| "sentinel-ui/dist".into());
    let serve_dir = ServeDir::new(&ui_dir).append_index_html_on_directories(true);

    let app = Router::new()
        .merge(api::router(state.clone()))
        .merge(ws::router(state.clone()))
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let bind_addr = std::env::var("SENTINEL_BIND").unwrap_or_else(|_| "0.0.0.0:3100".into());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("RF-SENTINEL listening on http://{bind_addr}");
    info!("UI served from {ui_dir}");

    axum::serve(listener, app).await?;
    Ok(())
}

fn seed_static_data(conn: &rusqlite::Connection) {
    // Drone signatures
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM drone_signatures WHERE builtin = 1", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        for sig in rf_masint::drone::builtin_signatures() {
            let ranges_json = serde_json::to_string(&sig.freq_ranges_mhz).unwrap_or_default();
            let _ = conn.execute(
                "INSERT INTO drone_signatures (manufacturer, model, freq_ranges_json, bandwidth_mhz, notes, builtin) VALUES (?1,?2,?3,?4,?5,1)",
                rusqlite::params![sig.manufacturer, sig.model, ranges_json, sig.bandwidth_mhz, sig.notes],
            );
        }
        info!("Seeded {} built-in drone signatures", rf_masint::drone::builtin_signatures().len());
    }
}

// ---------------------------------------------------------------------------
// Simulation loop — drives all analysis pipelines when no real SDR is present
// ---------------------------------------------------------------------------

async fn sim_spectrum_loop(state: AppState) {
    use std::f64::consts::PI;
    use num_complex::Complex32;
    use rf_masint::change_detect::detect_anomalies;
    use rf_masint::harmonics::find_harmonics;
    use rf_masint::fingerprint::extract_fingerprint;
    use rf_elint::reference::{EmitterRef, score_match};

    let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(250));
    let mut t: f64 = 0.0;
    let bin_count = 512usize;
    let freq_start = 430.0_f64;
    let freq_end = 450.0_f64;
    let step = (freq_end - freq_start) / bin_count as f64;
    let noise_floor_dbfs: f32 = -97.0;

    // PDW state — track last pulse TOA to compute PRI
    let mut last_pulse_toa: Option<f64> = None;
    let mut pulse_was_active = false;

    // Tick counters for throttling
    let mut tick_n: u64 = 0;

    // Cache reference library (reload every 5 min)
    let mut ref_library: Vec<EmitterRef> = Vec::new();
    let mut ref_loaded_at: u64 = 0;

    loop {
        tick.tick().await;
        t += 0.25;
        tick_n += 1;

        // ── Build synthetic spectrum ──────────────────────────────────────
        let freqs: Vec<f64> = (0..bin_count).map(|i| freq_start + i as f64 * step).collect();
        let powers_f64: Vec<f64> = freqs.iter().map(|&f| {
            let noise = -95.0 + (rand_f64(f + t) * 4.0 - 2.0);
            // Signal 1: 435.5 MHz — CW, slowly varying
            let sig1 = if (f - 435.5).abs() < 0.1 { -72.0 + (PI * t * 0.3).sin() * 3.0 } else { -999.0 };
            // Signal 2: 442.1 MHz — pulsed (harmonic source sim)
            let sig2 = if (f - 442.1).abs() < 0.05 && (t * 0.5).sin() > 0.3 { -68.0 } else { -999.0 };
            // Signal 3: 436.0 MHz — harmonic of ~218 MHz clock (sim TEMPEST)
            let sig3 = if (f - 436.0).abs() < 0.08 { -84.0 + rand_f64(f + t * 0.1) * 2.0 } else { -999.0 };
            noise.max(sig1).max(sig2).max(sig3)
        }).collect();
        let powers_f32: Vec<f32> = powers_f64.iter().map(|&x| x as f32).collect();

        // ── Update bridge ─────────────────────────────────────────────────
        let frame = SpectrumFrame {
            band: "UHF-SIM".into(),
            freqs: freqs.clone(),
            powers: powers_f64.clone(),
            noise_floor: noise_floor_dbfs as f64,
        };
        if let Ok(mut b) = state.bridge.lock() {
            b.update_spectrum(frame);
        }

        // ── Feed capture accumulator ──────────────────────────────────────
        if let Ok(mut cap) = state.capture.lock() {
            if let Some(ref mut session) = *cap {
                session.accumulator.update(&powers_f32);
            }
        }

        // ── P1.4: PDW generation for pulsed signal ────────────────────────
        // Detect rising edge of the 442.1 MHz pulse train
        let pulse_active = (t * 0.5).sin() > 0.3;
        if pulse_active && !pulse_was_active {
            let pri_us = last_pulse_toa.map(|prev| (t - prev) * 1_000_000.0);
            let pw_us = 1.2 + rand_f64(t * 7.3) * 0.3;
            last_pulse_toa = Some(t);
            if let Ok(db) = state.db.lock() {
                // Find or create the emitter for 442.1 MHz
                let emitter_id: Option<i64> = db.query_row(
                    "SELECT id FROM emitters WHERE ABS(freq_mhz - 442.1) < 0.5 LIMIT 1",
                    [], |r| r.get(0)
                ).ok();
                let _ = db.execute(
                    "INSERT INTO pdw_log (emitter_id, toa, pw_us, freq_mhz, amplitude_dbfs, pri_us) VALUES (?1,?2,?3,?4,?5,?6)",
                    rusqlite::params![emitter_id, t, pw_us, 442.1_f64, -68.0_f64, pri_us],
                );
            }
        }
        pulse_was_active = pulse_active;

        // ── 1 Hz tasks ────────────────────────────────────────────────────
        if tick_n % 4 == 0 {
            let now = chrono::Utc::now().timestamp();

            // Reload reference library cache every 5 min
            if tick_n - ref_loaded_at > 1200 {
                ref_loaded_at = tick_n;
                if let Ok(db) = state.db.lock() {
                    ref_library = load_ref_library(&db);
                }
            }

            // P1.3: Emitter peak detection + upsert
            let peaks = detect_peaks(&powers_f32, &freqs, noise_floor_dbfs + 10.0);
            if !peaks.is_empty() {
                if let Ok(db) = state.db.lock() {
                    upsert_emitters(&db, &peaks, &ref_library, now, &state.event_tx);
                }
            }

            // P1.2: Anomaly detection against active baseline
            let baseline_opt = state.active_baseline.lock().ok().and_then(|g| g.clone());
            if let Some(baseline) = baseline_opt {
                let z_threshold = state.config.lock()
                    .map(|c| c.anomaly_z_threshold as f32)
                    .unwrap_or(3.5);
                let anomalies = detect_anomalies(&powers_f32, &baseline, z_threshold);
                if !anomalies.is_empty() {
                    if let Ok(db) = state.db.lock() {
                        for a in anomalies.iter().take(5) {
                            let kind_str = format!("{:?}", a.kind);
                            let sev = if a.z_score.abs() > 5.0 { "CRITICAL" } else { "WARNING" };
                            let _ = db.execute(
                                "INSERT INTO anomalies (detected_at, freq_mhz, kind, delta_db, z_score, severity) VALUES (?1,?2,?3,?4,?5,?6)",
                                rusqlite::params![now, a.freq_mhz, kind_str, a.delta_db as f64, a.z_score as f64, sev],
                            );
                            let evt = serde_json::json!({ "type":"anomaly","freq_mhz":a.freq_mhz,"kind":kind_str,"delta_db":a.delta_db,"z_score":a.z_score,"severity":sev,"detected_at":now });
                            let _ = state.event_tx.send(evt.to_string());
                        }
                    }
                }
            }
        }

        // ── 10 s tasks ────────────────────────────────────────────────────
        if tick_n % 40 == 0 {
            let now = chrono::Utc::now().timestamp();

            // P1.6: Harmonic analysis from peak list
            let peak_freqs: Vec<f64> = detect_peaks(&powers_f32, &freqs, noise_floor_dbfs + 10.0)
                .into_iter().map(|(f, _)| f).collect();
            if peak_freqs.len() >= 2 {
                let groups = find_harmonics(&peak_freqs, 1.0);
                if let Ok(db) = state.db.lock() {
                    for g in &groups {
                        let harmonics_json = serde_json::to_string(&g.harmonics).unwrap_or_default();
                        // Only insert if not already cataloged at this fundamental (±1 MHz, last 60s)
                        let exists: bool = db.query_row(
                            "SELECT COUNT(*) FROM harmonic_groups WHERE ABS(fundamental_mhz - ?1) < 1.0 AND detected_at > ?2",
                            rusqlite::params![g.fundamental_mhz, now - 60],
                            |r| r.get::<_, i64>(0)
                        ).unwrap_or(0) > 0;
                        if !exists {
                            let _ = db.execute(
                                "INSERT INTO harmonic_groups (detected_at, fundamental_mhz, harmonics_json, source_hypothesis) VALUES (?1,?2,?3,?4)",
                                rusqlite::params![now, g.fundamental_mhz, harmonics_json, g.source_hypothesis],
                            );
                        }
                    }
                }
            }

            // P1.8: RF fingerprint for each known emitter
            if let Ok(db) = state.db.lock() {
                let emitter_ids: Vec<(i64, f64)> = {
                    let mut stmt = db.prepare(
                        "SELECT id, freq_mhz FROM emitters WHERE fingerprint_json IS NULL LIMIT 5"
                    ).unwrap_or_else(|_| db.prepare("SELECT id, freq_mhz FROM emitters LIMIT 0").unwrap());
                    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                for (id, freq_mhz) in emitter_ids {
                    let samples = synth_iq_burst(freq_mhz, t, 256);
                    let fp = extract_fingerprint(&samples, 2_400_000.0);
                    let fp_json = serde_json::to_string(&fp).unwrap_or_default();
                    let _ = db.execute(
                        "UPDATE emitters SET fingerprint_json = ?1 WHERE id = ?2",
                        rusqlite::params![fp_json, id],
                    );
                }
            }
        }

        // ── 60 s tasks ────────────────────────────────────────────────────
        if tick_n % 240 == 0 {
            let now = chrono::Utc::now().timestamp();

            // P1.5: Drone detection injection (30% probability)
            if rand_f64(t * 13.7) > 0.7 {
                let is_dji = rand_f64(t * 3.1) > 0.4;
                let (mfr, model, method, freq) = if is_dji {
                    ("DJI", "Mini 3 Pro", "RfSignature", 2437.0_f64)
                } else {
                    ("Unknown", "", "RfSignature", 5800.0_f64)
                };
                let confidence = 0.55 + rand_f64(t * 2.9) * 0.35;
                let signal_dbm = -68.0 + rand_f64(t * 5.1) * 10.0;

                if let Ok(db) = state.db.lock() {
                    // Find or create a track
                    let track_id: i64 = db.query_row(
                        "SELECT id FROM drone_tracks WHERE manufacturer = ?1 AND model = ?2 AND last_seen > ?3 LIMIT 1",
                        rusqlite::params![mfr, model, now - 120],
                        |r| r.get(0)
                    ).unwrap_or_else(|_| {
                        let _ = db.execute(
                            "INSERT INTO drone_tracks (first_seen, last_seen, detection_methods, peak_signal_dbm, manufacturer, model) VALUES (?1,?1,?2,?3,?4,?5)",
                            rusqlite::params![now, method, signal_dbm, mfr, model],
                        );
                        db.last_insert_rowid()
                    });
                    let _ = db.execute(
                        "UPDATE drone_tracks SET last_seen = ?1, peak_signal_dbm = MAX(peak_signal_dbm, ?2) WHERE id = ?3",
                        rusqlite::params![now, signal_dbm, track_id],
                    );
                    let _ = db.execute(
                        "INSERT INTO drone_detections (detected_at, detection_method, manufacturer, model, confidence, signal_dbm, freq_mhz, track_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                        rusqlite::params![now, method, mfr, model, confidence, signal_dbm, freq, track_id],
                    );
                }

                let evt = serde_json::json!({
                    "type": "drone",
                    "manufacturer": mfr, "model": model,
                    "method": method, "confidence": confidence,
                    "signal_dbm": signal_dbm, "freq_mhz": freq,
                    "detected_at": now,
                });
                let _ = state.event_tx.send(evt.to_string());
                info!("Sim: drone detection — {mfr} {model} @ {freq:.0} MHz");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline helpers
// ---------------------------------------------------------------------------

/// Find local maxima above `threshold_dbfs`.
fn detect_peaks(powers: &[f32], freqs: &[f64], threshold_dbfs: f32) -> Vec<(f64, f32)> {
    let mut peaks = Vec::new();
    for i in 1..powers.len().saturating_sub(1) {
        if powers[i] > threshold_dbfs
            && powers[i] > powers[i - 1]
            && powers[i] > powers[i + 1]
        {
            peaks.push((freqs[i], powers[i]));
        }
    }
    peaks
}

/// Load the emitter reference library from DB.
fn load_ref_library(db: &rusqlite::Connection) -> Vec<rf_elint::reference::EmitterRef> {
    let mut stmt = match db.prepare(
        "SELECT id, name, emitter_type, freq_min_mhz, freq_max_mhz, pri_min_us, pri_max_us, pw_min_us, pw_max_us, notes FROM emitter_reference"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([], |r| Ok(rf_elint::reference::EmitterRef {
        id: r.get(0)?,
        name: r.get(1)?,
        emitter_type: r.get(2)?,
        freq_min_mhz: r.get(3)?,
        freq_max_mhz: r.get(4)?,
        pri_min_us: r.get(5)?,
        pri_max_us: r.get(6)?,
        pw_min_us: r.get(7)?,
        pw_max_us: r.get(8)?,
        notes: r.get(9)?,
    }))
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Upsert emitters from detected peaks. Creates NEW entries or refreshes last_seen.
fn upsert_emitters(
    db: &rusqlite::Connection,
    peaks: &[(f64, f32)],
    ref_library: &[rf_elint::reference::EmitterRef],
    now: i64,
    event_tx: &tokio::sync::broadcast::Sender<String>,
) {
    for &(freq, _power) in peaks {
        // Check existing within ±0.5 MHz
        let existing: Option<(i64, String)> = db.query_row(
            "SELECT id, status FROM emitters WHERE ABS(freq_mhz - ?1) < 0.5 LIMIT 1",
            rusqlite::params![freq],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();

        if let Some((id, status)) = existing {
            let new_status = if status == "GONE" { "UNKNOWN".to_string() } else { status };
            let _ = db.execute(
                "UPDATE emitters SET last_seen = ?1, status = ?2 WHERE id = ?3",
                rusqlite::params![now, new_status, id],
            );
        } else {
            // New emitter — match against reference library
            let best = ref_library.iter()
                .filter_map(|r| rf_elint::reference::score_match(freq, None, None, r))
                .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal));
            let (id_match, emitter_type, confidence) = match best {
                Some(m) => (Some(m.emitter_ref.name), Some(m.emitter_ref.emitter_type), m.confidence),
                None => (None, None, 0.0),
            };
            let res = db.execute(
                "INSERT OR IGNORE INTO emitters (freq_mhz, emitter_type, id_match, confidence, first_seen, last_seen, status) VALUES (?1,?2,?3,?4,?5,?5,'NEW')",
                rusqlite::params![freq, emitter_type, id_match, confidence, now],
            );
            if res.map(|n| n > 0).unwrap_or(false) {
                let evt = serde_json::json!({ "type":"new_emitter","freq_mhz":freq,"detected_at":now });
                let _ = event_tx.send(evt.to_string());
            }
        }
    }
}

/// Generate synthetic IQ burst samples for fingerprinting.
fn synth_iq_burst(freq_mhz: f64, t: f64, n: usize) -> Vec<num_complex::Complex32> {
    let cfo_offset = rand_f64(freq_mhz * 137.3) as f32 * 500.0; // ±500 Hz CFO per emitter
    let iq_skew = 1.0 + rand_f64(freq_mhz * 57.1) as f32 * 0.05;
    (0..n).map(|i| {
        let phase = (i as f32 * 0.1) + cfo_offset * i as f32 / 2_400_000.0;
        let noise_i = (rand_f64(freq_mhz + t + i as f64 * 0.01) as f32 - 0.5) * 0.05;
        let noise_q = (rand_f64(freq_mhz + t + i as f64 * 0.02) as f32 - 0.5) * 0.05;
        num_complex::Complex32::new(
            phase.cos() * iq_skew + noise_i,
            phase.sin() + noise_q,
        )
    }).collect()
}

/// Simple deterministic pseudo-noise (no deps).
fn rand_f64(seed: f64) -> f64 {
    let x = seed.sin() * 43758.5453;
    x - x.floor()
}
