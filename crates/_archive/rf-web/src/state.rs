use rf_db::Db;
use rf_scan::ScanStatus;
use rf_sdr::{SdrDeviceInfo, SdrStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::broadcast;

/// GPS position — core primitive carried by every event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsPosition {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
    pub heading_deg: Option<f64>,
    pub speed_mps: Option<f64>,
    pub accuracy_m: f64,
    pub hdop: Option<f64>,
    pub fix_type: String,       // "none", "2d", "3d", "dgps"
    pub satellite_count: u8,
    pub source: String,         // "external", "browser", "fixed", "simulation", "none"
}

/// Compact receiver tag stamped on every detection and frame.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ReceiverCoords {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f64,
    pub accuracy_m: f64,
}

/// Channel parameters learned from P25 ChannelParamsUpdate TSBKs.
/// Used to resolve channel_id + channel_num → voice frequency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelParams {
    pub base_freq_hz: u32,
    pub spacing_hz: u32,
}

impl ChannelParams {
    /// Resolve a channel number to frequency in MHz.
    pub fn resolve_freq(&self, channel_num: u16) -> f64 {
        let freq_hz = self.base_freq_hz as u64 + self.spacing_hz as u64 * channel_num as u64;
        freq_hz as f64 / 1_000_000.0
    }
}

/// Voice decoder slot state for network scanner mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSlotState {
    pub slot_index: usize,
    pub current_tgid: Option<u32>,
    pub current_freq: Option<f64>,
    pub current_uid: Option<u32>,
    pub last_grant_epoch: f64,
    pub active: bool,
    pub priority: u8,
}

impl Default for VoiceSlotState {
    fn default() -> Self {
        Self {
            slot_index: 0,
            current_tgid: None,
            current_freq: None,
            current_uid: None,
            last_grant_epoch: 0.0,
            active: false,
            priority: 0,
        }
    }
}

/// Per-device status for multi-SDR support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdrSlotStatus {
    /// Stable key: "rtlsdr:00000001"
    pub device_key: String,
    /// Human-readable label
    pub label: String,
    pub driver: String,
    pub serial: String,
    /// User-assigned display name (from DB sdr_devices table)
    pub user_name: String,
    /// "scan", "monitor", "idle", "failed"
    pub role: String,
    /// Whether the device is currently streaming
    pub alive: bool,
    /// Whether the device has been quarantined due to freq tune failures
    pub quarantined: bool,
    /// Whether the DSP pipeline is producing PSD data (IQ actually flowing)
    pub streaming: bool,
    /// Band keys assigned to this device
    pub assigned_bands: Vec<String>,
    pub sample_rate: f64,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    sdr_status: RwLock<SdrStatus>,
    sdr_devices: RwLock<Vec<SdrDeviceInfo>>,
    config: RwLock<AppConfig>,
    heartbeat_tx: broadcast::Sender<Arc<Value>>,
    protocol_tx: broadcast::Sender<Arc<Value>>,
    spectrum_tx: broadcast::Sender<Arc<Value>>,
    started: Instant,
    sweep_count: AtomicU64,
    sdr_alive: Arc<AtomicBool>,
    sdr_refresh: Arc<AtomicBool>,
    squelch_open: AtomicBool,
    monitor_freq: RwLock<Option<f64>>,
    gps_position: RwLock<Option<GpsPosition>>,
    scan_status: Arc<RwLock<ScanStatus>>,
    /// SDR role for single-SDR mode: "scan", "monitor", "sigex_cc", "idle"
    sdr_role: RwLock<String>,
    /// Per-device slot status for multi-SDR
    sdr_slots: RwLock<Vec<SdrSlotStatus>>,
    /// Per-device scan statuses: (device_key, scan_status_arc)
    scan_statuses: RwLock<Vec<(String, Arc<RwLock<ScanStatus>>)>>,
    /// Startup progress phase (empty string = startup complete)
    startup_phase: RwLock<String>,
    /// P25 channel parameter table: id → ChannelParams (from TSBK ChannelParamsUpdate)
    channel_params: RwLock<HashMap<u8, ChannelParams>>,
    /// Voice decoder slot state for network scanner mode (single voice SDR)
    voice_slot: RwLock<Option<VoiceSlotState>>,
    /// Global shutdown flag — signals all threads/tasks to exit cleanly
    shutdown: Arc<AtomicBool>,
    /// Recording engine status (active audio/IQ slots)
    recorder_status: RwLock<RecorderStatusData>,
    /// Fingerprint pipeline metrics
    fp_session_count: AtomicU64,
    uid_links_total: AtomicU64,
    emitters_unique: AtomicU64,
    /// Active alert highlights (expires after duration)
    alert_highlights: RwLock<Vec<AlertHighlight>>,
}

