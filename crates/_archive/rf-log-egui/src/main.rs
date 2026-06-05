#![windows_subsystem = "windows"]

mod alert_engine;
mod app;
mod bridge;
#[allow(dead_code)]
mod commands;
mod config_poller;
mod dsp_bridge;
mod event_ingestion;
mod event_manager;
mod lifecycle_monitor;
mod panels;
mod pool;
mod sigex_bridge;
mod state;
mod theme;
mod views;
mod widgets;

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

use eframe::egui;
use tracing_subscriber::EnvFilter;

/// Fetch device names from DB for slot status generation.
fn device_names(state: &rf_web::AppState) -> HashMap<String, String> {
    state.db().get_device_names().unwrap_or_default()
}

fn main() {
    // --- Panic handler: write to crash.log ---
    std::panic::set_hook(Box::new(|info| {
        let crash_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().and_then(|p| p.parent())
            .map(|p| p.join("crash.log"))
            .unwrap_or_else(|| std::path::PathBuf::from("crash.log"));
        let msg = format!("{}\n\nBacktrace:\n{:?}", info, std::backtrace::Backtrace::force_capture());
        let _ = std::fs::write(&crash_path, &msg);
        eprintln!("PANIC: {}", info);
    }));

    // --- Tracing ---
    // Log to file if RFLOG_LOG_FILE env is set, otherwise stderr
    let filter = EnvFilter::from_default_env()
        .add_directive("rf_log_egui=info".parse().unwrap())
        .add_directive("rf_scan=info".parse().unwrap())
        .add_directive("rf_sdr=info".parse().unwrap())
        .add_directive("rf_dsp=info".parse().unwrap())
        .add_directive("rf_web=info".parse().unwrap())
        .add_directive("rf_db=info".parse().unwrap());
    if let Ok(log_path) = std::env::var("RFLOG_LOG_FILE") {
        let file = std::fs::File::create(&log_path).expect("Failed to create log file");
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(file)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
    }

    tracing::info!("RF-LOG egui v0.2.0 starting (Glass Cockpit HUD)...");

    // --- Data directory ---
    let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("Cannot determine project root");
    let data_dir = project_root.join("data");
    std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");
    let db_path = data_dir.join("rf-log.sqlite");
    let freq_json_path = data_dir.join("portland-frequencies.json");
    let freq_json_str = freq_json_path.to_str().unwrap_or("").to_string();

    // --- Database ---
    let db = rf_db::Db::open(db_path.to_str().expect("invalid db path"))
        .expect("Failed to open database");
    tracing::info!("Database initialized at {}", db_path.display());

    match db.ensure_default_test_operation() {
        Ok(id) => tracing::info!("Default test operation ready (id={})", id),
        Err(e) => tracing::warn!("Could not ensure default test operation: {}", e),
    }

    // --- AppState ---
    let empty_sdr = rf_sdr::SdrStatus {
        detected: false,
        driver: String::new(),
        serial: String::new(),
        sample_rate: 0.0,
    };
    let state = rf_web::AppState::new(db, empty_sdr, Vec::new());

    // --- Tokio runtime for background tasks ---
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    // --- SIEM Event Pipeline ---
    let event_bus = rf_events::EventBus::new();
    let (pipeline, gps_enricher, ops_enricher, session_enricher) =
        event_ingestion::create_pipeline(event_bus.clone());
    let pipeline = Arc::new(pipeline);

    // Batch writer: subscribes to EventBus, flushes to SQLite
    rt.spawn(event_ingestion::batch_writer_task(
        event_bus.clone(),
        state.db().clone(),
    ));

    // EventManager: evaluates custom event rules, produces derived events
    rt.spawn(event_manager::event_manager_task(
        event_bus.clone(),
        state.db().clone(),
        pipeline.clone(),
    ));

    // AlertEngine: sound channel created below after cpal setup
    // (alert_engine_task spawned after audio init)

    // SIGEX bridge: 3-tier passive intelligence processing
    sigex_bridge::spawn_all(&rt, state.clone(), pipeline.clone());

    // Lifecycle monitor: operation/site/GPS state transitions → SIEM events
    lifecycle_monitor::spawn(&rt, state.clone(), pipeline.clone());

    // --- Background services ---
    rt.spawn(rf_web::heartbeat_loop(state.clone()));
    {
        let s = state.clone();
        rt.spawn(async move { rf_web::gps_auto_detect(s).await });
    }
    rt.spawn(rf_web::gps_simulation_loop(state.clone()));
    rt.spawn(rf_web::gps_serial_loop(state.clone()));
    rt.spawn(rf_web::gps_fixed_loop(state.clone()));
    match state.db().close_all_open_site_sessions() {
        Ok(n) if n > 0 => tracing::info!("Closed {} orphaned site sessions", n),
        Ok(_) => {}
        Err(e) => tracing::warn!("Failed to close orphaned site sessions: {}", e),
    }
    rt.spawn(rf_web::geofence_loop(state.clone()));

    // --- Enricher update loop (GPS + operation context + session sweep) ---
    {
        let s = state.clone();
        let gps = gps_enricher.clone();
        let ops = ops_enricher.clone();
        let sess = session_enricher.clone();
        let pipe = pipeline.clone();
        rt.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                // GPS enricher update
                let coords = s.receiver_coords();
                gps.update(coords.lat, coords.lon);
                // Operation enricher update
                let config = s.config();
                ops.update(
                    config.active_operation_id.unwrap_or(0),
                    config.active_site_session_id.unwrap_or(0),
                );
                // Session correlator: sweep timed-out sessions, persist finalized
                let finalized = sess.sweep_and_drain();
                for session in &finalized {
                    // Persist to transmission_sessions table
                    if let Err(e) = s.db().insert_transmission_session(session) {
                        tracing::error!("Failed to persist transmission session {}: {}", session.trace_id, e);
                    }
                    // Emit session summary event through pipeline
                    let summary = rf_events::SessionCorrelator::session_to_event(session);
                    pipe.ingest(summary);
                }
                if !finalized.is_empty() {
                    tracing::debug!(
                        "Session correlator: finalized {} sessions, {} active",
                        finalized.len(), sess.active_count(),
                    );
                }
            }
        });
    }

    // --- DSP/Scan channels ---
    let (frame_tx, frame_rx) = mpsc::channel::<rf_scan::SpectrumFrame>();
    let (audio_tx, audio_rx) = mpsc::channel::<rf_recorder::AudioChunk>();
    let (rds_tx, rds_rx) = mpsc::channel::<String>();
    let (p25_tx, p25_rx) = mpsc::channel::<String>();
    let (rec_status_tx, rec_status_rx) = mpsc::channel::<rf_recorder::RecorderStatus>();
    let (rec_finalize_tx, rec_finalize_rx) = mpsc::channel::<rf_recorder::FinalizeResult>();
    let (rec_cmd_tx, rec_cmd_rx) = mpsc::channel::<rf_recorder::RecorderCommand>();

    // Spawn recorder engine thread
    std::thread::Builder::new()
        .name("recorder".into())
        .spawn(move || {
            rf_recorder::run_recorder(rec_cmd_rx, rec_status_tx, rec_finalize_tx);
        })
        .expect("Failed to spawn recorder thread");

    // --- Audio output (cpal/WASAPI) ---
    let (cpal_tx, cpal_rx) = mpsc::channel::<Vec<f32>>();

    // Alert sound channel: AlertEngine → cpal callback (mixed with monitor audio)
    let (alert_sound_tx, alert_sound_rx) = mpsc::channel::<Vec<f32>>();

    // Playback channel: UI sends WAV samples → cpal callback (mixed with monitor audio)
    let (playback_tx, playback_rx) = mpsc::channel::<Vec<f32>>();

    // Shared alert volume atomic — UI writes, cpal callback reads
    let alert_volume_atomic = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(
        0.8_f32.to_bits(), // default 80%, overwritten when UI state loads
    ));

    // Audio fork: DSP AudioChunks → cpal speaker + P25 clip events → SIEM
    let pipeline_audio = Arc::clone(&pipeline);
    std::thread::Builder::new()
        .name("audio_fork".into())
        .spawn(move || {
            while let Ok(chunk) = audio_rx.recv() {
                // Forward audio samples to speaker
                if !chunk.samples.is_empty() {
                    let _ = cpal_tx.send(chunk.samples);
                }
                // Emit SIEM events for P25 clip boundaries
                if let Some(ref evt) = chunk.p25_event {
                    match evt {
                        rf_recorder::P25ClipEvent::TransmissionStart => {
                            pipeline_audio.ingest(event_ingestion::clip_start_record(
                                chunk.freq_mhz,
                                chunk.talkgroup,
                                chunk.source_unit,
                                chunk.encrypted,
                            ));
                        }
                        rf_recorder::P25ClipEvent::TransmissionEnd => {
                            pipeline_audio.ingest(event_ingestion::clip_end_record(
                                chunk.freq_mhz,
                                chunk.talkgroup,
                                chunk.source_unit,
                                chunk.encrypted,
                            ));
                        }
                        rf_recorder::P25ClipEvent::VoiceFrame => {}
                    }
                }
            }
        })
        .expect("Failed to spawn audio fork");

    // Native audio output
    let config = state.config();
    // Shared volume/mute atomics — UI writes, cpal callback reads
    let volume_atomic = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(
        (config.volume as f32 / 100.0_f32).to_bits(),
    ));
    let muted_atomic = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(config.muted));
    let _audio_stream = {
        use std::collections::VecDeque;
        use std::sync::atomic::Ordering;
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        match host.default_output_device() {
            Some(device) => {
                let stream_config = cpal::StreamConfig {
                    channels: 1,
                    sample_rate: cpal::SampleRate(48000),
                    buffer_size: cpal::BufferSize::Default,
                };
                let volume = std::sync::Arc::clone(&volume_atomic);
                let muted = std::sync::Arc::clone(&muted_atomic);
                let alert_vol_cpal = std::sync::Arc::clone(&alert_volume_atomic);
                let mut buf: VecDeque<f32> = VecDeque::with_capacity(48000);
                let mut alert_buf: VecDeque<f32> = VecDeque::with_capacity(48000);
                let mut playback_buf: VecDeque<f32> = VecDeque::with_capacity(48000);

                match device.build_output_stream(
                    &stream_config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        // Drain monitor audio
                        while let Ok(chunk) = cpal_rx.try_recv() {
                            buf.extend(chunk.iter());
                        }
                        // Cap at 0.5 sec to prevent latency buildup
                        while buf.len() > 24000 {
                            let _ = buf.pop_front();
                        }
                        // Drain alert audio
                        while let Ok(chunk) = alert_sound_rx.try_recv() {
                            alert_buf.extend(chunk.iter());
                        }
                        // Cap alert buffer at 1 sec (alerts are short tones)
                        while alert_buf.len() > 48000 {
                            let _ = alert_buf.pop_front();
                        }
                        // Drain playback audio (WAV file playback)
                        while let Ok(chunk) = playback_rx.try_recv() {
                            playback_buf.extend(chunk.iter());
                        }
                        // Cap playback buffer at 2 sec
                        while playback_buf.len() > 96000 {
                            let _ = playback_buf.pop_front();
                        }
                        let vol = f32::from_bits(volume.load(Ordering::Relaxed));
                        let is_muted = muted.load(Ordering::Relaxed);
                        let alert_vol = f32::from_bits(alert_vol_cpal.load(Ordering::Relaxed));
                        for sample in data.iter_mut() {
                            let monitor = if is_muted { 0.0 } else { buf.pop_front().unwrap_or(0.0) * vol };
                            // Alert audio always plays (ignores mute — alerts must be audible)
                            let alert = alert_buf.pop_front().unwrap_or(0.0) * alert_vol;
                            // Playback audio uses monitor volume
                            let playback = playback_buf.pop_front().unwrap_or(0.0) * vol;
                            *sample = (monitor + alert + playback).clamp(-1.0, 1.0);
                        }
                    },
                    |err| {
                        tracing::error!("cpal output stream error: {}", err);
                    },
                    None,
                ) {
                    Ok(stream) => {
                        if let Err(e) = stream.play() {
                            tracing::warn!("Failed to start audio stream: {}", e);
                            None
                        } else {
                            tracing::info!("Native audio output initialized (cpal/WASAPI)");
                            Some(stream)
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to build audio stream: {} — monitor will be silent", e);
                        None
                    }
                }
            }
            None => {
                tracing::warn!("No audio output device — monitor will be silent");
                // Drain cpal_rx, alert_sound_rx and playback_rx to prevent channel backpressure
                std::thread::Builder::new()
                    .name("audio_drain".into())
                    .spawn(move || {
                        loop {
                            // Drain all channels; break when all are disconnected
                            let mon = cpal_rx.try_recv();
                            let alert = alert_sound_rx.try_recv();
                            let _play = playback_rx.try_recv();
                            if mon.is_err() && alert.is_err() {
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                        }
                    })
                    .expect("Failed to spawn audio drain");
                None
            }
        }
    };

    // AlertEngine: evaluates alert rules, creates firings, executes actions
    rt.spawn(alert_engine::alert_engine_task(
        event_bus.clone(),
        state.db().clone(),
        pipeline.clone(),
        state.clone(),
        alert_sound_tx,
    ));

    // --- Empty pool for config poller (devices added by deferred init) ---
    let pool = Arc::new(Mutex::new(pool::DevicePool::empty()));

    // --- Config poller ---
    {
        let state_cp = state.clone();
        let pool_cp = Arc::clone(&pool);
        let pipeline_cp = Arc::clone(&pipeline);
        rt.spawn(config_poller::run(
            state_cp, pool_cp, pipeline_cp,
            rec_status_rx, rec_finalize_rx,
            frame_rx, rds_rx, p25_rx,
        ));
    }

    // Clone rec_cmd_tx for deferred SDR init and for UI
    let rec_cmd_tx_pool = rec_cmd_tx.clone();
    let rec_cmd_tx_ui = rec_cmd_tx.clone();

    // --- Deferred SDR initialization ---
    {
        let state_init = state.clone();
        let pool_init = Arc::clone(&pool);
        let pipeline_init = Arc::clone(&pipeline);
        let freq_json = freq_json_str.clone();
        let sample_rate = 2_400_000.0;

        rt.spawn(async move {
            // Enumerate SDR devices (blocking)
            state_init.set_startup_phase("Enumerating SDR devices...");
            let sdr_devices = tokio::task::spawn_blocking(rf_sdr::enumerate_all)
                .await
                .unwrap_or_default();

            let sdr_status = if let Some(dev) = sdr_devices.first() {
                rf_sdr::SdrStatus {
                    detected: true,
                    driver: dev.label.clone(),
                    serial: dev.serial.clone(),
                    sample_rate,
                }
            } else {
                rf_sdr::SdrStatus {
                    detected: false,
                    driver: String::new(),
                    serial: String::new(),
                    sample_rate: 0.0,
                }
            };

            if sdr_devices.is_empty() {
                tracing::warn!("No SDR hardware detected — running in simulation mode");
                state_init.set_startup_phase("No SDR hardware — simulation mode");
            } else {
                state_init.set_startup_phase(&format!("{} SDR device(s) found", sdr_devices.len()));
                tracing::info!("{} SDR device(s) detected:", sdr_devices.len());
                for (i, d) in sdr_devices.iter().enumerate() {
                    tracing::info!("  [{}] {} (serial: {})", i, d.label, d.serial);
                }
            }

            // Update AppState
            state_init.set_sdr_devices(sdr_devices.clone());
            state_init.set_sdr_status(sdr_status.clone());
            state_init.set_sdr_alive(sdr_status.detected);

            // Register devices in DB
            {
                let db = state_init.db().clone();
                let devs = sdr_devices.clone();
                tokio::task::spawn_blocking(move || {
                    for d in &devs {
                        if !d.serial.is_empty() {
                            let manufacturer = d.args.get("manufacturer").cloned().unwrap_or_default();
                            let product = d.args.get("product").cloned().unwrap_or_default();
                            let tuner = d.args.get("tuner").cloned().unwrap_or_default();
                            let _ = db.upsert_device(&d.serial, &manufacturer, &product, &tuner);
                        }
                    }
                });
            }

            // Open device pool (blocking — each device takes ~13s)
            let initial_bands = state_init.config().bands.clone();
            let shutdown_flag = state_init.shutdown_flag();

            let device_pool = if !sdr_devices.is_empty() {
                state_init.set_startup_phase(&format!("Opening {} SDR device(s)...", sdr_devices.len()));
                let ft = frame_tx.clone();
                let at = audio_tx.clone();
                let rt_tx = rds_tx.clone();
                let pt = p25_tx.clone();
                let ib = initial_bands.clone();
                let fj = freq_json.clone();
                let sd = Arc::clone(&shutdown_flag);
                let devs = sdr_devices.clone();
                let rct = rec_cmd_tx_pool.clone();
                tokio::task::spawn_blocking(move || {
                    tracing::info!("SDR init: opening {} device(s)...", devs.len());
                    let pool = pool::DevicePool::open_all_devices(
                        &devs, sample_rate, &ft, &at, &rt_tx, &pt, &ib, &fj, sd,
                        None, Some(rct),
                    );
                    if pool.slots.is_empty() {
                        tracing::warn!("All SDR devices failed to open");
                    } else {
                        tracing::info!("{} SDR device(s) opened successfully", pool.slots.len());
                    }
                    pool
                }).await.unwrap()
            } else {
                // Simulation mode
                state_init.set_startup_phase("Starting simulation...");
                let ft = frame_tx.clone();
                let at = audio_tx.clone();
                let rt_tx = rds_tx.clone();
                let pt = p25_tx.clone();
                let ib = initial_bands.clone();
                let fj = freq_json.clone();
                let sd = Arc::clone(&shutdown_flag);
                let rct = rec_cmd_tx_pool.clone();
                tokio::task::spawn_blocking(move || {
                    pool::DevicePool::create_simulated_slot(
                        &fj, sample_rate, &ft, &at, &rt_tx, &pt, &ib, sd,
                        None, Some(rct),
                    )
                }).await.unwrap()
            };

            // If all real devices failed, fall back to simulation
            let device_pool = if device_pool.slots.is_empty() && !sdr_devices.is_empty() {
                state_init.set_startup_phase("Device failure — falling back to simulation...");
                tracing::warn!("Falling back to simulation mode");
                let ft = frame_tx.clone();
                let at = audio_tx.clone();
                let rt_tx = rds_tx.clone();
                let pt = p25_tx.clone();
                let ib = initial_bands.clone();
                let fj = freq_json.clone();
                let sd = Arc::clone(&shutdown_flag);
                let rct = rec_cmd_tx_pool.clone();
                tokio::task::spawn_blocking(move || {
                    pool::DevicePool::create_simulated_slot(
                        &fj, sample_rate, &ft, &at, &rt_tx, &pt, &ib, sd,
                        None, Some(rct),
                    )
                }).await.unwrap()
            } else {
                device_pool
            };

            // Install into shared pool
            state_init.set_sdr_slots(device_pool.to_slot_statuses(&device_names(&state_init)));
            state_init.set_scan_statuses(device_pool.scan_statuses());

            // SIEM: system.sdr.connect for each successfully opened device
            for slot in &device_pool.slots {
                pipeline_init.ingest(event_ingestion::sdr_connect_record(
                    &slot.device_key,
                    &slot.role,
                ));
            }

            *pool_init.lock().unwrap() = device_pool;

            // Auto-enable scanning with default bands if none configured
            {
                let mut config = state_init.config();
                if config.bands.is_empty() {
                    let default_bands = vec![
                        "FM".to_string(), "VHF".to_string(), "UHF".to_string(),
                    ];
                    tracing::info!("No bands configured — enabling defaults: {:?}", default_bands);
                    state_init.update_config(|c| {
                        c.bands = default_bands;
                        c.scanning = true;
                        c.mode = "scan".to_string();
                    });
                    config = state_init.config();
                } else if !config.scanning {
                    tracing::info!("Enabling scanning on configured bands: {:?}", config.bands);
                    state_init.update_config(|c| {
                        c.scanning = true;
                        c.mode = "scan".to_string();
                    });
                    config = state_init.config();
                }

                let pool = pool_init.lock().unwrap();

                let thr = config.threshold;
                pool.broadcast_scan_cmd(move || rf_scan::ScanCommand::SetThreshold(thr));
                let snr = config.snr_margin;
                pool.broadcast_scan_cmd(move || rf_scan::ScanCommand::SetSnrMargin(snr));

                pool.broadcast_scan_cmd(|| rf_scan::ScanCommand::Start);
                tracing::info!("SDR init complete: {} device(s), scanning started on {:?}", pool.slots.len(), config.bands);
                state_init.set_startup_phase("Starting scan...");
            }

            // Clear startup phase after a brief delay so the user sees the final message
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            state_init.set_startup_phase("");
            tracing::info!("Deferred SDR initialization complete");
        });
    }

    // --- UI Bridge ---
    let bridge = bridge::UiBridge::new(
        state.subscribe_heartbeat(),
        state.subscribe_spectrum(),
        state.subscribe_protocol(),
    );

    // EventBus subscription for SIEM live tail in WATCHDOG view
    let event_bus_rx = event_bus.subscribe();

    // --- Launch egui ---
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 600.0])
            .with_title("RF-LOG SIGINT Platform"),
        ..Default::default()
    };

    tracing::info!("Launching RF-LOG egui...");
    eframe::run_native(
        "rf-log-egui",
        options,
        Box::new(|cc| {
            theme::setup_tactical_theme(&cc.egui_ctx);
            Ok(Box::new(app::RfLogApp::new(state, bridge, event_bus_rx, alert_volume_atomic, volume_atomic, muted_atomic, rec_cmd_tx_ui, playback_tx, cc)))
        }),
    )
    .expect("Failed to run eframe");
}
