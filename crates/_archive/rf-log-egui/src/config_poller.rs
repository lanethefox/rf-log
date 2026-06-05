//! Config poller — watches AppConfig for changes and dispatches commands
//! to SDR devices, DSP pipelines, scan controllers, and the recorder engine.
//!
//! Extracted from main.rs for readability. The `run()` method is the async
//! loop that runs for the lifetime of the application.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

use crate::dsp_bridge;
use crate::event_ingestion;
use crate::pool;

/// SDR center frequency offset to avoid RTL-SDR DC spike.
const CC_DC_OFFSET_HZ: f64 = 100_000.0;

/// All state tracked between iterations of the config poller loop.
struct PrevState {
    scanning: bool,
    bands: Vec<String>,
    threshold: f64,
    per_band_threshold: HashMap<String, f64>,
    gain: f64,
    per_device_gain: HashMap<String, f64>,
    snr_margin: f64,
    persist_min_hits: u32,
    persist_window: u32,
    mode: String,
    freq: f64,
    modulation: String,
    squelch: f64,
    scan_active: bool,
    per_device_agc: HashMap<String, bool>,
    per_device_ppm: HashMap<String, f64>,
    per_device_offset_tuning: HashMap<String, bool>,
    bandwidth_hz: f64,
    priority_cache: HashMap<u32, u8>,
    dept_cache: HashMap<u32, String>,
    quarantine: Vec<bool>,
    cc_last_valid_tsbk: std::time::Instant,
    cc_hunt_active: bool,
    cc_freq: Option<f64>,
    sync_count: u64,
    last_rebalance: std::time::Instant,
    slot_count: usize,
    antenna_map: HashMap<String, (f64, f64)>,
    last_antenna_check: std::time::Instant,
    /// Per-band last emission time for source-side spectrum throttle (15fps/band).
    last_spectrum_emit: HashMap<String, std::time::Instant>,
    /// Last fingerprint DB stats refresh
    last_fp_stats_refresh: std::time::Instant,
}

impl Default for PrevState {
    fn default() -> Self {
        Self {
            scanning: false,
            bands: Vec::new(),
            threshold: -80.0,
            per_band_threshold: HashMap::new(),
            gain: 0.0,
            per_device_gain: HashMap::new(),
            snr_margin: 10.0,
            persist_min_hits: 2,
            persist_window: 3,
            mode: "scan".into(),
            freq: 0.0,
            modulation: "NFM".into(),
            squelch: -60.0,
            scan_active: false,
            per_device_agc: HashMap::new(),
            per_device_ppm: HashMap::new(),
            per_device_offset_tuning: HashMap::new(),
            bandwidth_hz: 0.0,
            priority_cache: HashMap::new(),
            dept_cache: HashMap::new(),
            quarantine: Vec::new(),
            cc_last_valid_tsbk: std::time::Instant::now(),
            cc_hunt_active: false,
            cc_freq: None,
            sync_count: 0,
            last_rebalance: std::time::Instant::now(),
            slot_count: 0,
            antenna_map: HashMap::new(),
            last_antenna_check: std::time::Instant::now(),
            last_spectrum_emit: HashMap::new(),
            last_fp_stats_refresh: std::time::Instant::now(),
        }
    }
}

/// Fetch device names from DB for slot status generation.
fn device_names(state: &rf_web::AppState) -> HashMap<String, String> {
    state.db().get_device_names().unwrap_or_default()
}

/// Run the config poller loop. This is the central dispatch loop that
/// bridges AppConfig changes to hardware commands.
pub async fn run(
    state: rf_web::AppState,
    pool: Arc<Mutex<pool::DevicePool>>,
    pipeline: Arc<rf_events::pipeline::IngestionPipeline>,
    rec_status_rx: mpsc::Receiver<rf_recorder::RecorderStatus>,
    rec_finalize_rx: mpsc::Receiver<rf_recorder::FinalizeResult>,
    frame_rx: mpsc::Receiver<rf_scan::SpectrumFrame>,
    rds_rx: mpsc::Receiver<String>,
    p25_rx: mpsc::Receiver<String>,
) {
    let mut prev = PrevState::default();

    loop {
        // Shutdown check
        if state.is_shutdown() {
            tracing::info!("Config poller: shutdown signal received");
            let pool = pool.lock().unwrap();
            pool.broadcast_sdr_cmd(|| rf_sdr::SdrCommand::Stop);
            break;
        }

        let config = state.config();

        // Detect pool population changes (e.g., refresh_sdr adding devices).
        // When new devices appear, reset prev to force full resync of
        // scanning state, thresholds, and other parameters.
        {
            let current_slots = pool.lock().unwrap().slots.len();
            if current_slots > 0 && current_slots != prev.slot_count {
                if prev.slot_count > 0 {
                    tracing::info!(
                        "Config poller: pool changed ({} → {} slots) — forcing resync",
                        prev.slot_count, current_slots
                    );
                }
                prev.slot_count = current_slots;
                // Force resync by marking scanning as "not yet sent"
                prev.scanning = !config.scanning;
                prev.bands.clear();
                prev.threshold = -99999.0;
                prev.snr_margin = -99999.0;
                prev.persist_min_hits = u32::MAX;
                prev.per_device_gain.clear();
                prev.per_device_agc.clear();
                prev.per_device_ppm.clear();
                prev.per_device_offset_tuning.clear();
                prev.quarantine.clear();
            } else {
                prev.slot_count = current_slots;
            }
        }

        handle_mode_change(&state, &pool, &config, &mut prev, &pipeline);
        handle_scanning_change(&state, &pool, &config, &mut prev);
        handle_band_change(&state, &pool, &config, &mut prev);
        dispatch_thresholds(&pool, &config, &mut prev);
        dispatch_device_params(&pool, &config, &mut prev);
        dispatch_dsp_params(&pool, &config, &mut prev);
        handle_network_scanner(&state, &pool, &config, &mut prev);
        drain_rds(&state, &rds_rx, &pipeline);
        drain_p25(&state, &pool, &p25_rx, &mut prev, &pipeline);
        drain_recorder(&state, &rec_status_rx, &rec_finalize_rx, &pipeline);
        check_quarantine(&state, &pool, &config, &mut prev, &pipeline);
        check_sweep_balance(&state, &pool, &config, &mut prev);
        check_antenna_map(&state, &pool, &config, &mut prev);

        // Process spectrum frames (non-blocking try_recv, sleep if empty)
        if !process_spectrum_tick(&state, &frame_rx, &mut prev, &pipeline) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Recorder status/finalize draining
// ---------------------------------------------------------------------------

fn drain_recorder(
    state: &rf_web::AppState,
    status_rx: &mpsc::Receiver<rf_recorder::RecorderStatus>,
    finalize_rx: &mpsc::Receiver<rf_recorder::FinalizeResult>,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) {
    while let Ok(status) = status_rx.try_recv() {
        let data = rf_web::RecorderStatusData {
            active_audio: status.active_audio.iter()
                .map(|s| serde_json::json!({
                    "db_id": s.db_id,
                    "freq_mhz": s.freq_mhz,
                    "duration_sec": s.duration_sec,
                    "file_size_bytes": s.file_size_bytes,
                }))
                .collect(),
            active_iq: status.active_iq.iter()
                .map(|s| serde_json::json!({
                    "db_id": s.db_id,
                    "freq_mhz": s.freq_mhz,
                    "duration_sec": s.duration_sec,
                    "file_size_bytes": s.file_size_bytes,
                }))
                .collect(),
        };
        state.set_recorder_status(data);
    }

    while let Ok(result) = finalize_rx.try_recv() {
        let _ = state.db().finalize_recording(
            result.db_id,
            result.file_size_bytes as i64,
            result.duration_sec,
        );
        // SIEM: system.recording.stop
        pipeline.ingest(event_ingestion::recording_stop_record(
            result.db_id,
            result.duration_sec,
            result.file_size_bytes as i64,
        ));

        // REC-11: Post-capture fingerprint extraction for IQ recordings
        maybe_extract_fingerprint(state, result.db_id, pipeline);
    }
}

// ---------------------------------------------------------------------------
// REC-11: Post-capture IQ fingerprint extraction
// ---------------------------------------------------------------------------

fn maybe_extract_fingerprint(
    state: &rf_web::AppState,
    db_id: i64,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) {
    // Query the recording to check if it's IQ type
    let recording = match state.db().get_recording(db_id) {
        Ok(Some(r)) if r.rec_type == "iq" => r,
        _ => return, // Not IQ or not found — skip
    };

    let file_path = &recording.file_path;
    let freq_mhz = recording.freq_mhz;

    tracing::info!("REC-11: Extracting fingerprint from IQ recording id={}", db_id);

    // Read the IQ file: raw interleaved f32 pairs (I, Q, I, Q, ...)
    let bytes = match std::fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("REC-11: Failed to read IQ file {}: {}", file_path, e);
            return;
        }
    };

    let float_count = bytes.len() / 4;
    let pair_count = float_count / 2;
    if pair_count < 480 {
        tracing::debug!("REC-11: IQ file too small for fingerprint ({} samples)", pair_count);
        return;
    }

    // Cap at 500K samples to keep processing fast
    let cap = pair_count.min(500_000);
    let mut accum = rf_dsp::FingerprintAccumulator::new();
    let mut iq_buf = Vec::with_capacity(cap);
    for i in 0..cap {
        let i_off = i * 8;
        let q_off = i_off + 4;
        if q_off + 4 <= bytes.len() {
            let i_val = f32::from_le_bytes([
                bytes[i_off], bytes[i_off + 1], bytes[i_off + 2], bytes[i_off + 3],
            ]);
            let q_val = f32::from_le_bytes([
                bytes[q_off], bytes[q_off + 1], bytes[q_off + 2], bytes[q_off + 3],
            ]);
            iq_buf.push(num_complex::Complex32::new(i_val, q_val));
        }
    }
    accum.feed_iq(&iq_buf);

    if let Some(fp) = accum.finalize() {
        // Store in DB
        match state.db().upsert_radio_fingerprint(
            fp.cfo_hz,
            fp.iq_amplitude_imbal,
            fp.iq_phase_imbal,
            fp.avg_power_db,
            fp.power_variance,
            fp.sample_count as i32,
            freq_mhz,
            50.0,  // cfo_bucket_hz
            0.01,  // iq_resolution
        ) {
            Ok((fp_id, count)) => {
                tracing::info!(
                    "REC-11: Fingerprint extracted from recording {}: fp_id={}, count={}",
                    db_id, fp_id, count
                );
                // Emit SIGEX event
                pipeline.ingest(event_ingestion::sigex_to_log_record(
                    "new_emitter",
                    &format!("Auto-extracted fingerprint from IQ recording id={}", db_id),
                    Some(freq_mhz),
                    None,
                ));
            }
            Err(e) => {
                tracing::warn!("REC-11: Failed to store fingerprint: {}", e);
            }
        }
    } else {
        tracing::debug!("REC-11: Fingerprint extraction returned None for recording {}", db_id);
    }
}