/// Lightweight recording status for heartbeat broadcast.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecorderStatusData {
    pub active_audio: Vec<serde_json::Value>,
    pub active_iq: Vec<serde_json::Value>,
}

/// An active alert highlight for the UI status ribbon.
#[derive(Debug, Clone, Serialize)]
pub struct AlertHighlight {
    /// Rule name for display
    pub rule_name: String,
    /// CSS-style color string (e.g., "#FF3333", "red")
    pub color: String,
    /// Priority label for ordering
    pub priority: String,
    /// When this highlight was created (nanoseconds since epoch)
    pub created_ns: u64,
    /// How long this highlight should be visible (seconds)
    pub duration_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mode: String,
    pub scanning: bool,
    pub freq: f64,
    pub bands: Vec<String>,
    pub gain: f64,
    pub threshold: f64,
    pub squelch: f64,
    pub modulation: String,
    pub volume: u8,
    pub muted: bool,
    pub debug_logging: bool,
    pub snr_margin: f64,
    pub persist_min_hits: u32,
    pub persist_window: u32,
    // GPS configuration
    pub gps_enabled: bool,
    pub gps_source: String,         // "external", "browser", "fixed", "simulation", "none"
    pub gps_port: String,
    pub gps_baud: u32,
    pub gps_update_hz: u8,
    pub fixed_lat: Option<f64>,
    pub fixed_lon: Option<f64>,
    pub fixed_alt_m: Option<f64>,
    // Per-device gain overrides (device_key → gain dB)
    #[serde(default)]
    pub per_device_gain: HashMap<String, f64>,
    // Per-band threshold overrides (band_key → dBFS)
    #[serde(default)]
    pub per_band_threshold: HashMap<String, f64>,
    // Network scanner (multi-TG P25 trunking)
    #[serde(default)]
    pub network_scan_active: bool,
    #[serde(default)]
    pub network_scan_cc_freq: Option<f64>,
    #[serde(default)]
    pub network_scan_tgids: Vec<u32>,
    /// "id_scan" (watched TGs only) or "id_search" (all TGs)
    #[serde(default = "default_scan_mode")]
    pub network_scan_mode: String,
    /// Department hold filter — only follow TGs in this department
    #[serde(default)]
    pub network_scan_dept_hold: Option<String>,
    /// CC hunting: ordered list of CC frequencies to try (MHz)
    #[serde(default)]
    pub network_scan_cc_list: Vec<f64>,
    /// Current CC hunt index into cc_list
    #[serde(default)]
    pub network_scan_cc_index: usize,
    /// CC hunt dwell time — seconds to wait on each CC before advancing (default 10)
    #[serde(default = "default_cc_dwell")]
    pub network_scan_cc_dwell: f64,
    // Per-device AGC toggle (device_key → enabled)
    #[serde(default)]
    pub per_device_agc: HashMap<String, bool>,
    // Per-device PPM correction (device_key → ppm)
    #[serde(default)]
    pub per_device_ppm: HashMap<String, f64>,
    // Per-device offset tuning toggle (device_key → enabled)
    #[serde(default)]
    pub per_device_offset_tuning: HashMap<String, bool>,
    // Demod filter bandwidth in Hz (0 = mode default)
    #[serde(default)]
    pub bandwidth_hz: f64,
    // Active collection site (set by geofence loop)
    #[serde(default)]
    pub active_site_id: Option<i64>,
    // Active site session ID (for closing on leave)
    #[serde(default)]
    pub active_site_session_id: Option<i64>,
    // Active operator identity (set via set_operator command)
    #[serde(default)]
    pub active_operator_id: Option<i64>,
    #[serde(default)]
    pub active_operator_callsign: Option<String>,
    // Active operation (set via start_operation / resume_operation)
    #[serde(default)]
    pub active_operation_id: Option<i64>,
    #[serde(default)]
    pub active_operation_name: Option<String>,
    // Active session within current operation
    #[serde(default)]
    pub active_session_id: Option<i64>,
    // Active operation profile ("test" or "live")
    #[serde(default)]
    pub active_operation_profile: Option<String>,
    // Auto-clip recording (per-transmission clips)
    #[serde(default)]
    pub auto_clip_enabled: bool,
    // Duck volume (0-100) — monitoring volume reduction during clip playback
    #[serde(default = "default_duck_volume")]
    pub duck_volume: u8,
    // Active observation count (set periodically by observation engine)
    #[serde(default)]
    pub active_observation_count: u32,
    // Fingerprint pipeline configuration
    #[serde(default = "default_fp_cfo_bucket_hz")]
    pub fp_cfo_bucket_hz: f64,
    #[serde(default = "default_fp_iq_resolution")]
    pub fp_iq_resolution: f64,
    #[serde(default = "default_fp_grant_window_sec")]
    pub fp_grant_window_sec: i64,
    #[serde(default = "default_fp_min_samples")]
    pub fp_min_samples: usize,
    #[serde(default = "default_fp_reappearance_hours")]
    pub fp_reappearance_hours: i64,
}

fn default_scan_mode() -> String { "id_scan".into() }
fn default_cc_dwell() -> f64 { 10.0 }
fn default_duck_volume() -> u8 { 100 }
fn default_fp_cfo_bucket_hz() -> f64 { 5.0 }
fn default_fp_iq_resolution() -> f64 { 0.0001 }
fn default_fp_grant_window_sec() -> i64 { 30 }
fn default_fp_min_samples() -> usize { 480 }
fn default_fp_reappearance_hours() -> i64 { 24 }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mode: "scan".into(),
            scanning: false,
            freq: 146.52,
            bands: vec![],
            gain: 20.0,
            threshold: -90.0,
            squelch: -60.0,
            modulation: "NFM".into(),
            volume: 80,
            muted: false,
            debug_logging: false,
            snr_margin: 8.0,
            persist_min_hits: 1,
            persist_window: 3,
            gps_enabled: true,
            gps_source: "external".into(),
            gps_port: String::new(),
            gps_baud: 9600,
            gps_update_hz: 1,
            fixed_lat: None,
            fixed_lon: None,
            fixed_alt_m: None,
            per_device_gain: HashMap::new(),
            per_band_threshold: HashMap::new(),
            network_scan_active: false,
            network_scan_cc_freq: Some(772.60625),
            network_scan_tgids: Vec::new(),
            network_scan_mode: "id_search".into(),
            network_scan_dept_hold: None,
            network_scan_cc_list: vec![
                772.60625,  // West Simulcast (site 21) — primary
                773.13125,  // West Simulcast alt
                770.85625,  // East Simulcast (site 22)
                773.48125,  // Goat Mountain (site 23)
                853.35,     // Cornelius Pass (site 24)
            ],
            network_scan_cc_index: 0,
            network_scan_cc_dwell: 3.0,
            per_device_agc: HashMap::new(),
            per_device_ppm: HashMap::new(),
            per_device_offset_tuning: HashMap::new(),
            bandwidth_hz: 0.0,
            active_site_id: None,
            active_site_session_id: None,
            active_operator_id: None,
            active_operator_callsign: None,
            active_operation_id: None,
            active_operation_name: None,
            active_session_id: None,
            active_operation_profile: None,
            auto_clip_enabled: false,
            duck_volume: 100,
            active_observation_count: 0,
            fp_cfo_bucket_hz: 5.0,
            fp_iq_resolution: 0.0001,
            fp_grant_window_sec: 30,
            fp_min_samples: 480,
            fp_reappearance_hours: 24,
        }
    }
}