// ---------------------------------------------------------------------------
// Mode switching (scan ↔ monitor)
// ---------------------------------------------------------------------------

fn handle_mode_change(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) {
    if config.mode != prev.mode
        || (config.mode == "monitor" && (config.freq - prev.freq).abs() > 0.001)
    {
        let old_mode = prev.mode.clone();
        prev.mode = config.mode.clone();
        prev.freq = config.freq;

        // SIEM: system.mode.change
        let freq = if config.mode == "monitor" { Some(config.freq) } else { None };
        pipeline.ingest(event_ingestion::mode_change_record(&old_mode, &config.mode, freq));
        let mut pool = pool.lock().unwrap();

        if config.mode == "monitor" && config.scanning {
            // Stop primary's scan, switch to monitor
            if let Some(primary) = pool.primary() {
                let _ = primary.scan_cmd_tx.send(rf_scan::ScanCommand::Stop);
                let freq_hz = config.freq * 1_000_000.0;
                let _ = primary.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetFreq(freq_hz));
                let _ = primary.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::MonitorMode {
                        center_freq: freq_hz,
                        target_freq: freq_hz,
                        mode: config.modulation.clone(),
                        squelch: config.squelch,
                    },
                );
            }
            state.set_monitor_freq(Some(config.freq));
            state.set_sdr_role("monitor");

            // Redistribute primary's bands to remaining devices
            if pool.slots.len() > 1 {
                if let Some(primary) = pool.primary_mut() {
                    primary.role = "monitor".into();
                    primary.assigned_bands.clear();
                }
                let alive_others: Vec<usize> = pool.slots.iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(_, s)| s.alive.load(std::sync::atomic::Ordering::Relaxed))
                    .map(|(i, _)| i)
                    .collect();
                if !alive_others.is_empty() {
                    let dist = pool::distribute_segments(
                        &config.bands, alive_others.len(),
                        &pool.band_defs, pool.sample_rate,
                    );
                    for (di, &si) in alive_others.iter().enumerate() {
                        if let Some(ranges) = dist.get(di) {
                            if let Some(slot) = pool.slots.get_mut(si) {
                                let band_keys: Vec<String> = {
                                    let mut keys: Vec<String> = ranges.iter().map(|r| r.key.clone()).collect();
                                    keys.sort();
                                    keys.dedup();
                                    keys
                                };
                                slot.assigned_bands = band_keys;
                                let _ = slot.scan_cmd_tx.send(
                                    rf_scan::ScanCommand::SetBandRanges(ranges.clone()),
                                );
                            }
                        }
                    }
                }
            }

            state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));
            tracing::info!(
                "Monitor mode: tuned to {} MHz, demod={}",
                config.freq, config.modulation
            );
        } else if config.mode == "monitor" && !config.scanning {
            // wait for scanning-change block
        } else {
            // Back to scan mode
            if let Some(primary) = pool.primary() {
                let _ = primary.dsp_cmd_tx.send(dsp_bridge::DspCommand::ScanMode);
            }
            state.set_monitor_freq(None);
            state.set_squelch_open(false);
            state.set_sdr_role(if config.scanning { "scan" } else { "idle" });

            if let Some(primary) = pool.primary_mut() {
                primary.role = "scan".into();
            }
            pool.redistribute(&config.bands);
            state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));

            if config.scanning {
                pool.broadcast_scan_cmd(|| rf_scan::ScanCommand::Start);
            }
            tracing::info!("Scan mode resumed");
        }
    }
}

// ---------------------------------------------------------------------------
// Scanning state changes
// ---------------------------------------------------------------------------