impl AppState {
    pub fn new(db: Db, sdr_status: SdrStatus, sdr_devices: Vec<SdrDeviceInfo>) -> Self {
        let (heartbeat_tx, _) = broadcast::channel(64);
        let (protocol_tx, _) = broadcast::channel(4096);
        let (spectrum_tx, _) = broadcast::channel(2048);
        let sdr_alive = Arc::new(AtomicBool::new(sdr_status.detected));
        let sdr_refresh = Arc::new(AtomicBool::new(false));
        Self {
            inner: Arc::new(Inner {
                db,
                sdr_status: RwLock::new(sdr_status),
                sdr_devices: RwLock::new(sdr_devices),
                config: RwLock::new(AppConfig::default()),
                heartbeat_tx,
                protocol_tx,
                spectrum_tx,
                started: Instant::now(),
                sweep_count: AtomicU64::new(0),
                sdr_alive,
                sdr_refresh,
                squelch_open: AtomicBool::new(false),
                monitor_freq: RwLock::new(None),
                gps_position: RwLock::new(None),
                scan_status: Arc::new(RwLock::new(ScanStatus::default())),
                sdr_role: RwLock::new("idle".into()),
                sdr_slots: RwLock::new(Vec::new()),
                scan_statuses: RwLock::new(Vec::new()),
                startup_phase: RwLock::new(String::new()),
                channel_params: RwLock::new(HashMap::new()),
                voice_slot: RwLock::new(None),
                shutdown: Arc::new(AtomicBool::new(false)),
                recorder_status: RwLock::new(RecorderStatusData::default()),
                fp_session_count: AtomicU64::new(0),
                uid_links_total: AtomicU64::new(0),
                emitters_unique: AtomicU64::new(0),
                alert_highlights: RwLock::new(Vec::new()),
            }),
        }
    }

    // --- Shutdown ---

    /// Signal all threads/tasks to shut down.
    pub fn set_shutdown(&self, val: bool) {
        self.inner.shutdown.store(val, Ordering::Release);
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.inner.shutdown.load(Ordering::Acquire)
    }