fn handle_scanning_change(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    if config.scanning != prev.scanning {
        prev.scanning = config.scanning;
        let mut pool = pool.lock().unwrap();

        if config.mode == "scan" {
            if config.scanning {
                state.set_sdr_role("scan");
                for slot in &mut pool.slots {
                    slot.role = "scan".into();
                }
                pool.broadcast_scan_cmd(|| rf_scan::ScanCommand::Start);
            } else {
                state.set_sdr_role("idle");
                for slot in &mut pool.slots {
                    slot.role = "idle".into();
                }
                pool.broadcast_scan_cmd(|| rf_scan::ScanCommand::Stop);
            }
            state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));
        }
        if config.mode == "monitor" && config.scanning {
            if let Some(primary) = pool.primary() {
                let freq_hz = config.freq * 1_000_000.0;
                let _ = primary.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetFreq(freq_hz));
                let _ = primary.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::MonitorMode {
                        center_freq: freq_hz,
                        target_freq: freq_hz,
                        mode: config.modulation.clone(),
                        squelch: config.squelch,
                    },
                );
            }
            state.set_monitor_freq(Some(config.freq));
            state.set_sdr_role("monitor");
        }
        if !config.scanning && config.mode == "monitor" {
            if let Some(primary) = pool.primary() {
                let _ = primary.dsp_cmd_tx.send(dsp_bridge::DspCommand::ScanMode);
            }
            state.set_monitor_freq(None);
            state.set_squelch_open(false);
            state.set_sdr_role("idle");
        }
    }
}

// ---------------------------------------------------------------------------
// Band changes
// ---------------------------------------------------------------------------

fn handle_band_change(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    if config.bands != prev.bands {
        prev.bands = config.bands.clone();
        let mut pool = pool.lock().unwrap();
        pool.redistribute(&prev.bands);
        state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));
    }
}

// ---------------------------------------------------------------------------
// Threshold dispatch (global + per-band)
// ---------------------------------------------------------------------------

fn dispatch_thresholds(
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    if (config.threshold - prev.threshold).abs() > 0.1 {
        prev.threshold = config.threshold;
        let pool = pool.lock().unwrap();
        let thr = prev.threshold;
        pool.broadcast_scan_cmd(move || rf_scan::ScanCommand::SetThreshold(thr));
    }
    if config.per_band_threshold != prev.per_band_threshold {
        prev.per_band_threshold = config.per_band_threshold.clone();
        let pool = pool.lock().unwrap();
        let bt = prev.per_band_threshold.clone();
        pool.broadcast_scan_cmd(move || rf_scan::ScanCommand::SetBandThresholds(bt.clone()));
    }
}

// ---------------------------------------------------------------------------
// Per-device hardware parameters (gain, AGC, PPM, offset tuning)
// ---------------------------------------------------------------------------

fn dispatch_device_params(
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    // Gain
    {
        let pool = pool.lock().unwrap();
        let global_changed = (config.gain - prev.gain).abs() > 0.1;
        if global_changed {
            prev.gain = config.gain;
        }
        for slot in &pool.slots {
            let effective = config.per_device_gain
                .get(&slot.device_key)
                .copied()
                .unwrap_or(config.gain);
            let prev_val = prev.per_device_gain
                .get(&slot.device_key)
                .copied()
                .unwrap_or(0.0);
            if (effective - prev_val).abs() > 0.1 {
                prev.per_device_gain.insert(slot.device_key.clone(), effective);
                tracing::info!("Setting gain for {} to {}", slot.device_key, effective);
                let _ = slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetGain(effective));
            }
        }
    }
    // AGC
    {
        let pool = pool.lock().unwrap();
        for slot in &pool.slots {
            let effective = config.per_device_agc
                .get(&slot.device_key)
                .copied()
                .unwrap_or(false);
            let prev_val = prev.per_device_agc
                .get(&slot.device_key)
                .copied()
                .unwrap_or(false);
            if effective != prev_val {
                prev.per_device_agc.insert(slot.device_key.clone(), effective);
                tracing::info!("Setting AGC for {} to {}", slot.device_key, effective);
                let _ = slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetAgc(effective));
            }
        }
    }
    // PPM
    {
        let pool = pool.lock().unwrap();
        for slot in &pool.slots {
            let effective = config.per_device_ppm
                .get(&slot.device_key)
                .copied()
                .unwrap_or(0.0);
            let prev_val = prev.per_device_ppm
                .get(&slot.device_key)
                .copied()
                .unwrap_or(0.0);
            if (effective - prev_val).abs() > 0.01 {
                prev.per_device_ppm.insert(slot.device_key.clone(), effective);
                tracing::info!("Setting PPM for {} to {}", slot.device_key, effective);
                let _ = slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetPpm(effective));
            }
        }
    }
    // Offset tuning
    {
        let pool = pool.lock().unwrap();
        for slot in &pool.slots {
            let effective = config.per_device_offset_tuning
                .get(&slot.device_key)
                .copied()
                .unwrap_or(false);
            let prev_val = prev.per_device_offset_tuning
                .get(&slot.device_key)
                .copied()
                .unwrap_or(false);
            if effective != prev_val {
                prev.per_device_offset_tuning.insert(slot.device_key.clone(), effective);
                tracing::info!("Setting offset tuning for {} to {}", slot.device_key, effective);
                let _ = slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetOffsetTuning(effective));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DSP parameters (bandwidth, SNR margin, persistence, modulation, squelch)
// ---------------------------------------------------------------------------

fn dispatch_dsp_params(
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    if (config.bandwidth_hz - prev.bandwidth_hz).abs() > 0.1 {
        prev.bandwidth_hz = config.bandwidth_hz;
        if config.mode == "monitor" {
            let pool = pool.lock().unwrap();
            if let Some(primary) = pool.primary() {
                let _ = primary.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::SetBandwidth(prev.bandwidth_hz),
                );
            }
        }
    }
    if (config.snr_margin - prev.snr_margin).abs() > 0.1 {
        prev.snr_margin = config.snr_margin;
        let pool = pool.lock().unwrap();
        let m = prev.snr_margin;
        pool.broadcast_scan_cmd(move || rf_scan::ScanCommand::SetSnrMargin(m));
    }
    if config.persist_min_hits != prev.persist_min_hits
        || config.persist_window != prev.persist_window
    {
        prev.persist_min_hits = config.persist_min_hits;
        prev.persist_window = config.persist_window;
        let pool = pool.lock().unwrap();
        let mh = prev.persist_min_hits;
        let pw = prev.persist_window;
        pool.broadcast_scan_cmd(move || rf_scan::ScanCommand::SetPersistence {
            min_hits: mh,
            window: pw,
        });
    }
    if config.modulation != prev.modulation {
        prev.modulation = config.modulation.clone();
        if config.mode == "monitor" {
            let pool = pool.lock().unwrap();
            if let Some(primary) = pool.primary() {
                let _ = primary.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::SetModulation(prev.modulation.clone()),
                );
            }
        }
    }
    if (config.squelch - prev.squelch).abs() > 0.1 {
        prev.squelch = config.squelch;
        if config.mode == "monitor" {
            let pool = pool.lock().unwrap();
            if let Some(primary) = pool.primary() {
                let _ = primary.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::SetSquelch(prev.squelch),
                );
            }
        }
    }

}

// ---------------------------------------------------------------------------
// Network scanner (2-SDR P25 trunking)
// ---------------------------------------------------------------------------

fn handle_network_scanner(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    let scan_active = config.network_scan_active;

    if scan_active && !prev.scan_active {
        start_network_scanner(state, pool, config, prev);
    } else if !scan_active && prev.scan_active {
        stop_network_scanner(state, pool, config, prev);
    }

    if scan_active {
        check_voice_idle_timeout(state);
        check_cc_hunt(state, pool, config, prev);
        check_cc_freq_change(state, pool, prev);
    }
}

fn start_network_scanner(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    let mut pool = pool.lock().unwrap();
    let cc_freq_mhz = config.network_scan_cc_freq.unwrap_or(0.0);

    if pool.slots.len() >= 2 && cc_freq_mhz > 0.0 {
        tracing::info!(
            "Network scanner START: CC {:.4} MHz, watching {} TGs, {} SDRs",
            cc_freq_mhz, config.network_scan_tgids.len(), pool.slots.len()
        );

        if let Some(s) = pool.slots.get(0) {
            let _ = s.scan_cmd_tx.send(rf_scan::ScanCommand::Stop);
        }
        if let Some(s) = pool.slots.get(1) {
            let _ = s.scan_cmd_tx.send(rf_scan::ScanCommand::Stop);
        }

        // SDR #0 → CC
        let cc_freq_hz = cc_freq_mhz * 1_000_000.0;
        let sdr_center = cc_freq_hz + CC_DC_OFFSET_HZ;
        if let Some(cc_slot) = pool.slots.get(0) {
            let _ = cc_slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetFreq(sdr_center));
            let _ = cc_slot.dsp_cmd_tx.send(
                dsp_bridge::DspCommand::MonitorMode {
                    center_freq: sdr_center,
                    target_freq: cc_freq_hz,
                    mode: "P25".into(),
                    squelch: -90.0,
                },
            );
        }
        if let Some(s) = pool.slots.get_mut(0) {
            s.role = "cc".into();
            s.assigned_bands.clear();
        }

        // SDR #1 → voice idle
        if let Some(s) = pool.slots.get_mut(1) {
            s.role = "voice_scan".into();
            s.assigned_bands.clear();
        }
        state.clear_voice_slot();

        // Remaining SDRs (3+) continue scanning
        if pool.slots.len() > 2 {
            let scan_bands = config.bands.clone();
            let alive_others: Vec<usize> = pool.slots.iter()
                .enumerate()
                .skip(2)
                .filter(|(_, s)| s.alive.load(std::sync::atomic::Ordering::Relaxed))
                .map(|(i, _)| i)
                .collect();
            if !alive_others.is_empty() {
                let dist = pool::distribute_segments(
                    &scan_bands, alive_others.len(),
                    &pool.band_defs, pool.sample_rate,
                );
                for (di, &si) in alive_others.iter().enumerate() {
                    if let Some(ranges) = dist.get(di) {
                        if let Some(slot) = pool.slots.get_mut(si) {
                            let band_keys: Vec<String> = {
                                let mut keys: Vec<String> = ranges.iter().map(|r| r.key.clone()).collect();
                                keys.sort();
                                keys.dedup();
                                keys
                            };
                            slot.assigned_bands = band_keys;
                            slot.role = "scan".into();
                            let _ = slot.scan_cmd_tx.send(
                                rf_scan::ScanCommand::SetBandRanges(ranges.clone()),
                            );
                            let _ = slot.scan_cmd_tx.send(rf_scan::ScanCommand::Start);
                        }
                    }
                }
            }
        }

        state.set_monitor_freq(Some(cc_freq_mhz));
        state.set_sdr_role("network_scan");
        state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));

        // Load priority + department caches
        if let Ok(cache) = state.db().load_priority_cache("Portland") {
            prev.priority_cache = cache;
            tracing::info!("Scanner: loaded {} TG priorities", prev.priority_cache.len());
        }
        if let Ok(cache) = state.db().load_dept_cache("Portland") {
            prev.dept_cache = cache;
            tracing::info!("Scanner: loaded {} TG departments", prev.dept_cache.len());
        }

        prev.cc_last_valid_tsbk = std::time::Instant::now();
        prev.cc_hunt_active = true;
        prev.cc_freq = Some(cc_freq_mhz);
        prev.scan_active = true;
    } else if pool.slots.len() >= 2 {
        tracing::warn!("Network scanner: no CC freq set — disabling");
        state.update_config(|c| c.network_scan_active = false);
    } else {
        tracing::warn!("Network scanner requires >= 2 SDRs ({} present) — disabling", pool.slots.len());
        state.update_config(|c| c.network_scan_active = false);
    }
}

fn stop_network_scanner(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    let mut pool = pool.lock().unwrap();
    tracing::info!("Network scanner STOP — returning to scan");

    for slot in &mut pool.slots {
        let _ = slot.dsp_cmd_tx.send(dsp_bridge::DspCommand::ScanMode);
        slot.role = "scan".into();
    }

    pool.redistribute(&config.bands);
    state.clear_voice_slot();
    state.set_monitor_freq(None);
    state.set_squelch_open(false);
    state.set_sdr_role(if config.scanning { "scan" } else { "idle" });
    state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));

    if config.scanning {
        pool.broadcast_scan_cmd(|| rf_scan::ScanCommand::Start);
    }
    prev.scan_active = false;
}

fn check_voice_idle_timeout(state: &rf_web::AppState) {
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if let Some(slot) = state.voice_slot() {
        if slot.active && slot.last_grant_epoch > 0.0
            && (now_epoch - slot.last_grant_epoch) > 5.0
        {
            tracing::debug!(
                "Scanner: voice slot idle timeout (TG {:?})",
                slot.current_tgid
            );
            state.clear_voice_slot();
        }
    }
}

fn check_cc_hunt(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    let cc_dwell = config.network_scan_cc_dwell;
    if prev.cc_last_valid_tsbk.elapsed().as_secs_f64() > cc_dwell {
        let config = state.config();
        let cc_list = &config.network_scan_cc_list;
        if cc_list.len() > 1 {
            let next_idx = (config.network_scan_cc_index + 1) % cc_list.len();
            let next_freq = cc_list[next_idx];

            tracing::info!(
                "CC hunt: switching to {:.4} MHz (idx {}/{})",
                next_freq, next_idx + 1, cc_list.len()
            );

            let pool = pool.lock().unwrap();
            if let Some(cc_slot) = pool.slots.get(0) {
                let freq_hz = next_freq * 1_000_000.0;
                let sdr_center = freq_hz + CC_DC_OFFSET_HZ;
                let _ = cc_slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetFreq(sdr_center));
                let _ = cc_slot.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::SetFrequency {
                        center_freq: sdr_center,
                        target_freq: freq_hz,
                    },
                );
            }
            drop(pool);

            state.update_config(|c| {
                c.network_scan_cc_freq = Some(next_freq);
                c.network_scan_cc_index = next_idx;
            });
            state.set_monitor_freq(Some(next_freq));

            prev.cc_last_valid_tsbk = std::time::Instant::now();
            prev.cc_hunt_active = true;
            prev.cc_freq = Some(next_freq);
        }
    }
}