    /// Get the shared shutdown flag Arc (for passing to threads).
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.shutdown)
    }

    pub fn db(&self) -> &Db { &self.inner.db }
    pub fn sdr_status(&self) -> SdrStatus {
        self.inner.sdr_status.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn set_sdr_status(&self, status: SdrStatus) {
        *self.inner.sdr_status.write().expect("sdr_status poisoned") = status;
    }
    pub fn sdr_devices(&self) -> Vec<SdrDeviceInfo> {
        self.inner.sdr_devices.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Update the device list (used by deferred SDR init).
    pub fn set_sdr_devices(&self, devices: Vec<SdrDeviceInfo>) {
        *self.inner.sdr_devices.write().expect("sdr_devices poisoned") = devices;
    }

    /// Set the sdr_alive flag directly (used by deferred SDR init).
    pub fn set_sdr_alive(&self, alive: bool) {
        self.inner.sdr_alive.store(alive, Ordering::Release);
    }

    /// Re-enumerate SoapySDR devices and update the device list.
    /// Also sets sdr_alive based on whether any devices were found.
    pub fn refresh_sdr_devices(&self) {
        let devices = rf_sdr::enumerate_all();
        let found = !devices.is_empty();
        tracing::info!("SDR refresh: {} device(s) detected", devices.len());
        for (i, d) in devices.iter().enumerate() {
            tracing::info!("  [{}] {} (serial: {}) {:?}", i, d.label, d.serial, d.args);
        }
        *self.inner.sdr_devices.write().expect("sdr_devices poisoned") = devices;
        // Update alive status and trigger reader reconnect
        if found {
            self.inner.sdr_refresh.store(true, Ordering::Release);
        }
    }

    /// Get the shared sdr_alive flag (for passing to SDR reader thread).
    pub fn sdr_alive(&self) -> Arc<AtomicBool> { Arc::clone(&self.inner.sdr_alive) }

    /// Get the shared sdr_refresh flag (for passing to SDR reader thread).
    pub fn sdr_refresh(&self) -> Arc<AtomicBool> { Arc::clone(&self.inner.sdr_refresh) }

    /// Check if the SDR device is currently alive/connected.
    pub fn is_sdr_alive(&self) -> bool {
        self.inner.sdr_alive.load(Ordering::Relaxed)
    }

    /// Trigger an SDR refresh/reconnect attempt.
    pub fn trigger_sdr_refresh(&self) {
        self.inner.sdr_refresh.store(true, Ordering::Release);
    }

    pub fn config(&self) -> AppConfig {
        self.inner.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update_config(&self, f: impl FnOnce(&mut AppConfig)) {
        let mut config = self.inner.config.write().expect("config poisoned");
        f(&mut config);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.inner.started.elapsed().as_secs()
    }

    // --- Heartbeat broadcast (1 Hz status) ---

    pub fn subscribe_heartbeat(&self) -> broadcast::Receiver<Arc<Value>> {
        self.inner.heartbeat_tx.subscribe()
    }

    pub fn broadcast_heartbeat(&self, msg: Arc<Value>) -> Result<usize, broadcast::error::SendError<Arc<Value>>> {
        self.inner.heartbeat_tx.send(msg)
    }

    // --- Protocol broadcast (P25/RDS/CQPSK events) ---

    pub fn subscribe_protocol(&self) -> broadcast::Receiver<Arc<Value>> {
        self.inner.protocol_tx.subscribe()
    }

    pub fn broadcast_protocol(&self, msg: Arc<Value>) -> Result<usize, broadcast::error::SendError<Arc<Value>>> {
        self.inner.protocol_tx.send(msg)
    }

    // --- Spectrum broadcast (high-rate) ---

    pub fn subscribe_spectrum(&self) -> broadcast::Receiver<Arc<Value>> {
        self.inner.spectrum_tx.subscribe()
    }

    pub fn broadcast_spectrum(&self, msg: Arc<Value>) -> Result<usize, broadcast::error::SendError<Arc<Value>>> {
        self.inner.spectrum_tx.send(msg)
    }

    // --- Monitor status ---

    pub fn set_squelch_open(&self, open: bool) {
        self.inner.squelch_open.store(open, Ordering::Relaxed);
    }

    pub fn is_squelch_open(&self) -> bool {
        self.inner.squelch_open.load(Ordering::Relaxed)
    }

    pub fn set_monitor_freq(&self, freq: Option<f64>) {
        *self.inner.monitor_freq.write().expect("monitor_freq poisoned") = freq;
    }

    pub fn monitor_freq(&self) -> Option<f64> {
        *self.inner.monitor_freq.read().unwrap_or_else(|e| e.into_inner())
    }

    // --- Sweep counter ---

    pub fn increment_sweeps(&self) {
        self.inner.sweep_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn sweep_count(&self) -> u64 {
        self.inner.sweep_count.load(Ordering::Relaxed)
    }

    // --- Fingerprint pipeline metrics ---

    pub fn inc_fingerprint(&self) {
        self.inner.fp_session_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn fp_session_count(&self) -> u64 {
        self.inner.fp_session_count.load(Ordering::Relaxed)
    }

    pub fn uid_links_total(&self) -> u64 {
        self.inner.uid_links_total.load(Ordering::Relaxed)
    }

    pub fn set_uid_links_total(&self, v: u64) {
        self.inner.uid_links_total.store(v, Ordering::Relaxed);
    }

    pub fn emitters_unique(&self) -> u64 {
        self.inner.emitters_unique.load(Ordering::Relaxed)
    }

    pub fn set_emitters_unique(&self, v: u64) {
        self.inner.emitters_unique.store(v, Ordering::Relaxed);
    }

    // --- Alert Highlights ---

    /// Push an alert highlight for the UI status ribbon.
    pub fn push_alert_highlight(&self, highlight: AlertHighlight) {
        let mut highlights = self.inner.alert_highlights.write()
            .unwrap_or_else(|e| e.into_inner());
        highlights.push(highlight);
    }

    /// Get active (non-expired) alert highlights, pruning expired ones.
    pub fn active_alert_highlights(&self) -> Vec<AlertHighlight> {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut highlights = self.inner.alert_highlights.write()
            .unwrap_or_else(|e| e.into_inner());
        highlights.retain(|h| {
            let elapsed_ns = now_ns.saturating_sub(h.created_ns);
            elapsed_ns < h.duration_sec * 1_000_000_000
        });
        highlights.clone()
    }

    // --- GPS ---

    /// Update the current GPS position.
    /// Rejects positions with out-of-range coordinates or NaN values.
    pub fn set_gps_position(&self, pos: GpsPosition) {
        if !pos.latitude.is_finite() || !pos.longitude.is_finite()
            || pos.latitude < -90.0 || pos.latitude > 90.0
            || pos.longitude < -180.0 || pos.longitude > 180.0
        {
            tracing::warn!(
                "GPS: rejecting invalid position lat={} lon={}",
                pos.latitude, pos.longitude
            );
            return;
        }
        *self.inner.gps_position.write().expect("gps_position poisoned") = Some(pos);
    }

    /// Get the current GPS position.
    pub fn gps_position(&self) -> Option<GpsPosition> {
        self.inner.gps_position.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Get compact receiver coordinates for stamping events.
    pub fn receiver_coords(&self) -> ReceiverCoords {
        match self.gps_position() {
            Some(pos) => ReceiverCoords {
                lat: pos.latitude,
                lon: pos.longitude,
                alt_m: pos.altitude_m.unwrap_or(0.0),
                accuracy_m: pos.accuracy_m,
            },
            None => ReceiverCoords::default(),
        }
    }

    // --- Scan status (shared with scan controller thread) ---

    /// Get the shared scan status Arc for passing to the scan controller thread.
    pub fn scan_status(&self) -> Arc<RwLock<ScanStatus>> {
        Arc::clone(&self.inner.scan_status)
    }

    /// Set the SDR role label (scan, monitor, idle, etc).
    pub fn set_sdr_role(&self, role: &str) {
        if let Ok(mut r) = self.inner.sdr_role.write() {
            *r = role.into();
        }
    }

    /// Get the current SDR role.
    pub fn sdr_role(&self) -> String {
        self.inner.sdr_role.read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    // --- Multi-SDR slot management ---

    /// Set the per-device slot statuses.
    pub fn set_sdr_slots(&self, slots: Vec<SdrSlotStatus>) {
        *self.inner.sdr_slots.write().expect("sdr_slots poisoned") = slots;
    }

    /// Get a snapshot of per-device slot statuses.
    pub fn sdr_slots(&self) -> Vec<SdrSlotStatus> {
        self.inner.sdr_slots.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Register per-device scan statuses for multi-device band_status().
    pub fn set_scan_statuses(&self, statuses: Vec<(String, Arc<RwLock<ScanStatus>>)>) {
        *self.inner.scan_statuses.write().expect("scan_statuses poisoned") = statuses;
    }

    /// Build band_status map: for each configured band, determine scanning/paused/idle.
    /// With multi-SDR, checks all per-device scan_statuses and slot assignments.
    fn band_status(&self) -> HashMap<String, String> {
        let config = self.config();
        let mode = &config.mode;
        let monitor_active = mode == "monitor" && self.monitor_freq().is_some();

        let all_bands = ["AM", "HF", "FM", "VHF", "FEDV", "BIII", "UHF", "GMRS", "P25"];
        let mut status = HashMap::new();

        // Try multi-SDR path: check slots and per-device scan_statuses
        let slots = self.sdr_slots();
        let scan_statuses = self.inner.scan_statuses.read()
            .unwrap_or_else(|e| e.into_inner());

        if !slots.is_empty() && !scan_statuses.is_empty() {
            // Multi-SDR: determine status per band from assigned device's scan_status
            for &band_key in &all_bands {
                let enabled = config.bands.contains(&band_key.to_string());
                if !enabled {
                    status.insert(band_key.to_string(), "idle".to_string());
                    continue;
                }
                if monitor_active || !config.scanning {
                    status.insert(band_key.to_string(), "paused".to_string());
                    continue;
                }

                // Find which slot has this band assigned
                let mut band_status_val = "paused";
                for slot in &slots {
                    if !slot.assigned_bands.contains(&band_key.to_string()) {
                        continue;
                    }
                    if !slot.alive {
                        band_status_val = "paused";
                        break;
                    }
                    // Find this device's scan_status
                    if let Some((_, scan_st)) = scan_statuses.iter()
                        .find(|(key, _)| *key == slot.device_key)
                    {
                        let st = scan_st.read().unwrap_or_else(|e| e.into_inner());
                        if st.scanning && st.current_band == band_key {
                            band_status_val = "scanning";
                        } else if st.scanning {
                            band_status_val = "queued";
                        } else {
                            band_status_val = "paused";
                        }
                    }
                    break;
                }
                status.insert(band_key.to_string(), band_status_val.to_string());
            }
        } else {
            // Single-SDR fallback: use the legacy scan_status
            let scan_st = self.inner.scan_status.read().unwrap_or_else(|e| e.into_inner());
            for &band_key in &all_bands {
                let enabled = config.bands.contains(&band_key.to_string());
                let st = if !enabled {
                    "idle"
                } else if monitor_active || !config.scanning {
                    "paused"
                } else if scan_st.scanning && scan_st.current_band == band_key {
                    "scanning"
                } else if scan_st.scanning {
                    "queued"
                } else {
                    "paused"
                };
                status.insert(band_key.to_string(), st.to_string());
            }
        }

        status
    }

    /// Set the startup progress phase text. Empty string = startup complete.
    pub fn set_startup_phase(&self, phase: &str) {
        *self.inner.startup_phase.write().expect("startup_phase poisoned") = phase.into();
    }

    /// Get the current startup phase text.
    pub fn startup_phase(&self) -> String {
        self.inner.startup_phase.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    // --- P25 Channel Parameter Table ---

    /// Store a channel parameter entry learned from TSBK ChannelParamsUpdate.
    pub fn set_channel_params(&self, id: u8, params: ChannelParams) {
        self.inner.channel_params.write().expect("channel_params poisoned").insert(id, params);
    }

    /// Resolve a (channel_id, channel_num) pair to a frequency in MHz.
    pub fn resolve_voice_freq(&self, channel_id: u8, channel_num: u16) -> Option<f64> {
        self.inner.channel_params.read().unwrap_or_else(|e| e.into_inner())
            .get(&channel_id)
            .map(|p| p.resolve_freq(channel_num))
    }

    /// Snapshot of all learned channel parameter entries.
    pub fn channel_params(&self) -> HashMap<u8, ChannelParams> {
        self.inner.channel_params.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Number of channel param entries learned.
    pub fn channel_params_count(&self) -> usize {
        self.inner.channel_params.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    // --- Voice slot management (network scanner — single voice SDR) ---

    /// Get a snapshot of the voice slot state.
    pub fn voice_slot(&self) -> Option<VoiceSlotState> {
        self.inner.voice_slot.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Set the voice slot state.
    pub fn set_voice_slot(&self, slot: Option<VoiceSlotState>) {
        *self.inner.voice_slot.write().expect("voice_slot poisoned") = slot;
    }

    /// Mutate the voice slot if present.
    pub fn update_voice_slot(&self, f: impl FnOnce(&mut VoiceSlotState)) {
        let mut slot = self.inner.voice_slot.write().expect("voice_slot poisoned");
        if let Some(ref mut s) = *slot {
            f(s);
        }
    }

    /// Clear the voice slot to None.
    pub fn clear_voice_slot(&self) {
        *self.inner.voice_slot.write().expect("voice_slot poisoned") = None;
    }

    // --- Recording status ---

    pub fn set_recorder_status(&self, status: RecorderStatusData) {
        *self.inner.recorder_status.write().expect("recorder_status poisoned") = status;
    }

    pub fn recorder_status(&self) -> RecorderStatusData {
        self.inner.recorder_status.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn status_message(&self) -> Arc<Value> {
        let config = self.config();
        let alive = self.is_sdr_alive();
        let gps = self.gps_position();
        let band_status = self.band_status();
        let sdr_role = self.sdr_role();
        let sdr_slots = self.sdr_slots();
        let startup_phase = self.startup_phase();
        let rec_status = self.recorder_status();
        // Aggregate band_sweep_ms from all scan_statuses
        let mut band_sweep_ms: HashMap<String, u64> = HashMap::new();
        {
            let scan_statuses = self.inner.scan_statuses.read()
                .unwrap_or_else(|e| e.into_inner());
            if !scan_statuses.is_empty() {
                for (_, ss) in scan_statuses.iter() {
                    if let Ok(st) = ss.read() {
                        for (k, &v) in &st.band_sweep_ms {
                            let entry = band_sweep_ms.entry(k.clone()).or_insert(v);
                            if v < *entry {
                                *entry = v;
                            }
                        }
                    }
                }
            } else {
                // Fallback to legacy single scan_status
                if let Ok(st) = self.inner.scan_status.read() {
                    band_sweep_ms = st.band_sweep_ms.clone();
                }
            }
        }
        let sdr_st = self.sdr_status();
        Arc::new(serde_json::json!({
            "type": "status",
            "mode": config.mode,
            "scanning": config.scanning,
            "freq": config.freq,
            "gain": config.gain,
            "squelch": config.squelch,
            "threshold": config.threshold,
            "modulation": config.modulation,
            "volume": config.volume,
            "muted": config.muted,
            "sdr_ok": alive,
            "sdr_driver": sdr_st.driver,
            "sdr_serial": sdr_st.serial,
            "sdr_sample_rate": sdr_st.sample_rate,
            "sdr_devices": self.sdr_devices().iter().map(|d| {
                let mut obj = serde_json::json!({
                    "driver": d.driver,
                    "label": d.label,
                    "serial": d.serial,
                });
                // Merge all SoapySDR args into the device object
                if let Some(map) = obj.as_object_mut() {
                    for (k, v) in &d.args {
                        map.entry(k.clone()).or_insert_with(|| serde_json::json!(v));
                    }
                }
                obj
            }).collect::<Vec<_>>(),
            "debug_logging": config.debug_logging,
            "snr_margin": config.snr_margin,
            "persist_min_hits": config.persist_min_hits,
            "persist_window": config.persist_window,
            "sweeps": self.sweep_count(),
            "uptime": self.uptime_secs(),
            "bands": config.bands,
            "monitor_freq": self.monitor_freq(),
            "squelch_open": self.is_squelch_open(),
            // Band scan status (per-band indicators)
            "band_status": band_status,
            "band_sweep_ms": band_sweep_ms,
            "sdr_role": sdr_role,
            "sdr_slots": sdr_slots,
            "per_device_gain": config.per_device_gain,
            "per_device_agc": config.per_device_agc,
            "per_device_ppm": config.per_device_ppm,
            "per_device_offset_tuning": config.per_device_offset_tuning,
            "bandwidth_hz": config.bandwidth_hz,
            "per_band_threshold": config.per_band_threshold,
            // GPS position fields (live — only present when position received)
            "gps_fix": gps.as_ref().map(|g| g.fix_type.as_str()).unwrap_or("none"),
            "gps_sats": gps.as_ref().map(|g| g.satellite_count).unwrap_or(0),
            "gps_lat": gps.as_ref().map(|g| g.latitude),
            "gps_lon": gps.as_ref().map(|g| g.longitude),
            "gps_alt_m": gps.as_ref().and_then(|g| g.altitude_m),
            "gps_heading_deg": gps.as_ref().and_then(|g| g.heading_deg),
            "gps_speed_mps": gps.as_ref().and_then(|g| g.speed_mps),
            "gps_accuracy_m": gps.as_ref().map(|g| g.accuracy_m),
            "gps_hdop": gps.as_ref().and_then(|g| g.hdop),
            "gps_source": gps.as_ref().map(|g| g.source.as_str()).unwrap_or("none"),
            // GPS config fields (always present — from AppConfig)
            "gps_config_enabled": config.gps_enabled,
            "gps_config_source": config.gps_source,
            "gps_config_port": config.gps_port,
            "gps_config_baud": config.gps_baud,
            "gps_fixed_lat": config.fixed_lat,
            "gps_fixed_lon": config.fixed_lon,
            "gps_fixed_alt_m": config.fixed_alt_m,
            "startup_phase": startup_phase,
            // Network scanner (2-SDR P25 trunking)
            "network_scan_active": config.network_scan_active,
            "network_scan_cc_freq": config.network_scan_cc_freq,
            "network_scan_tgids": config.network_scan_tgids,
            "network_scan_voice_slot": self.voice_slot(),
            "network_scan_channel_params": self.channel_params_count(),
            "network_scan_mode": config.network_scan_mode,
            "network_scan_dept_hold": config.network_scan_dept_hold,
            "network_scan_cc_list": config.network_scan_cc_list,
            "network_scan_cc_index": config.network_scan_cc_index,
            "network_scan_cc_dwell": config.network_scan_cc_dwell,
            // Active collection site (geofence)
            "active_site_id": config.active_site_id,
            // Active operator & operation
            "operator_id": config.active_operator_id,
            "operator_callsign": config.active_operator_callsign,
            "operation_id": config.active_operation_id,
            "operation_name": config.active_operation_name,
            "operation_profile": config.active_operation_profile,
            "session_id": config.active_session_id,
            // Recording engine status
            "recording_active_count": rec_status.active_audio.len() + rec_status.active_iq.len(),
            "recording_active_audio": rec_status.active_audio,
            "recording_active_iq": rec_status.active_iq,
            "auto_clip_enabled": config.auto_clip_enabled,
            "duck_volume": config.duck_volume,
            "active_observation_count": config.active_observation_count,
            // Fingerprint pipeline metrics
            "fp_session_count": self.inner.fp_session_count.load(Ordering::Relaxed),
            "uid_links_total": self.inner.uid_links_total.load(Ordering::Relaxed),
            "emitters_unique": self.inner.emitters_unique.load(Ordering::Relaxed),
            // Derived: are we running simulated SDR?
            "is_simulation": sdr_slots.iter().all(|s| s.device_key.starts_with("simulated")),
            // Active alert highlights (for UI rendering)
            "alert_highlights": self.active_alert_highlights(),
        }))
    }
}