fn check_cc_freq_change(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    prev: &mut PrevState,
) {
    let current_cc = state.config().network_scan_cc_freq;
    if current_cc != prev.cc_freq && current_cc.is_some() {
        let cc_mhz = current_cc.unwrap();
        let cc_hz = cc_mhz * 1_000_000.0;
        let sdr_center = cc_hz + CC_DC_OFFSET_HZ;
        let pool = pool.lock().unwrap();
        if let Some(cc_slot) = pool.slots.get(0) {
            let _ = cc_slot.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetFreq(sdr_center));
            let _ = cc_slot.dsp_cmd_tx.send(
                dsp_bridge::DspCommand::SetFrequency {
                    center_freq: sdr_center,
                    target_freq: cc_hz,
                },
            );
        }
        drop(pool);
        state.set_monitor_freq(Some(cc_mhz));
        prev.cc_last_valid_tsbk = std::time::Instant::now();
        prev.cc_hunt_active = true;
        prev.cc_freq = current_cc;
        tracing::info!("CC changed to {:.4} MHz", cc_mhz);
    }
}

// ---------------------------------------------------------------------------
// Protocol metadata draining (RDS, P25)
// ---------------------------------------------------------------------------

fn drain_rds(
    state: &rf_web::AppState,
    rds_rx: &mpsc::Receiver<String>,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) {
    loop {
        match rds_rx.try_recv() {
            Ok(rds_json) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&rds_json) {
                    // Emit to SIEM pipeline
                    let record = event_ingestion::rds_to_log_record(&val);
                    pipeline.ingest(record);
                    // Existing broadcast
                    let _ = state.broadcast_protocol(std::sync::Arc::new(val));
                }
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

fn drain_p25(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    p25_rx: &mpsc::Receiver<String>,
    prev: &mut PrevState,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) {
    loop {
        match p25_rx.try_recv() {
            Ok(p25_json) => {
                if let Ok(mut msg) = serde_json::from_str::<serde_json::Value>(&p25_json) {
                    let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("").to_owned();

                    // CQPSK status — CC hunt decisions
                    if msg_type == "cqpsk_status" {
                        handle_cqpsk_status(&msg, prev);
                    }

                    // RF fingerprint — store to DB + UID correlation
                    if msg_type == "fingerprint" {
                        let cfo = msg.get("cfo_hz").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let iq_amp = msg.get("iq_amplitude_imbal").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let iq_phase = msg.get("iq_phase_imbal").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let power = msg.get("avg_power_db").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let variance = msg.get("power_variance").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let samples = msg.get("sample_count").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let freq_mhz = msg.get("freq_mhz").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let fp_config = state.config();
                        state.inc_fingerprint();
                        if let Ok((fp_db_id, capture_count)) = state.db().upsert_radio_fingerprint(
                            cfo, iq_amp, iq_phase, power, variance, samples, freq_mhz,
                            fp_config.fp_cfo_bucket_hz, fp_config.fp_iq_resolution,
                        ) {
                            // Reconstruct fingerprint_id for correlation
                            let cfo_bucket = if fp_config.fp_cfo_bucket_hz > 0.0 { (cfo / fp_config.fp_cfo_bucket_hz).round() as i64 } else { 0 };
                            let iq_bucket = if fp_config.fp_iq_resolution > 0.0 { (iq_amp / fp_config.fp_iq_resolution).round() as i64 } else { 0 };
                            let phase_bucket = if fp_config.fp_iq_resolution > 0.0 { (iq_phase / fp_config.fp_iq_resolution).round() as i64 } else { 0 };
                            let fp_id = format!("FP-{:+06}:{:04}:{:04}", cfo_bucket, iq_bucket, phase_bucket);

                            let operation_id = fp_config.active_operation_id;

                            // UID correlation: look up recent grant on this frequency
                            if freq_mhz > 0.0 {
                                if let Ok(Some(grant)) = state.db().find_recent_grant_by_freq(freq_mhz, fp_config.fp_grant_window_sec) {
                                    if let Some(uid) = grant.get("uid").and_then(|v| v.as_i64()) {
                                        let uid = uid as i32;
                                        let tgid = grant.get("tgid").and_then(|v| v.as_i64()).map(|v| v as i32);
                                        let system = grant.get("system").and_then(|v| v.as_str());
                                        if let Ok(_) = state.db().link_uid_to_fingerprint(uid, &fp_id, tgid, system) {
                                            tracing::info!(
                                                uid = uid, fp_id = %fp_id,
                                                tgid = ?tgid, freq_mhz = freq_mhz,
                                                "UID linked to fingerprint"
                                            );
                                            // SIGEX: check for UID-FP mismatch (multiple physical radios)
                                            if let Ok(fp_list) = state.db().get_fingerprints_for_uid(uid) {
                                                if fp_list.len() > 1 {
                                                    let _ = state.db().insert_sigex_event(
                                                        "fingerprint", "uid_fp_mismatch", "warning",
                                                        &format!("UID {} associated with {} distinct fingerprints - possible radio swap", uid, fp_list.len()),
                                                        None, None, None, Some(uid), Some(freq_mhz), None, operation_id,
                                                    );
                                                    tracing::warn!(uid = uid, fp_count = fp_list.len(), "UID associated with multiple fingerprints");
                                                    let fp_ids: Vec<&str> = fp_list.iter()
                                                        .filter_map(|f| f.get("fingerprint_id").and_then(|v| v.as_str()))
                                                        .collect();
                                                    let _ = state.broadcast_protocol(std::sync::Arc::new(serde_json::json!({
                                                        "type": "sigex", "subtype": "uid_fp_mismatch",
                                                        "uid": uid, "fingerprint_ids": fp_ids, "freq_mhz": freq_mhz,
                                                    })));
                                                    // SIEM
                                                    pipeline.ingest(event_ingestion::sigex_to_log_record(
                                                        "uid_fp_mismatch",
                                                        &format!("UID {} associated with {} distinct fingerprints", uid, fp_list.len()),
                                                        Some(freq_mhz), Some(uid as u32),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // SIGEX: new emitter detection (first observation)
                            if capture_count == 1 {
                                let _ = state.db().insert_sigex_event(
                                    "fingerprint", "new_emitter", "info",
                                    &format!("New emitter {} on {:.4} MHz (CFO={:.1}Hz)", fp_id, freq_mhz, cfo),
                                    None, None, None, None, Some(freq_mhz), None, operation_id,
                                );
                                tracing::info!(fp_id = %fp_id, freq_mhz = freq_mhz, cfo_hz = cfo, "New emitter detected");
                                let _ = state.broadcast_protocol(std::sync::Arc::new(serde_json::json!({
                                    "type": "sigex", "subtype": "new_emitter",
                                    "fingerprint_id": fp_id, "freq_mhz": freq_mhz, "cfo_hz": cfo,
                                })));
                                // SIEM
                                pipeline.ingest(event_ingestion::sigex_to_log_record(
                                    "new_emitter",
                                    &format!("New emitter {} on {:.4} MHz (CFO={:.1}Hz)", fp_id, freq_mhz, cfo),
                                    Some(freq_mhz), None,
                                ));
                            }

                            // SIGEX: emitter reappearance (gap > configured hours)
                            if capture_count > 1 {
                                if let Ok(Some(existing)) = state.db().get_fingerprint_by_fp_id(&fp_id) {
                                    if let Some(last_seen) = existing.get("last_seen").and_then(|v| v.as_str()) {
                                        // Parse last_seen and compare with now
                                        if let Ok(last) = chrono::NaiveDateTime::parse_from_str(last_seen, "%Y-%m-%d %H:%M:%S") {
                                            let now = chrono::Utc::now().naive_utc();
                                            let gap_hours = (now - last).num_hours();
                                            if gap_hours >= fp_config.fp_reappearance_hours {
                                                let _ = state.db().insert_sigex_event(
                                                    "fingerprint", "emitter_reappearance", "info",
                                                    &format!("Emitter {} reappeared after {}h absence", fp_id, gap_hours),
                                                    None, None, None, None, Some(freq_mhz), None, operation_id,
                                                );
                                                tracing::info!(fp_id = %fp_id, gap_hours = gap_hours, "Emitter reappeared after absence");
                                                let _ = state.broadcast_protocol(std::sync::Arc::new(serde_json::json!({
                                                    "type": "sigex", "subtype": "emitter_reappearance",
                                                    "fingerprint_id": fp_id, "gap_hours": gap_hours, "freq_mhz": freq_mhz,
                                                })));
                                                // SIEM
                                                pipeline.ingest(event_ingestion::sigex_to_log_record(
                                                    "emitter_reappearance",
                                                    &format!("Emitter {} reappeared after {}h absence", fp_id, gap_hours),
                                                    Some(freq_mhz), None,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            let _ = fp_db_id; // suppress unused warning
                        }

                        // Periodically refresh cached DB stats (every 30s)
                        if prev.last_fp_stats_refresh.elapsed() >= std::time::Duration::from_secs(30) {
                            prev.last_fp_stats_refresh = std::time::Instant::now();
                            if let Ok(n) = state.db().count_uid_fingerprint_links() {
                                state.set_uid_links_total(n as u64);
                            }
                            if let Ok(n) = state.db().count_distinct_fingerprints() {
                                state.set_emitters_unique(n as u64);
                            }
                        }
                    }

                    if msg_type == "tsbk" {
                        // Valid TSBK → reset CC hunt timer
                        prev.cc_last_valid_tsbk = std::time::Instant::now();
                        if prev.cc_hunt_active {
                            let current_cc = state.config().network_scan_cc_freq;
                            tracing::info!(
                                "CC hunt: LOCKED to {:.4} MHz — valid TSBK received",
                                current_cc.unwrap_or(0.0)
                            );
                            prev.cc_hunt_active = false;
                        }

                        enrich_tsbk(state, &mut msg);
                    }

                    // SIEM: emit P25 events as LogRecords for session correlation
                    if msg_type == "tsbk" {
                        if let Some(record) = event_ingestion::tsbk_to_log_record(&msg) {
                            pipeline.ingest(record);
                        }
                    } else if msg_type == "p25" {
                        let record = event_ingestion::p25_metadata_to_log_record(&msg);
                        pipeline.ingest(record);
                    }

                    // Broadcast filtered TSBK events
                    if msg_type == "tsbk" {
                        let ptype = msg.get("payload")
                            .and_then(|p| p.get("type"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        match ptype {
                            "GroupVoiceGrant" | "GroupVoiceUpdate"
                            | "UnitVoiceGrant" | "NetworkStatusBroadcast"
                            | "RfssStatusBroadcast" => {
                                let _ = state.broadcast_protocol(std::sync::Arc::new(msg.clone()));
                            }
                            _ => {}
                        }
                    } else {
                        let _ = state.broadcast_protocol(std::sync::Arc::new(msg.clone()));
                    }

                    // Scanner voice follow
                    if msg_type == "tsbk" {
                        handle_voice_grant(state, pool, &msg, prev);
                    }
                } else {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&p25_json) {
                        let _ = state.broadcast_protocol(std::sync::Arc::new(val));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

fn handle_cqpsk_status(msg: &serde_json::Value, prev: &mut PrevState) {
    let locked = msg.get("locked").and_then(|v| v.as_bool()).unwrap_or(false);
    let tsbks = msg.get("tsbks_decoded").and_then(|v| v.as_u64()).unwrap_or(0);
    let sync_count = msg.get("sync_nid_count").and_then(|v| v.as_u64()).unwrap_or(0);

    let dibits = msg.get("dibits_fed").and_then(|v| v.as_u64()).unwrap_or(0);
    let events = msg.get("p25_events").and_then(|v| v.as_u64()).unwrap_or(0);
    tracing::info!(
        "P25 pipeline: locked={}, dibits={}, events={}, tsbks={}, syncs={}",
        locked, dibits, events, tsbks, sync_count
    );

    if locked || tsbks > 0 {
        prev.cc_last_valid_tsbk = std::time::Instant::now();
    } else if sync_count > prev.sync_count {
        let new_syncs = sync_count - prev.sync_count;
        prev.cc_last_valid_tsbk = std::time::Instant::now();
        tracing::info!(
            "CC hunt: {} new frame syncs detected, resetting dwell timer",
            new_syncs
        );
    } else if prev.cc_hunt_active {
        let elapsed = prev.cc_last_valid_tsbk.elapsed().as_secs_f64();
        if elapsed > 1.5 {
            tracing::debug!(
                "CC hunt: no signal after {:.1}s — advancing",
                elapsed
            );
            prev.cc_last_valid_tsbk = std::time::Instant::now()
                - std::time::Duration::from_secs(100);
        }
    }
    prev.sync_count = sync_count;
}

fn enrich_tsbk(state: &rf_web::AppState, msg: &mut serde_json::Value) {
    if let Some(payload) = msg.get("payload").cloned() {
        let ptype = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if ptype == "ChannelParamsUpdate" {
            if let (Some(id), Some(base), Some(spacing)) = (
                payload.get("id").and_then(|v| v.as_u64()),
                payload.get("base_freq_hz").and_then(|v| v.as_u64()),
                payload.get("spacing_hz").and_then(|v| v.as_u64()),
            ) {
                state.set_channel_params(id as u8, rf_web::ChannelParams {
                    base_freq_hz: base as u32,
                    spacing_hz: spacing as u32,
                });
            }
        }

        if ptype == "GroupVoiceGrant" || ptype == "UnitVoiceGrant" {
            let ch_id = payload.get("channel_id").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let ch_num = payload.get("channel_num").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            if let Some(vf) = state.resolve_voice_freq(ch_id, ch_num) {
                msg["payload"]["voice_freq"] = serde_json::json!(vf);
            }
        }

        if ptype == "GroupVoiceUpdate" {
            if let Some(updates) = payload.get("updates").and_then(|u| u.as_array()).cloned() {
                let mut enriched = updates;
                for entry in enriched.iter_mut() {
                    let ch_id = entry.get("channel_id").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                    let ch_num = entry.get("channel_num").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                    if let Some(vf) = state.resolve_voice_freq(ch_id, ch_num) {
                        entry["voice_freq"] = serde_json::json!(vf);
                    }
                }
                msg["payload"]["updates"] = serde_json::json!(enriched);
            }
        }
    }
}

/// Handle GroupVoiceGrant TSBK — tune voice SDR, priority preemption.
fn handle_voice_grant(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    msg: &serde_json::Value,
    prev: &mut PrevState,
) {
    if let Some(payload) = msg.get("payload") {
        let ptype = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ptype != "GroupVoiceGrant" {
            return;
        }

        let config = state.config();
        if !config.network_scan_active {
            return;
        }

        let grant_tg = payload.get("talkgroup")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let grant_uid = payload.get("src_unit")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let should_follow = config.network_scan_mode == "id_search"
            || config.network_scan_tgids.contains(&grant_tg);

        if !should_follow || grant_tg == 0 {
            return;
        }

        let ch_id = payload.get("channel_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;
        let ch_num = payload.get("channel_num")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;

        let Some(voice_freq) = state.resolve_voice_freq(ch_id, ch_num) else {
            tracing::debug!(
                "Scanner: grant for TG {} but can't resolve ch_id={} ch_num={} (params: {})",
                grant_tg, ch_id, ch_num, state.channel_params_count()
            );
            return;
        };

        // Dept hold filter
        if config.network_scan_mode == "id_scan" {
            if let Some(hold_dept) = &config.network_scan_dept_hold {
                let tg_dept = prev.dept_cache.get(&grant_tg);
                if tg_dept.map_or(true, |d| d != hold_dept.as_str()) {
                    return;
                }
            }
        }

        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let current_slot = state.voice_slot();
        let pool = pool.lock().unwrap();
        let grant_prio = prev.priority_cache.get(&grant_tg).copied().unwrap_or(0);
        let grant_effective = if grant_prio == 0 { 255u8 } else { grant_prio };

        let should_tune = match current_slot.as_ref() {
            Some(slot) if slot.active && slot.current_tgid == Some(grant_tg) => {
                // Same TG — update timestamp/uid, retune if freq changed
                let freq_changed = slot.current_freq
                    .map_or(true, |f| (f - voice_freq).abs() > 0.001);
                state.update_voice_slot(|s| {
                    s.current_freq = Some(voice_freq);
                    s.current_uid = grant_uid;
                    s.last_grant_epoch = now_epoch;
                });
                if freq_changed {
                    tracing::info!("Scanner: retune TG {} → {:.4} MHz", grant_tg, voice_freq);
                }
                freq_changed
            }
            Some(slot) if !slot.active => {
                // Slot idle — assign directly
                assign_voice_slot(state, grant_tg, voice_freq, grant_uid, now_epoch, grant_prio);
                tracing::info!(
                    "Scanner: assign TG {} → {:.4} MHz (P{})",
                    grant_tg, voice_freq, grant_prio
                );
                true
            }
            Some(slot) => {
                // Slot busy with different TG — priority preemption
                let current_prio = slot.priority;
                let current_effective = if current_prio == 0 { 255u8 } else { current_prio };
                if grant_effective < current_effective {
                    let prev_tg = slot.current_tgid;
                    assign_voice_slot(state, grant_tg, voice_freq, grant_uid, now_epoch, grant_prio);
                    tracing::info!(
                        "Scanner: preempt TG {:?} (P{}) → TG {} (P{}) at {:.4} MHz",
                        prev_tg, current_prio, grant_tg, grant_prio, voice_freq
                    );
                    true
                } else {
                    false
                }
            }
            None => {
                // No slot at all — assign
                assign_voice_slot(state, grant_tg, voice_freq, grant_uid, now_epoch, grant_prio);
                tracing::info!(
                    "Scanner: assign TG {} → {:.4} MHz (P{})",
                    grant_tg, voice_freq, grant_prio
                );
                true
            }
        };

        if should_tune {
            if let Some(voice_sdr) = pool.slots.get(1) {
                let vfh = voice_freq * 1_000_000.0;
                let vfh_center = vfh + CC_DC_OFFSET_HZ;
                let _ = voice_sdr.sdr_cmd_tx.send(rf_sdr::SdrCommand::SetFreq(vfh_center));
                let _ = voice_sdr.dsp_cmd_tx.send(
                    dsp_bridge::DspCommand::MonitorMode {
                        center_freq: vfh_center,
                        target_freq: vfh,
                        mode: "P25".into(),
                        squelch: config.squelch,
                    },
                );
            }
        }
    }
}

fn assign_voice_slot(
    state: &rf_web::AppState,
    tgid: u32,
    freq: f64,
    uid: Option<u32>,
    epoch: f64,
    priority: u8,
) {
    state.set_voice_slot(Some(rf_web::VoiceSlotState {
        slot_index: 0,
        current_tgid: Some(tgid),
        current_freq: Some(freq),
        current_uid: uid,
        last_grant_epoch: epoch,
        active: true,
        priority,
    }));
}

// ---------------------------------------------------------------------------
// Quarantine detection + sweep rebalance
// ---------------------------------------------------------------------------

fn check_quarantine(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) {
    let mut pool = pool.lock().unwrap();
    let current_quarantine: Vec<bool> = pool.slots.iter()
        .map(|s| s.quarantine.load(std::sync::atomic::Ordering::Relaxed))
        .collect();
    if current_quarantine != prev.quarantine {
        for (i, (&prev_q, &curr_q)) in prev.quarantine.iter().zip(current_quarantine.iter()).enumerate() {
            if prev_q != curr_q {
                let key = pool.slots.get(i).map(|s| s.device_key.as_str()).unwrap_or("?");
                if curr_q {
                    tracing::warn!(
                        "Device {} quarantined — redistributing bands",
                        key
                    );
                    // SIEM: system.sdr.disconnect
                    pipeline.ingest(event_ingestion::sdr_disconnect_record(key));
                } else {
                    tracing::info!(
                        "Device {} unquarantined — redistributing bands",
                        key
                    );
                    // SIEM: system.sdr.connect (recovery)
                    pipeline.ingest(event_ingestion::sdr_connect_record(key, "recovered"));
                }
            }
        }
        prev.quarantine = current_quarantine;
        pool.redistribute(&config.bands);
        state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));
    }
}

fn check_sweep_balance(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    if prev.last_rebalance.elapsed() >= std::time::Duration::from_secs(30) && config.scanning {
        prev.last_rebalance = std::time::Instant::now();
        let mut pool = pool.lock().unwrap();
        let sweep_times: Vec<u64> = pool.slots.iter()
            .filter(|s| {
                s.alive.load(std::sync::atomic::Ordering::Relaxed)
                    && !s.quarantine.load(std::sync::atomic::Ordering::Relaxed)
                    && s.role == "scan"
            })
            .map(|s| {
                s.scan_status.read()
                    .map(|st| st.band_sweep_ms.values().sum())
                    .unwrap_or(0)
            })
            .collect();
        let all_have_data = sweep_times.iter().all(|&t| t > 0);
        if all_have_data {
            if let (Some(&max), Some(&min)) = (sweep_times.iter().max(), sweep_times.iter().min()) {
                if min > 0 && max > min * 2 {
                    tracing::info!(
                        "Sweep imbalance {}ms vs {}ms — redistributing",
                        max, min
                    );
                    pool.redistribute(&config.bands);
                    state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));
                }
            }
        }
    }
}

/// Poll the antenna→device assignment map from DB every 10s.
/// If it changed, update the pool and trigger redistribution.
fn check_antenna_map(
    state: &rf_web::AppState,
    pool: &Arc<Mutex<pool::DevicePool>>,
    config: &rf_web::AppConfig,
    prev: &mut PrevState,
) {
    if prev.last_antenna_check.elapsed() < std::time::Duration::from_secs(10) {
        return;
    }
    prev.last_antenna_check = std::time::Instant::now();

    let new_map: HashMap<String, (f64, f64)> = match state.db().get_antenna_map() {
        Ok(full) => {
            // Convert from serial → (antenna_id, min, max) to serial → (min, max)
            full.into_iter().map(|(serial, (_id, min, max))| (serial, (min, max))).collect()
        }
        Err(e) => {
            tracing::warn!("Failed to query antenna map: {}", e);
            return;
        }
    };

    if new_map != prev.antenna_map {
        tracing::info!("Antenna map changed ({} assignments) — updating pool", new_map.len());
        prev.antenna_map = new_map.clone();
        let mut pool = pool.lock().unwrap();
        pool.set_antenna_map(new_map);
        if config.scanning {
            pool.redistribute(&config.bands);
            state.set_sdr_slots(pool.to_slot_statuses(&device_names(state)));
        }
    }
}

// ---------------------------------------------------------------------------
// Spectrum frame processing + signal logging
// ---------------------------------------------------------------------------

/// Minimum interval between spectrum broadcasts per band (66ms = 15fps).
/// Source-side throttle prevents wasted serialization/broadcast of frames
/// that the events bridge would drop anyway.
const SPECTRUM_THROTTLE_MS: u64 = 66;

/// Process one spectrum frame. Returns true if data was consumed, false if empty.
/// The spectrum broadcast is throttled to 15fps per band at the source, but
/// signal detection and stats logging run on every frame unconditionally.
fn process_spectrum_tick(
    state: &rf_web::AppState,
    frame_rx: &mpsc::Receiver<rf_scan::SpectrumFrame>,
    prev: &mut PrevState,
    pipeline: &rf_events::pipeline::IngestionPipeline,
) -> bool {
    match frame_rx.try_recv() {
        Ok(frame) => {
            state.increment_sweeps();
            let coords = state.receiver_coords();

            // Source-side spectrum throttle: only broadcast if 66ms has
            // elapsed since last emission for this band. This prevents
            // ~80% of frames from being serialized, broadcast, cloned,
            // and then dropped by the events bridge.
            let now = std::time::Instant::now();
            let throttle_interval = std::time::Duration::from_millis(SPECTRUM_THROTTLE_MS);
            let should_broadcast = match prev.last_spectrum_emit.get(&frame.band) {
                Some(last) => now.duration_since(*last) >= throttle_interval,
                None => true,
            };

            if should_broadcast {
                prev.last_spectrum_emit.insert(frame.band.clone(), now);
                let spectrum_json = serde_json::json!({
                    "type": "spectrum",
                    "band": frame.band,
                    "band_start": frame.band_start,
                    "band_end": frame.band_end,
                    "freqs": frame.freqs,
                    "powers": frame.powers,
                    "signals": frame.signals,
                    "noise_floor": frame.noise_floor,
                    "segment_noise_floors": frame.segment_noise_floors,
                    "receiver_lat": if coords.lat != 0.0 { Some(coords.lat) } else { None::<f64> },
                    "receiver_lon": if coords.lon != 0.0 { Some(coords.lon) } else { None::<f64> },
                });
                let _ = state.broadcast_spectrum(std::sync::Arc::new(spectrum_json));
            }

            // Stats logging and signal detection run on EVERY frame,
            // regardless of whether the spectrum was broadcast.
            let config = state.config();
            if !frame.powers.is_empty() {
                log_spectrum_stats(state, &frame, &config);
            }

            // Phase 1 (reads): resolve channels + broadcast, collect writes
            let mut signal_writes = Vec::with_capacity(frame.signals.len());
            for sig in &frame.signals {
                let channel = state.db().lookup_channel_by_freq(sig.freq).ok().flatten();
                let mut signal_json = serde_json::json!({
                    "type": "signal",
                    "freq": sig.freq,
                    "power": sig.power,
                    "cls": sig.cls,
                    "name": sig.name,
                    "band": sig.band,
                    "mode": sig.mode,
                    "receiver_lat": if coords.lat != 0.0 { Some(coords.lat) } else { None::<f64> },
                    "receiver_lon": if coords.lon != 0.0 { Some(coords.lon) } else { None::<f64> },
                });
                if let Some(ref ch) = channel {
                    signal_json["channel_id"] = serde_json::json!(ch.id);
                    signal_json["channel_label"] = serde_json::json!(ch.label);
                    signal_json["channel_tag"] = serde_json::json!(ch.tag);
                    signal_json["channel_hits"] = serde_json::json!(ch.total_hits);
                    signal_json["channel_first_seen"] = serde_json::json!(ch.first_seen);
                    signal_json["encryption_seen"] = serde_json::json!(ch.encryption_seen);
                }
                let _ = state.broadcast_spectrum(std::sync::Arc::new(signal_json));
                // SIEM: emit signal detection
                pipeline.ingest(event_ingestion::signal_to_log_record(
                    sig, &frame.band, &frame.device_key,
                ));
                signal_writes.push(rf_db::SignalWrite {
                    freq: sig.freq,
                    power: sig.power as f64,
                    name: sig.name.clone(),
                    cls: sig.cls.clone(),
                    band: sig.band.clone(),
                    mode: sig.mode.clone(),
                    channel_id: channel.as_ref().map(|ch| ch.id),
                });
            }
            // Phase 2 (write batch): single lock, single transaction
            if !signal_writes.is_empty() {
                let op_id = state.config().active_operation_id;
                if let Err(e) = state.db().batch_process_signals(&signal_writes, op_id) {
                    tracing::warn!("Batch signal write failed: {}", e);
                }
            }
            true
        }
        Err(mpsc::TryRecvError::Empty) => false,
        Err(mpsc::TryRecvError::Disconnected) => {
            tracing::error!("Spectrum frame channel disconnected");
            false
        }
    }
}

fn log_spectrum_stats(
    state: &rf_web::AppState,
    frame: &rf_scan::SpectrumFrame,
    config: &rf_web::AppConfig,
) {
    let psd = &frame.powers;
    let psd_min = psd.iter().cloned().fold(f32::INFINITY, f32::min) as f64;
    let psd_max = psd.iter().cloned().fold(f32::NEG_INFINITY, f32::max) as f64;
    let psd_mean = (psd.iter().map(|&p| p as f64).sum::<f64>()) / psd.len() as f64;
    let mut sorted = psd.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = sorted[sorted.len() / 2] as f64;
    let peak_idx = psd.iter().enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let peak_power = psd[peak_idx] as f64;
    let peak_freq = if !frame.freqs.is_empty() {
        frame.freqs[peak_idx.min(frame.freqs.len() - 1)]
    } else { 0.0 };
    let center_freq = if !frame.freqs.is_empty() {
        frame.freqs[frame.freqs.len() / 2]
    } else { 0.0 };
    let dev_key = &frame.device_key;
    let dev_gain = config.per_device_gain.get(dev_key).copied().unwrap_or(config.gain);
    let dev_agc = config.per_device_agc.get(dev_key).copied().unwrap_or(false);
    let dev_ppm = config.per_device_ppm.get(dev_key).copied().unwrap_or(0.0);
    let _ = state.db().insert_debug_log(
        &frame.band, center_freq, dev_gain,
        config.threshold, noise_floor, peak_power,
        peak_freq, frame.signals.len() as i32,
        psd_min, psd_max, psd_mean,
        dev_key, 2_400_000.0,
        &config.modulation, config.snr_margin,
        dev_agc, dev_ppm,
    );
}

