mod migrations;
pub mod event_store;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Round-robin pool of read-only SQLite connections.
/// WAL mode allows concurrent readers alongside a single writer.
struct ReadPool {
    connections: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl ReadPool {
    fn new(connections: Vec<Connection>) -> Self {
        Self {
            connections: connections.into_iter().map(Mutex::new).collect(),
            next: AtomicUsize::new(0),
        }
    }

    fn acquire(&self) -> MutexGuard<'_, Connection> {
        let start_idx = self.next.fetch_add(1, Ordering::Relaxed) % self.connections.len();
        // Try lock rotation starting from our index
        for offset in 0..self.connections.len() {
            let idx = (start_idx + offset) % self.connections.len();
            if let Ok(guard) = self.connections[idx].try_lock() {
                return guard;
            }
        }
        // All busy — block on our assigned index
        let wait_start = std::time::Instant::now();
        let guard = self.connections[start_idx].lock().expect("rf-db: read pool mutex poisoned");
        let elapsed = wait_start.elapsed();
        if elapsed.as_millis() > 200 {
            tracing::warn!("rf-db: read_pool.acquire() blocked {}ms", elapsed.as_millis());
        }
        guard
    }
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// Pool of read-only connections — round-robin across 4 WAL readers.
    read_pool: Arc<ReadPool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: i64,
    pub name: String,
    pub config_json: String,
    pub created_at: String,
    pub status: String,
    pub description: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub created_by: Option<i64>,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operator {
    pub id: i64,
    pub callsign: String,
    pub display_name: String,
    pub notes: String,
    pub last_login: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DebugLogEntry {
    pub id: i64,
    pub timestamp: String,
    pub band: String,
    pub center_freq: f64,
    pub gain: f64,
    pub threshold: f64,
    pub noise_floor: f64,
    pub peak_power: f64,
    pub peak_freq: f64,
    pub n_signals: i32,
    pub psd_min: f64,
    pub psd_max: f64,
    pub psd_mean: f64,
    pub device_key: String,
    pub sample_rate: f64,
    pub modulation: String,
    pub snr_margin: f64,
    pub agc: bool,
    pub ppm: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Signal {
    pub id: i64,
    pub freq: f64,
    pub name: String,
    pub cls: String,
    pub band: String,
    pub mode: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub total_hits: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: i64,
    pub channel_type: String,
    pub freq_mhz: Option<f64>,
    pub tgid: Option<i32>,
    pub timeslot: Option<i32>,
    pub color_code: Option<i32>,
    pub label: String,
    pub cls: String,
    pub band: String,
    pub mode: Option<String>,
    pub tag: String,
    pub notes: String,
    pub network_id: Option<i64>,
    pub total_hits: i64,
    pub total_seconds: f64,
    pub avg_power: Option<f64>,
    pub last_power: Option<f64>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub encryption_seen: bool,
    pub encryption_current: bool,
    pub source: String,
}

#[derive(Debug, Default)]
pub struct ChannelFilter {
    pub band: Option<String>,
    pub cls: Option<String>,
    pub tag: Option<String>,
    pub source: Option<String>,
    pub active_since: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanPackage {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub item_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CustomChannel {
    pub id: i64,
    pub freq: f64,
    pub name: String,
    pub cls: String,
    pub band: String,
    pub mode: Option<String>,
    pub notes: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanPackageItem {
    pub id: i64,
    pub package_id: i64,
    pub target_type: String,
    pub target_index: i64,
    pub target_name: String,
    pub tgid: Option<i64>,
    pub freq_mhz: Option<f64>,
}

// ── SIGEX Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigexEvent {
    pub id: i64,
    pub module: String,
    pub event_type: String,
    pub severity: String,
    pub summary: String,
    pub details: Option<String>,
    pub system: Option<String>,
    pub tgid: Option<i32>,
    pub uid: Option<i32>,
    pub freq_mhz: Option<f64>,
    pub channel_id: Option<i64>,
    pub timestamp: String,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficSession {
    pub id: i64,
    pub uid: Option<i32>,
    pub tgid: Option<i32>,
    pub system: Option<String>,
    pub freq_mhz: Option<f64>,
    pub channel_id: Option<i64>,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_sec: Option<f64>,
    pub hit_count: i32,
    pub avg_signal: Option<f64>,
    pub encrypted: bool,
    pub modulation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub id: i64,
    pub callsign: Option<String>,
    pub identifier: Option<String>,
    pub description: Option<String>,
    pub organization_id: Option<i64>,
    pub actor_type: String,
    pub first_seen: String,
    pub last_seen: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: i64,
    pub name: String,
    pub abbreviation: Option<String>,
    pub org_type: Option<String>,
    pub parent_id: Option<i64>,
    pub jurisdiction: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelSite {
    pub id: i64,
    pub name: String,
    pub site_type: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub elevation_m: Option<f64>,
    pub address: Option<String>,
    pub notes: Option<String>,
    #[serde(default = "default_geofence_radius")]
    pub geofence_radius_m: f64,
}

fn default_geofence_radius() -> f64 { 500.0 }

/// Pre-resolved signal write for batched DB transactions.
pub struct SignalWrite {
    pub freq: f64,
    pub power: f64,
    pub name: String,
    pub cls: String,
    pub band: String,
    pub mode: Option<String>,
    /// Pre-resolved channel_id from read_conn lookup (None = auto-discover).
    pub channel_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSession {
    pub id: i64,
    pub site_id: i64,
    pub start_time: String,
    pub end_time: Option<String>,
    pub start_lat: Option<f64>,
    pub start_lon: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteDashboard {
    pub total_sessions: i64,
    pub total_grants: i64,
    pub unique_tgids: i64,
    pub unique_uids: i64,
    pub encrypted_grants: i64,
    pub global_grants: i64,
    pub active_session: Option<SiteSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteEncryptionBucket {
    pub hour: String,
    pub encrypted: i64,
    pub clear: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationTarget {
    pub id: i64,
    pub target_type: String,
    pub target_key: String,
    pub target_label: Option<String>,
    pub site_id: Option<i64>,
    pub priority: i32,
    pub notes: Option<String>,
    pub created_at: String,
    pub coverage_target_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionRequirement {
    pub id: i64,
    pub label: String,
    pub check_type: String,
    pub check_config_json: Option<String>,
    pub site_id: Option<i64>,
    pub met: bool,
    pub last_checked: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: i64,
    pub target_id: i64,
    pub site_id: Option<i64>,
    pub site_session_id: Option<i64>,
    pub operation_id: Option<i64>,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_sec: Option<f64>,
    pub receiver_lat: Option<f64>,
    pub receiver_lon: Option<f64>,
    pub device_key: Option<String>,
    pub freq_mhz: Option<f64>,
    pub tgid: Option<i32>,
    pub uid: Option<i32>,
    pub encrypted: bool,
    pub signal_dbfs: Option<f64>,
    pub observation_type: String,
    pub metadata_json: Option<String>,
    // Joined from observation_targets
    pub target_type: Option<String>,
    pub target_key: Option<String>,
    pub target_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationAlert {
    pub id: i64,
    pub target_id: i64,
    pub alert_type: String,
    pub threshold_json: Option<String>,
    pub cooldown_sec: i32,
    pub enabled: bool,
    pub last_fired: Option<String>,
    pub fire_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoIqRule {
    pub id: i64,
    pub trigger_type: String,
    pub trigger_config_json: Option<String>,
    pub enabled: bool,
    pub max_duration_sec: i32,
    pub site_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumintObservation {
    pub id: i64,
    pub observer: Option<String>,
    pub observation: String,
    pub actor_id: Option<i64>,
    pub site_id: Option<i64>,
    pub freq_mhz: Option<f64>,
    pub tgid: Option<i32>,
    pub confidence: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigexDashboard {
    pub total_sessions: i64,
    pub active_sessions: i64,
    pub total_events: i64,
    pub unacked_events: i64,
    pub total_actors: i64,
    pub total_organizations: i64,
    pub total_sites: i64,
    pub events_today: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandCount {
    pub band: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClsCount {
    pub cls: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopTalkgroup {
    pub tgid: i32,
    pub name: Option<String>,
    pub department: Option<String>,
    pub grants: i64,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCollectionStats {
    pub signals_today: i64,
    pub signals_total: i64,
    pub signals_by_band: Vec<BandCount>,
    pub signals_by_cls: Vec<ClsCount>,
    pub grants_today: i64,
    pub unique_tgs_today: i64,
    pub unique_uids_today: i64,
    pub encrypted_grants_today: i64,
    pub key_rotations_today: i64,
    pub active_wx_alerts: i64,
    pub sessions_today: i64,
    pub recordings_today: i64,
    pub recordings_size_today: i64,
    pub top_talkgroups: Vec<TopTalkgroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyEvent {
    pub id: i64,
    pub event_type: String,
    pub freq_mhz: Option<f64>,
    pub channel_id: Option<i64>,
    pub tgid: Option<i32>,
    pub uid: Option<i32>,
    pub system: Option<String>,
    pub severity: String,
    pub description: String,
    pub anomaly_score: Option<f64>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityBaseline {
    pub id: i64,
    pub freq_mhz: Option<f64>,
    pub channel_id: Option<i64>,
    pub tgid: Option<i32>,
    pub system: Option<String>,
    pub hour_of_day: i32,
    pub day_of_week: i32,
    pub avg_sessions: f64,
    pub stddev_sessions: f64,
    pub avg_duration: f64,
    pub avg_unique_uids: f64,
    pub sample_days: i32,
    pub last_computed: String,
    pub profile_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKeyEntry {
    pub id: i64,
    pub tgid: i32,
    pub system: String,
    pub algorithm_id: Option<i32>,
    pub algorithm_name: Option<String>,
    pub key_id: Option<i32>,
    pub first_seen: String,
    pub last_seen: String,
    pub session_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRotationEvent {
    pub id: i64,
    pub tgid: i32,
    pub system: String,
    pub old_key_id: Option<i32>,
    pub new_key_id: Option<i32>,
    pub old_algorithm: Option<i32>,
    pub new_algorithm: Option<i32>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioIdSighting {
    pub id: i64,
    pub uid: i32,
    pub tgid: Option<i32>,
    pub system: Option<String>,
    pub freq_mhz: Option<f64>,
    pub first_seen: String,
    pub last_seen: String,
    pub observation_count: i64,
}

// ── Network Types (Tier 3: P25 Trunking) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSite {
    pub id: i64,
    pub system: String,
    pub wacn: Option<i64>,
    pub system_id: Option<i64>,
    pub rfss_id: Option<i64>,
    pub site_id: Option<i64>,
    pub name: Option<String>,
    pub control_channel: Option<f64>,
    pub alt_control: Option<String>,
    pub voice_channels: Option<String>,
    pub adjacent_sites: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTalkgroup {
    pub id: i64,
    pub system: String,
    pub tgid: i32,
    pub name: Option<String>,
    pub department: Option<String>,
    pub tag: Option<String>,
    pub encrypted: String,
    pub algorithm: Option<String>,
    pub total_grants: i64,
    pub unique_uids: i64,
    pub first_seen: String,
    pub last_seen: String,
    pub priority: i32,
    pub scan_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentSummary {
    pub department: String,
    pub tg_count: i64,
    pub watched_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentDetail {
    pub department: String,
    pub tg_count: i64,
    pub watched_count: i64,
    pub total_grants: i64,
    pub unique_uids: i64,
    pub encrypted_tgs: i64,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub talkgroups: Vec<NetworkTalkgroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityBucket {
    pub day_of_week: i32,
    pub hour: i32,
    pub grant_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentRadio {
    pub uid: i32,
    pub observation_count: i64,
    pub tg_count: i64,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGrant {
    pub id: i64,
    pub system: String,
    pub tgid: i32,
    pub uid: Option<i32>,
    pub voice_freq: Option<f64>,
    pub grant_type: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioAffiliation {
    pub id: i64,
    pub system: String,
    pub uid: i32,
    pub tgid: i32,
    pub event_type: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSummary {
    pub total_sites: i64,
    pub total_talkgroups: i64,
    pub total_grants: i64,
    pub total_affiliations: i64,
    pub unique_uids: i64,
    pub grants_last_hour: i64,
}

/// Per-TGID encryption posture summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionPosture {
    pub tgid: i32,
    pub system: String,
    pub posture: String,  // "clear", "encrypted", "mixed"
    pub algorithms: Vec<String>,
    pub key_ids: Vec<i32>,
    pub total_sessions: i64,
    pub encrypted_sessions: i64,
    pub clear_sessions: i64,
    pub last_rotation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WxAlert {
    pub id: i64,
    pub originator: String,
    pub event_code: String,
    pub event_name: String,
    pub severity: String,
    pub locations: String,
    pub duration_mins: Option<i32>,
    pub issued_utc: Option<String>,
    pub station: Option<String>,
    pub raw_header: String,
    pub confidence: Option<f64>,
    pub received_at: String,
    pub expires_at: Option<String>,
    pub freq_mhz: Option<f64>,
    pub receiver_lat: Option<f64>,
    pub receiver_lon: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: i64,
    pub rec_type: String,
    pub freq_mhz: f64,
    pub modulation: Option<String>,
    pub label: Option<String>,
    pub sample_rate: i32,
    pub channels: i32,
    pub file_path: String,
    pub file_size_bytes: i64,
    pub duration_sec: f64,
    pub start_time: String,
    pub end_time: Option<String>,
    pub trigger_type: String,
    pub tgid: Option<i32>,
    pub device_key: Option<String>,
    pub receiver_lat: Option<f64>,
    pub receiver_lon: Option<f64>,
    pub notes: Option<String>,
    pub site_id: Option<i64>,
    pub site_session_id: Option<i64>,
    pub operation_id: Option<i64>,
    pub source_unit: Option<i32>,
    pub encrypted: bool,
    pub algorithm: Option<String>,
    pub key_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingStats {
    pub total_count: i64,
    pub total_size_bytes: i64,
    pub total_duration_sec: f64,
    pub audio_count: i64,
    pub iq_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipStats {
    pub total_clips: i64,
    pub p25_clips: i64,
    pub analog_clips: i64,
    pub today_clips: i64,
    pub today_size_bytes: i64,
    pub today_duration_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TgGroup {
    pub tgid: Option<i32>,
    pub tg_name: Option<String>,
    pub department: Option<String>,
    pub clip_count: i64,
    pub total_duration: f64,
    pub total_size: i64,
    pub unique_uids: i64,
    pub last_clip_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreqGroup {
    pub freq_mhz: f64,
    pub modulation: Option<String>,
    pub clip_count: i64,
    pub total_duration: f64,
    pub total_size: i64,
    pub last_clip_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceUnitInfo {
    pub source_unit: i32,
    pub clip_count: i64,
    pub total_duration: f64,
    pub last_seen: Option<String>,
}

// --- Query Engine types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub col_type: String,
    pub notnull: bool,
    pub pk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub table: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: i64,
    pub name: String,
    pub sql_text: String,
    pub chart_config: Option<String>,
    pub created_at: String,
}

// --- Antenna types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Antenna {
    pub id: i64,
    pub name: String,
    pub antenna_type: String,
    pub connector: String,
    pub freq_min_mhz: Option<f64>,
    pub freq_max_mhz: Option<f64>,
    pub gain_dbi: Option<f64>,
    pub notes: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntennaFreqRange {
    pub id: i64,
    pub antenna_id: i64,
    pub freq_min_mhz: f64,
    pub freq_max_mhz: f64,
    pub gain_dbi: Option<f64>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAntennaAssignment {
    pub id: i64,
    pub device_serial: String,
    pub antenna_id: i64,
    pub assigned_at: String,
}

// ── RF Fingerprinting (typed structs) ────────────────────

/// Typed view of radio_fingerprints table (v8 schema + v33 additions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioFingerprint {
    pub id: i64,
    pub fingerprint_id: String,
    pub uid: Option<i32>,
    pub freq_offset_hz: Option<f64>,
    pub iq_imbalance: Option<f64>,
    pub evm: Option<f64>,
    pub phase_noise: Option<f64>,
    pub confidence: Option<f64>,
    pub capture_count: i32,
    pub freq_mhz: Option<f64>,
    pub sample_count: Option<i32>,
    pub first_seen: String,
    pub last_seen: String,
}

/// Typed view of uid_fingerprint_map table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UidFingerprintLink {
    pub id: i64,
    pub uid: i32,
    pub fingerprint_id: String,
    pub tgid: Option<i32>,
    pub system: Option<String>,
    pub observation_count: i32,
    pub first_seen: String,
    pub last_seen: String,
}

/// Aggregate fingerprint stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintStatsTyped {
    pub total_fingerprints: i64,
    pub unique_emitters: i64,
    pub uid_links: i64,
    pub linked_uids: i64,
    pub avg_confidence: f64,
}

impl Db {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        // Open 4 read-only connections for SELECT queries.
        // WAL mode allows concurrent readers alongside a single writer,
        // so reads won't block on the write mutex and vice versa.
        let mut readers = Vec::with_capacity(4);
        for _ in 0..4 {
            let read = Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            read.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA query_only=ON;
                 PRAGMA busy_timeout=5000;"
            )?;
            readers.push(read);
        }
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            read_pool: Arc::new(ReadPool::new(readers)),
        };
        // Derive data directory from DB path for seeding JSON files
        let data_dir = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        db.run_migrations(&data_dir)?;
        Ok(db)
    }

    /// Acquire the write connection lock.
    /// Panics if the mutex is poisoned (indicates a prior panic during DB access).
    fn conn(&self) -> MutexGuard<'_, Connection> {
        let start = std::time::Instant::now();
        let guard = self.conn.lock().expect("rf-db: connection mutex poisoned");
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 200 {
            tracing::warn!("rf-db: conn() lock took {}ms", elapsed.as_millis());
        }
        guard
    }

    /// Acquire a read-only connection from the pool.
    /// Use this for SELECT-only queries to avoid blocking on write operations.
    fn read_conn(&self) -> MutexGuard<'_, Connection> {
        self.read_pool.acquire()
    }

    fn run_migrations(&self, data_dir: &std::path::Path) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        migrations::run(&conn, data_dir)
    }

    // --- Operations ---

    pub fn create_operation(&self, name: &str, config_json: &str, description: Option<&str>, created_by: Option<i64>, profile: Option<&str>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO operations (name, config_json, created_at, status, description, created_by, profile)
             VALUES (?1, ?2, datetime('now'), 'created', ?3, ?4, ?5)",
            rusqlite::params![name, config_json, description.unwrap_or(""), created_by, profile.unwrap_or("test")],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Ensure a default TEST MODE operation exists. Returns its id.
    /// Reuses the most recent test op if one exists (resetting completed/paused to created).
    pub fn ensure_default_test_operation(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let existing: Option<(i64, String)> = conn.query_row(
            "SELECT id, status FROM operations WHERE profile = 'test' ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((id, status)) = existing {
            if status == "completed" || status == "paused" {
                conn.execute(
                    "UPDATE operations SET status = 'created', started_at = NULL, stopped_at = NULL WHERE id = ?1",
                    rusqlite::params![id],
                )?;
            }
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO operations (name, config_json, created_at, status, description, profile)
             VALUES ('TEST MODE', '{}', datetime('now'), 'created', 'Default test operation (simulation allowed)', 'test')",
            [],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn row_to_operation(row: &rusqlite::Row) -> Result<Operation, rusqlite::Error> {
        Ok(Operation {
            id: row.get(0)?,
            name: row.get(1)?,
            config_json: row.get(2)?,
            created_at: row.get(3)?,
            status: row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "active".into()),
            description: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            started_at: row.get(6)?,
            stopped_at: row.get(7)?,
            created_by: row.get(8)?,
            profile: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "test".into()),
        })
    }

    const OPERATION_COLS: &str = "id, name, config_json, created_at, status, description, started_at, stopped_at, created_by, profile";

    pub fn list_operations(&self) -> Result<Vec<Operation>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!("SELECT {} FROM operations ORDER BY id DESC", Self::OPERATION_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let ops = stmt.query_map([], Self::row_to_operation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ops)
    }

    pub fn get_operation(&self, id: i64) -> Result<Option<Operation>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!("SELECT {} FROM operations WHERE id = ?1", Self::OPERATION_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(rusqlite::params![id], Self::row_to_operation)?;
        match rows.next() {
            Some(op) => Ok(Some(op?)),
            None => Ok(None),
        }
    }

    pub fn update_operation_status(&self, id: i64, status: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = match status {
            "active" => conn.execute(
                "UPDATE operations SET status = 'active', started_at = COALESCE(started_at, datetime('now')) WHERE id = ?1",
                rusqlite::params![id],
            )?,
            "completed" => conn.execute(
                "UPDATE operations SET status = 'completed', stopped_at = datetime('now') WHERE id = ?1",
                rusqlite::params![id],
            )?,
            "paused" => conn.execute(
                "UPDATE operations SET status = 'paused' WHERE id = ?1",
                rusqlite::params![id],
            )?,
            "archived" => conn.execute(
                "UPDATE operations SET status = 'archived' WHERE id = ?1",
                rusqlite::params![id],
            )?,
            _ => conn.execute(
                "UPDATE operations SET status = ?1 WHERE id = ?2",
                rusqlite::params![status, id],
            )?,
        };
        Ok(changes > 0)
    }

    pub fn save_operation_config(&self, id: i64, config_json: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "UPDATE operations SET config_json = ?1 WHERE id = ?2",
            rusqlite::params![config_json, id],
        )?;
        Ok(changes > 0)
    }

    pub fn get_operation_stats(&self, operation_id: i64) -> Result<serde_json::Value, rusqlite::Error> {
        let conn = self.read_conn();
        let signal_hits: i64 = conn.query_row(
            "SELECT COUNT(*) FROM signal_hits WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        )?;
        let traffic_sessions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM traffic_sessions WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        )?;
        let channel_grants: i64 = conn.query_row(
            "SELECT COUNT(*) FROM channel_grants WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        )?;
        let recordings: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recordings WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        )?;
        let sigex_events: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sigex_events WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        )?;
        let sessions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        )?;
        let encrypted_grants: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT g.id) FROM channel_grants g \
             JOIN network_talkgroups t ON g.tgid = t.tgid AND g.system = t.system \
             WHERE g.operation_id = ?1 AND t.encrypted = 1",
            rusqlite::params![operation_id], |r| r.get(0),
        ).unwrap_or(0);
        let unique_tgs: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT tgid) FROM channel_grants WHERE operation_id = ?1",
            rusqlite::params![operation_id], |r| r.get(0),
        ).unwrap_or(0);
        Ok(serde_json::json!({
            "signal_hits": signal_hits,
            "traffic_sessions": traffic_sessions,
            "channel_grants": channel_grants,
            "recordings": recordings,
            "sigex_events": sigex_events,
            "sessions": sessions,
            "encrypted_grants": encrypted_grants,
            "unique_tgs": unique_tgs,
        }))
    }

    pub fn get_aggregate_stats(&self) -> Result<serde_json::Value, rusqlite::Error> {
        let conn = self.read_conn();
        // Use MAX(rowid) as fast approximation for large tables (avoids full table scan).
        // Small tables (operations, operators, sessions) use COUNT(*) since they're tiny.
        let row = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM operations),
                (SELECT COUNT(*) FROM operations WHERE status = 'active'),
                (SELECT COUNT(*) FROM operations WHERE status = 'completed'),
                (SELECT COALESCE(MAX(rowid), 0) FROM signal_hits),
                (SELECT COALESCE(MAX(rowid), 0) FROM traffic_sessions),
                (SELECT COALESCE(MAX(rowid), 0) FROM channel_grants),
                (SELECT COALESCE(MAX(rowid), 0) FROM recordings),
                (SELECT COALESCE(MAX(rowid), 0) FROM sigex_events),
                (SELECT COUNT(*) FROM sessions),
                (SELECT COUNT(*) FROM operators)",
            [],
            |r| Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
            )),
        )?;
        let unique_tgs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM (SELECT DISTINCT tgid FROM channel_grants LIMIT 10000)",
            [],
            |r| r.get(0),
        ).unwrap_or(0);
        let encrypted_grants: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT g.id) FROM channel_grants g \
             JOIN network_talkgroups t ON g.tgid = t.tgid AND g.system = t.system \
             WHERE t.encrypted = 1",
            [], |r| r.get(0),
        ).unwrap_or(0);
        Ok(serde_json::json!({
            "total_ops": row.0,
            "active_ops": row.1,
            "completed_ops": row.2,
            "signal_hits": row.3,
            "traffic_sessions": row.4,
            "channel_grants": row.5,
            "recordings": row.6,
            "sigex_events": row.7,
            "sessions": row.8,
            "encrypted_grants": encrypted_grants,
            "unique_tgs": unique_tgs,
            "operators": row.9,
        }))
    }

    pub fn delete_operation(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM operation_operators WHERE operation_id = ?1", rusqlite::params![id])?;
        conn.execute("UPDATE signal_hits SET operation_id = NULL WHERE operation_id = ?1", rusqlite::params![id])?;
        conn.execute("UPDATE traffic_sessions SET operation_id = NULL WHERE operation_id = ?1", rusqlite::params![id])?;
        conn.execute("UPDATE channel_grants SET operation_id = NULL WHERE operation_id = ?1", rusqlite::params![id])?;
        conn.execute("UPDATE recordings SET operation_id = NULL WHERE operation_id = ?1", rusqlite::params![id])?;
        conn.execute("UPDATE sigex_events SET operation_id = NULL WHERE operation_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM sessions WHERE operation_id = ?1", rusqlite::params![id])?;
        let changes = conn.execute("DELETE FROM operations WHERE id = ?1", rusqlite::params![id])?;
        Ok(changes > 0)
    }

    // ── Operators ──

    pub fn create_operator(&self, callsign: &str, display_name: &str, notes: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO operators (callsign, display_name, notes) VALUES (?1, ?2, ?3)",
            rusqlite::params![callsign, display_name, notes],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_operators(&self) -> Result<Vec<Operator>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, callsign, display_name, notes, last_login, created_at FROM operators ORDER BY callsign"
        )?;
        let ops = stmt.query_map([], |row| {
            Ok(Operator {
                id: row.get(0)?,
                callsign: row.get(1)?,
                display_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                notes: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                last_login: row.get(4)?,
                created_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(ops)
    }

    pub fn update_operator(&self, id: i64, callsign: Option<&str>, display_name: Option<&str>, notes: Option<&str>) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let current = {
            let mut stmt = conn.prepare("SELECT callsign, display_name, notes FROM operators WHERE id = ?1")?;
            let mut rows = stmt.query(rusqlite::params![id])?;
            match rows.next()? {
                Some(row) => (row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?),
                None => return Ok(false),
            }
        };
        let cs = callsign.unwrap_or(&current.0);
        let dn = display_name.unwrap_or(&current.1);
        let n = notes.unwrap_or(&current.2);
        let changes = conn.execute(
            "UPDATE operators SET callsign = ?1, display_name = ?2, notes = ?3 WHERE id = ?4",
            rusqlite::params![cs, dn, n, id],
        )?;
        Ok(changes > 0)
    }

    pub fn delete_operator(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM operation_operators WHERE operator_id = ?1", rusqlite::params![id])?;
        let changes = conn.execute("DELETE FROM operators WHERE id = ?1", rusqlite::params![id])?;
        Ok(changes > 0)
    }

    pub fn update_operator_login(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE operators SET last_login = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    // ── Operation ↔ Operator junction ──

    pub fn assign_operator_to_operation(&self, operation_id: i64, operator_id: i64, role: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "INSERT OR IGNORE INTO operation_operators (operation_id, operator_id, role) VALUES (?1, ?2, ?3)",
            rusqlite::params![operation_id, operator_id, role],
        )?;
        Ok(changes > 0)
    }

    pub fn remove_operator_from_operation(&self, operation_id: i64, operator_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "DELETE FROM operation_operators WHERE operation_id = ?1 AND operator_id = ?2",
            rusqlite::params![operation_id, operator_id],
        )?;
        Ok(changes > 0)
    }

    pub fn list_operation_operators(&self, operation_id: i64) -> Result<Vec<Operator>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT o.id, o.callsign, o.display_name, o.notes, o.last_login, o.created_at
             FROM operators o
             JOIN operation_operators oo ON oo.operator_id = o.id
             WHERE oo.operation_id = ?1
             ORDER BY o.callsign"
        )?;
        let ops = stmt.query_map(rusqlite::params![operation_id], |row| {
            Ok(Operator {
                id: row.get(0)?,
                callsign: row.get(1)?,
                display_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                notes: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                last_login: row.get(4)?,
                created_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(ops)
    }

    // ── Sessions (operation sessions) ──

    pub fn create_session(&self, operation_id: i64, operator_id: Option<i64>, mode: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sessions (operation_id, start, mode, operator_id) VALUES (?1, datetime('now'), ?2, ?3)",
            rusqlite::params![operation_id, mode, operator_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn close_session(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "UPDATE sessions SET end_time = datetime('now') WHERE id = ?1 AND end_time IS NULL",
            rusqlite::params![id],
        )?;
        Ok(changes > 0)
    }

    // --- Signals ---

    /// Insert or update a signal. Returns the signal_id.
    pub fn upsert_signal(
        &self,
        freq: f64,
        name: &str,
        cls: &str,
        band: &str,
        mode: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.prepare_cached(
            "INSERT INTO signals (freq, name, cls, band, mode, first_seen, last_seen, total_hits)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'), 1)
             ON CONFLICT(freq) DO UPDATE SET
               last_seen = datetime('now'),
               total_hits = total_hits + 1,
               name = CASE WHEN excluded.name != 'Unknown' THEN excluded.name ELSE signals.name END,
               cls = CASE WHEN excluded.cls != 'UNK' THEN excluded.cls ELSE signals.cls END",
        )?.execute(rusqlite::params![freq, name, cls, band, mode])?;
        // Get the signal_id
        let id: i64 = conn.query_row(
            "SELECT id FROM signals WHERE freq = ?1",
            rusqlite::params![freq],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Insert a signal hit record.
    pub fn insert_hit(
        &self,
        signal_id: i64,
        power: f64,
        session_id: Option<i64>,
        operation_id: Option<i64>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.prepare_cached(
            "INSERT INTO signal_hits (signal_id, power, timestamp, session_id, operation_id)
             VALUES (?1, ?2, datetime('now'), ?3, ?4)",
        )?.execute(rusqlite::params![signal_id, power, session_id, operation_id])?;
        Ok(())
    }

    /// Batch insert signal hits (for 500ms batch writes).
    pub fn batch_insert_hits(
        &self,
        hits: &[(i64, f64)],
        session_id: Option<i64>,
        operation_id: Option<i64>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO signal_hits (signal_id, power, timestamp, session_id, operation_id)
                 VALUES (?1, ?2, datetime('now'), ?3, ?4)",
            )?;
            for &(signal_id, power) in hits {
                stmt.execute(rusqlite::params![signal_id, power, session_id, operation_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Batched signal processing — single write lock, single transaction.
    /// Pre-resolved channel lookups happen on read connections before calling this.
    pub fn batch_process_signals(
        &self,
        writes: &[SignalWrite],
        operation_id: Option<i64>,
    ) -> Result<(), rusqlite::Error> {
        if writes.is_empty() { return Ok(()); }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut upsert_stmt = tx.prepare_cached(
                "INSERT INTO signals (freq, name, cls, band, mode, first_seen, last_seen, total_hits)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'), 1)
                 ON CONFLICT(freq) DO UPDATE SET
                   last_seen = datetime('now'),
                   total_hits = total_hits + 1,
                   name = CASE WHEN excluded.name != 'Unknown' THEN excluded.name ELSE signals.name END,
                   cls = CASE WHEN excluded.cls != 'UNK' THEN excluded.cls ELSE signals.cls END",
            )?;
            let mut id_stmt = tx.prepare_cached(
                "SELECT id FROM signals WHERE freq = ?1",
            )?;
            let mut hit_stmt = tx.prepare_cached(
                "INSERT INTO signal_hits (signal_id, power, timestamp, session_id, operation_id)
                 VALUES (?1, ?2, datetime('now'), ?3, ?4)",
            )?;
            let mut link_stmt = tx.prepare_cached(
                "UPDATE signals SET channel_id = ?1 WHERE id = ?2",
            )?;
            let mut chan_stats_stmt = tx.prepare_cached(
                "UPDATE channels SET
                    total_hits = total_hits + 1,
                    last_power = ?1,
                    avg_power = CASE WHEN avg_power IS NULL THEN ?1 ELSE (avg_power * (total_hits - 1) + ?1) / total_hits END,
                    first_seen = COALESCE(first_seen, datetime('now')),
                    last_seen = datetime('now'),
                    updated_at = datetime('now')
                 WHERE id = ?2",
            )?;
            let mut discover_stmt = tx.prepare_cached(
                "INSERT INTO channels (channel_type, freq_mhz, label, cls, band, mode, source, first_seen, last_seen)
                 VALUES ('analog', ?1, ?2, ?3, ?4, ?5, 'discovered', datetime('now'), datetime('now'))
                 ON CONFLICT (channel_type, freq_mhz, tgid, timeslot) DO UPDATE SET last_seen = datetime('now')",
            )?;
            let mut discover_id_stmt = tx.prepare_cached(
                "SELECT id FROM channels WHERE channel_type = 'analog' AND freq_mhz = ?1",
            )?;

            for w in writes {
                // Upsert signal
                upsert_stmt.execute(rusqlite::params![w.freq, w.name, w.cls, w.band, w.mode])?;
                let signal_id: i64 = id_stmt.query_row(rusqlite::params![w.freq], |row| row.get(0))?;

                // Insert hit
                hit_stmt.execute(rusqlite::params![signal_id, w.power, Option::<i64>::None, operation_id])?;

                // Link to channel and update stats
                if let Some(ch_id) = w.channel_id {
                    link_stmt.execute(rusqlite::params![ch_id, signal_id])?;
                    chan_stats_stmt.execute(rusqlite::params![w.power, ch_id])?;
                } else {
                    // Auto-discover
                    let label = format!("Unknown {:.3}", w.freq);
                    discover_stmt.execute(rusqlite::params![w.freq, label, w.cls, w.band, w.mode])?;
                    if let Ok(ch_id) = discover_id_stmt.query_row(rusqlite::params![w.freq], |row| row.get::<_, i64>(0)) {
                        link_stmt.execute(rusqlite::params![ch_id, signal_id])?;
                        chan_stats_stmt.execute(rusqlite::params![w.power, ch_id])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Insert a debug log entry for real-time PSD analysis (timeseries scan data).
    pub fn insert_debug_log(
        &self,
        band: &str,
        center_freq: f64,
        gain: f64,
        threshold: f64,
        noise_floor: f64,
        peak_power: f64,
        peak_freq: f64,
        n_signals: i32,
        psd_min: f64,
        psd_max: f64,
        psd_mean: f64,
        device_key: &str,
        sample_rate: f64,
        modulation: &str,
        snr_margin: f64,
        agc: bool,
        ppm: f64,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.prepare_cached(
            "INSERT INTO debug_log (band, center_freq, gain, threshold, noise_floor, peak_power, peak_freq, n_signals, psd_min, psd_max, psd_mean, device_key, sample_rate, modulation, snr_margin, agc, ppm)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        )?.execute(rusqlite::params![band, center_freq, gain, threshold, noise_floor, peak_power, peak_freq, n_signals, psd_min, psd_max, psd_mean, device_key, sample_rate, modulation, snr_margin, agc as i32, ppm])?;
        Ok(())
    }

    /// Get recent debug log entries.
    pub fn recent_debug_log(&self, limit: usize) -> Result<Vec<DebugLogEntry>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, band, center_freq, gain, threshold, noise_floor, peak_power, peak_freq, n_signals, psd_min, psd_max, psd_mean, device_key, sample_rate, modulation, snr_margin, agc, ppm
             FROM debug_log ORDER BY id DESC LIMIT ?1"
        )?;
        let entries = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(DebugLogEntry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                band: row.get(2)?,
                center_freq: row.get(3)?,
                gain: row.get(4)?,
                threshold: row.get(5)?,
                noise_floor: row.get(6)?,
                peak_power: row.get(7)?,
                peak_freq: row.get(8)?,
                n_signals: row.get(9)?,
                psd_min: row.get(10)?,
                psd_max: row.get(11)?,
                psd_mean: row.get(12)?,
                device_key: row.get::<_, Option<String>>(13)?.unwrap_or_default(),
                sample_rate: row.get::<_, Option<f64>>(14)?.unwrap_or(0.0),
                modulation: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
                snr_margin: row.get::<_, Option<f64>>(16)?.unwrap_or(0.0),
                agc: row.get::<_, Option<i32>>(17)?.unwrap_or(0) != 0,
                ppm: row.get::<_, Option<f64>>(18)?.unwrap_or(0.0),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Clear all debug log entries.
    pub fn clear_debug_log(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM debug_log", [])?;
        Ok(())
    }

    // --- Scan Packages ---

    pub fn create_package(&self, name: &str, description: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO scan_packages (name, description) VALUES (?1, ?2)",
            rusqlite::params![name, description],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_package(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM scan_package_items WHERE package_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM scan_packages WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    pub fn get_packages(&self) -> Result<Vec<ScanPackage>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.description, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM scan_package_items WHERE package_id = p.id) as item_count
             FROM scan_packages p ORDER BY p.name"
        )?;
        let pkgs = stmt.query_map([], |row| {
            Ok(ScanPackage {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                item_count: row.get(5)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(pkgs)
    }

    pub fn get_package_items(&self, package_id: i64) -> Result<Vec<ScanPackageItem>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, package_id, target_type, target_index, target_name, tgid, freq_mhz
             FROM scan_package_items WHERE package_id = ?1 ORDER BY target_type, target_index"
        )?;
        let items = stmt.query_map(rusqlite::params![package_id], |row| {
            Ok(ScanPackageItem {
                id: row.get(0)?,
                package_id: row.get(1)?,
                target_type: row.get(2)?,
                target_index: row.get(3)?,
                target_name: row.get(4)?,
                tgid: row.get(5)?,
                freq_mhz: row.get(6)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn add_package_item(
        &self,
        package_id: i64,
        target_type: &str,
        target_index: i64,
        target_name: &str,
        tgid: Option<i64>,
        freq_mhz: Option<f64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO scan_package_items (package_id, target_type, target_index, target_name, tgid, freq_mhz)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![package_id, target_type, target_index, target_name, tgid, freq_mhz],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM scan_package_items WHERE package_id = ?1 AND target_type = ?2 AND target_index = ?3",
            rusqlite::params![package_id, target_type, target_index],
            |row| row.get(0),
        )?;
        conn.execute(
            "UPDATE scan_packages SET updated_at = datetime('now') WHERE id = ?1",
            rusqlite::params![package_id],
        )?;
        Ok(id)
    }

    pub fn remove_package_item(
        &self,
        package_id: i64,
        target_type: &str,
        target_index: i64,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM scan_package_items WHERE package_id = ?1 AND target_type = ?2 AND target_index = ?3",
            rusqlite::params![package_id, target_type, target_index],
        )?;
        conn.execute(
            "UPDATE scan_packages SET updated_at = datetime('now') WHERE id = ?1",
            rusqlite::params![package_id],
        )?;
        Ok(())
    }

    /// Delete all packages and re-seed defaults.
    pub fn reset_default_packages(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute_batch(
            "DELETE FROM scan_package_items;
             DELETE FROM scan_packages;"
        )?;
        crate::migrations::seed_default_packages(&conn)?;
        Ok(())
    }

    // --- Custom Channels ---

    pub fn create_custom_channel(
        &self,
        freq: f64,
        name: &str,
        cls: &str,
        band: &str,
        mode: Option<&str>,
        notes: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO custom_channels (freq, name, cls, band, mode, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(freq) DO UPDATE SET name=excluded.name, cls=excluded.cls, band=excluded.band, mode=excluded.mode, notes=excluded.notes",
            rusqlite::params![freq, name, cls, band, mode, notes],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM custom_channels WHERE freq = ?1",
            rusqlite::params![freq],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn get_custom_channels(&self) -> Result<Vec<CustomChannel>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, freq, name, cls, band, mode, notes, created_at FROM custom_channels ORDER BY band, freq"
        )?;
        let channels = stmt.query_map([], |row| {
            Ok(CustomChannel {
                id: row.get(0)?,
                freq: row.get(1)?,
                name: row.get(2)?,
                cls: row.get(3)?,
                band: row.get(4)?,
                mode: row.get(5)?,
                notes: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(channels)
    }

    pub fn delete_custom_channel(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM custom_channels WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    // --- Channels ---

    fn row_to_channel(row: &rusqlite::Row) -> Result<Channel, rusqlite::Error> {
        Ok(Channel {
            id: row.get(0)?,
            channel_type: row.get(1)?,
            freq_mhz: row.get(2)?,
            tgid: row.get(3)?,
            timeslot: row.get(4)?,
            color_code: row.get(5)?,
            label: row.get(6)?,
            cls: row.get(7)?,
            band: row.get(8)?,
            mode: row.get(9)?,
            tag: row.get(10)?,
            notes: row.get(11)?,
            network_id: row.get(12)?,
            total_hits: row.get(13)?,
            total_seconds: row.get(14)?,
            avg_power: row.get(15)?,
            last_power: row.get(16)?,
            first_seen: row.get(17)?,
            last_seen: row.get(18)?,
            encryption_seen: row.get(19)?,
            encryption_current: row.get(20)?,
            source: row.get(21)?,
        })
    }

    const CHANNEL_COLS: &str =
        "id, channel_type, freq_mhz, tgid, timeslot, color_code, label, cls, band, mode, tag, notes, network_id, total_hits, total_seconds, avg_power, last_power, first_seen, last_seen, encryption_seen, encryption_current, source";

    /// Look up a channel by frequency (±15 kHz tolerance for analog).
    pub fn lookup_channel_by_freq(&self, freq_mhz: f64) -> Result<Option<Channel>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!(
            "SELECT {} FROM channels WHERE freq_mhz IS NOT NULL AND ABS(freq_mhz - ?1) < 0.015 ORDER BY ABS(freq_mhz - ?1) LIMIT 1",
            Self::CHANNEL_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(rusqlite::params![freq_mhz], Self::row_to_channel)?;
        match rows.next() {
            Some(ch) => Ok(Some(ch?)),
            None => Ok(None),
        }
    }

    /// Look up a channel by P25 talkgroup ID.
    pub fn lookup_channel_by_tgid(&self, tgid: i32) -> Result<Option<Channel>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!(
            "SELECT {} FROM channels WHERE tgid = ?1 LIMIT 1",
            Self::CHANNEL_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(rusqlite::params![tgid], Self::row_to_channel)?;
        match rows.next() {
            Some(ch) => Ok(Some(ch?)),
            None => Ok(None),
        }
    }

    /// List channels with optional filtering.
    pub fn list_channels(&self, filter: &ChannelFilter) -> Result<Vec<Channel>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut sql = format!("SELECT {} FROM channels WHERE 1=1", Self::CHANNEL_COLS);
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(ref band) = filter.band {
            sql.push_str(&format!(" AND band = ?{}", idx));
            params.push(Box::new(band.clone()));
            idx += 1;
        }
        if let Some(ref cls) = filter.cls {
            sql.push_str(&format!(" AND cls = ?{}", idx));
            params.push(Box::new(cls.clone()));
            idx += 1;
        }
        if let Some(ref tag) = filter.tag {
            sql.push_str(&format!(" AND tag = ?{}", idx));
            params.push(Box::new(tag.clone()));
            idx += 1;
        }
        if let Some(ref source) = filter.source {
            sql.push_str(&format!(" AND source = ?{}", idx));
            params.push(Box::new(source.clone()));
            idx += 1;
        }
        if let Some(ref since) = filter.active_since {
            sql.push_str(&format!(" AND last_seen >= ?{}", idx));
            params.push(Box::new(since.clone()));
            idx += 1;
        }
        sql.push_str(" ORDER BY band, freq_mhz");
        let limit = filter.limit.unwrap_or(1000) as i64;
        sql.push_str(&format!(" LIMIT ?{}", idx));
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let channels = stmt.query_map(param_refs.as_slice(), Self::row_to_channel)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(channels)
    }

    /// Get a single channel by ID.
    pub fn get_channel(&self, id: i64) -> Result<Option<Channel>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!("SELECT {} FROM channels WHERE id = ?1", Self::CHANNEL_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(rusqlite::params![id], Self::row_to_channel)?;
        match rows.next() {
            Some(ch) => Ok(Some(ch?)),
            None => Ok(None),
        }
    }

    /// Update channel traffic stats after a signal detection.
    pub fn update_channel_stats(&self, id: i64, power: f64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.prepare_cached(
            "UPDATE channels SET
                total_hits = total_hits + 1,
                last_power = ?1,
                avg_power = CASE WHEN avg_power IS NULL THEN ?1 ELSE (avg_power * (total_hits - 1) + ?1) / total_hits END,
                first_seen = COALESCE(first_seen, datetime('now')),
                last_seen = datetime('now'),
                updated_at = datetime('now')
             WHERE id = ?2",
        )?.execute(rusqlite::params![power, id])?;
        Ok(())
    }

    pub fn increment_channel_seconds(&self, id: i64, seconds: f64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE channels SET total_seconds = total_seconds + ?1 WHERE id = ?2",
            rusqlite::params![seconds, id],
        )?;
        Ok(())
    }

    /// Auto-discover a new channel from an unknown signal.
    pub fn discover_channel(&self, freq: f64, cls: &str, band: &str, mode: Option<&str>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let label = format!("Unknown {:.3}", freq);
        conn.execute(
            "INSERT INTO channels (channel_type, freq_mhz, label, cls, band, mode, source, first_seen, last_seen)
             VALUES ('analog', ?1, ?2, ?3, ?4, ?5, 'discovered', datetime('now'), datetime('now'))
             ON CONFLICT (channel_type, freq_mhz, tgid, timeslot) DO UPDATE SET last_seen = datetime('now')",
            rusqlite::params![freq, label, cls, band, mode],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM channels WHERE channel_type = 'analog' AND freq_mhz = ?1",
            rusqlite::params![freq],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Create a custom channel.
    pub fn create_channel(
        &self, freq: f64, label: &str, cls: &str, band: &str, mode: Option<&str>, tag: &str, notes: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO channels (channel_type, freq_mhz, label, cls, band, mode, tag, notes, source)
             VALUES ('analog', ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'custom')",
            rusqlite::params![freq, label, cls, band, mode, tag, notes],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update a channel's metadata.
    pub fn update_channel(&self, id: i64, label: &str, cls: &str, tag: &str, notes: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE channels SET label = ?1, cls = ?2, tag = ?3, notes = ?4, updated_at = datetime('now') WHERE id = ?5",
            rusqlite::params![label, cls, tag, notes, id],
        )?;
        Ok(())
    }

    /// Delete a channel (only custom/discovered).
    pub fn delete_channel(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM channels WHERE id = ?1 AND source IN ('custom', 'discovered')",
            rusqlite::params![id],
        )?;
        Ok(deleted > 0)
    }

    /// Set channel_id on a signal row.
    pub fn link_signal_to_channel(&self, signal_id: i64, channel_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE signals SET channel_id = ?1 WHERE id = ?2",
            rusqlite::params![channel_id, signal_id],
        )?;
        Ok(())
    }

    /// Get signal_hits history for a channel.
    pub fn channel_history(&self, channel_id: i64, limit: usize) -> Result<Vec<(f64, String)>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT sh.power, sh.timestamp FROM signal_hits sh
             JOIN signals s ON sh.signal_id = s.id
             WHERE s.channel_id = ?1
             ORDER BY sh.id DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![channel_id, limit as i64], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── SIGEX: Events ──

    pub fn insert_sigex_event(
        &self,
        module: &str,
        event_type: &str,
        severity: &str,
        summary: &str,
        details: Option<&str>,
        system: Option<&str>,
        tgid: Option<i32>,
        uid: Option<i32>,
        freq_mhz: Option<f64>,
        channel_id: Option<i64>,
        operation_id: Option<i64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sigex_events (module, event_type, severity, summary, details, system, tgid, uid, freq_mhz, channel_id, timestamp, operation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'), ?11)",
            rusqlite::params![module, event_type, severity, summary, details, system, tgid, uid, freq_mhz, channel_id, operation_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_sigex_events(&self, limit: usize, module_filter: Option<&str>, severity_filter: Option<&str>) -> Result<Vec<SigexEvent>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut sql = String::from(
            "SELECT id, module, event_type, severity, summary, details, system, tgid, uid, freq_mhz, channel_id, timestamp, acknowledged FROM sigex_events WHERE 1=1"
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;
        if let Some(m) = module_filter {
            sql.push_str(&format!(" AND module = ?{}", idx));
            params.push(Box::new(m.to_string()));
            idx += 1;
        }
        if let Some(s) = severity_filter {
            sql.push_str(&format!(" AND severity = ?{}", idx));
            params.push(Box::new(s.to_string()));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", idx));
        params.push(Box::new(limit as i64));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(SigexEvent {
                id: row.get(0)?,
                module: row.get(1)?,
                event_type: row.get(2)?,
                severity: row.get(3)?,
                summary: row.get(4)?,
                details: row.get(5)?,
                system: row.get(6)?,
                tgid: row.get(7)?,
                uid: row.get(8)?,
                freq_mhz: row.get(9)?,
                channel_id: row.get(10)?,
                timestamp: row.get(11)?,
                acknowledged: row.get(12)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn ack_sigex_event(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE sigex_events SET acknowledged = 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(n > 0)
    }

    // ── SIGEX: Traffic Sessions ──

    pub fn insert_traffic_session(
        &self,
        freq_mhz: f64,
        channel_id: Option<i64>,
        start_time: &str,
        end_time: Option<&str>,
        duration_sec: Option<f64>,
        hit_count: i32,
        avg_signal: Option<f64>,
        encrypted: bool,
        modulation: Option<&str>,
        operation_id: Option<i64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO traffic_sessions (freq_mhz, channel_id, start_time, end_time, duration_sec, hit_count, avg_signal, encrypted, modulation, operation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![freq_mhz, channel_id, start_time, end_time, duration_sec, hit_count, avg_signal, encrypted as i32, modulation, operation_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_traffic_sessions(&self, limit: usize) -> Result<Vec<TrafficSession>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, uid, tgid, system, freq_mhz, channel_id, start_time, end_time, duration_sec, hit_count, avg_signal, encrypted, modulation
             FROM traffic_sessions ORDER BY id DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(TrafficSession {
                id: row.get(0)?,
                uid: row.get(1)?,
                tgid: row.get(2)?,
                system: row.get(3)?,
                freq_mhz: row.get(4)?,
                channel_id: row.get(5)?,
                start_time: row.get(6)?,
                end_time: row.get(7)?,
                duration_sec: row.get(8)?,
                hit_count: row.get(9)?,
                avg_signal: row.get(10)?,
                encrypted: row.get(11)?,
                modulation: row.get(12)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── SIGEX: Actors ──

    pub fn create_actor(
        &self, callsign: Option<&str>, identifier: Option<&str>, description: Option<&str>,
        organization_id: Option<i64>, actor_type: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO actors (callsign, identifier, description, organization_id, actor_type, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))",
            rusqlite::params![callsign, identifier, description, organization_id, actor_type],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_actors(&self, limit: usize) -> Result<Vec<Actor>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, callsign, identifier, description, organization_id, actor_type, first_seen, last_seen, created_at
             FROM actors ORDER BY last_seen DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(Actor {
                id: row.get(0)?,
                callsign: row.get(1)?,
                identifier: row.get(2)?,
                description: row.get(3)?,
                organization_id: row.get(4)?,
                actor_type: row.get(5)?,
                first_seen: row.get(6)?,
                last_seen: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_actor(&self, id: i64) -> Result<Option<Actor>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, callsign, identifier, description, organization_id, actor_type, first_seen, last_seen, created_at
             FROM actors WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok(Actor {
                id: row.get(0)?,
                callsign: row.get(1)?,
                identifier: row.get(2)?,
                description: row.get(3)?,
                organization_id: row.get(4)?,
                actor_type: row.get(5)?,
                first_seen: row.get(6)?,
                last_seen: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        match rows.next() {
            Some(a) => Ok(Some(a?)),
            None => Ok(None),
        }
    }

    pub fn update_actor(
        &self, id: i64, callsign: Option<&str>, identifier: Option<&str>,
        description: Option<&str>, organization_id: Option<i64>, actor_type: &str,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE actors SET callsign = ?1, identifier = ?2, description = ?3, organization_id = ?4, actor_type = ?5, last_seen = datetime('now') WHERE id = ?6",
            rusqlite::params![callsign, identifier, description, organization_id, actor_type, id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_actor(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM actor_attributes WHERE actor_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM actor_radio_ids WHERE actor_id = ?1", rusqlite::params![id])?;
        let n = conn.execute("DELETE FROM actors WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }

    // ── SIGEX: Organizations ──

    pub fn create_organization(
        &self, name: &str, abbreviation: Option<&str>, org_type: Option<&str>,
        parent_id: Option<i64>, jurisdiction: Option<&str>, notes: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO organizations (name, abbreviation, org_type, parent_id, jurisdiction, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, abbreviation, org_type, parent_id, jurisdiction, notes],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_organizations(&self, limit: usize) -> Result<Vec<Organization>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, abbreviation, org_type, parent_id, jurisdiction, notes
             FROM organizations ORDER BY name LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(Organization {
                id: row.get(0)?,
                name: row.get(1)?,
                abbreviation: row.get(2)?,
                org_type: row.get(3)?,
                parent_id: row.get(4)?,
                jurisdiction: row.get(5)?,
                notes: row.get(6)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_organization(
        &self, id: i64, name: &str, abbreviation: Option<&str>, org_type: Option<&str>,
        parent_id: Option<i64>, jurisdiction: Option<&str>, notes: Option<&str>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE organizations SET name = ?1, abbreviation = ?2, org_type = ?3, parent_id = ?4, jurisdiction = ?5, notes = ?6 WHERE id = ?7",
            rusqlite::params![name, abbreviation, org_type, parent_id, jurisdiction, notes, id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_organization(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute("DELETE FROM organizations WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }

    pub fn delete_humint_observation(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute("DELETE FROM humint_observations WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }

    // ── SIGEX: Intel Sites ──

    pub fn create_intel_site(
        &self, name: &str, site_type: Option<&str>, latitude: Option<f64>,
        longitude: Option<f64>, elevation_m: Option<f64>, address: Option<&str>, notes: Option<&str>,
        geofence_radius_m: Option<f64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let radius = geofence_radius_m.unwrap_or(500.0);
        conn.execute(
            "INSERT INTO intel_sites (name, site_type, latitude, longitude, elevation_m, address, notes, geofence_radius_m)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![name, site_type, latitude, longitude, elevation_m, address, notes, radius],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_intel_sites(&self, limit: usize) -> Result<Vec<IntelSite>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, site_type, latitude, longitude, elevation_m, address, notes, COALESCE(geofence_radius_m, 500.0)
             FROM intel_sites ORDER BY name LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(IntelSite {
                id: row.get(0)?,
                name: row.get(1)?,
                site_type: row.get(2)?,
                latitude: row.get(3)?,
                longitude: row.get(4)?,
                elevation_m: row.get(5)?,
                address: row.get(6)?,
                notes: row.get(7)?,
                geofence_radius_m: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_intel_site(
        &self, id: i64, name: &str, site_type: Option<&str>, latitude: Option<f64>,
        longitude: Option<f64>, elevation_m: Option<f64>, address: Option<&str>, notes: Option<&str>,
        geofence_radius_m: Option<f64>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let radius = geofence_radius_m.unwrap_or(500.0);
        let n = conn.execute(
            "UPDATE intel_sites SET name = ?1, site_type = ?2, latitude = ?3, longitude = ?4, elevation_m = ?5, address = ?6, notes = ?7, geofence_radius_m = ?8 WHERE id = ?9",
            rusqlite::params![name, site_type, latitude, longitude, elevation_m, address, notes, radius, id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_intel_site(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM site_sessions WHERE site_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM site_frequencies WHERE site_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM site_organizations WHERE site_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM site_observations WHERE site_id = ?1", rusqlite::params![id])?;
        let n = conn.execute("DELETE FROM intel_sites WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }

    // ── SIGEX: Site Sessions & Site-Scoped Queries ──

    pub fn open_site_session(&self, site_id: i64, lat: Option<f64>, lon: Option<f64>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO site_sessions (site_id, start_time, start_lat, start_lon)
             VALUES (?1, datetime('now'), ?2, ?3)",
            rusqlite::params![site_id, lat, lon],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn close_site_session(&self, session_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE site_sessions SET end_time = datetime('now') WHERE id = ?1 AND end_time IS NULL",
            rusqlite::params![session_id],
        )?;
        Ok(n > 0)
    }

    /// Close all open site sessions. Called on startup to clean up sessions
    /// orphaned when the app exited without proper shutdown.
    pub fn close_all_open_site_sessions(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE site_sessions SET end_time = datetime('now') WHERE end_time IS NULL",
            [],
        )?;
        Ok(n)
    }

    pub fn active_site_session(&self) -> Result<Option<SiteSession>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, site_id, start_time, end_time, start_lat, start_lon
             FROM site_sessions WHERE end_time IS NULL ORDER BY id DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map([], |row| {
            Ok(SiteSession {
                id: row.get(0)?,
                site_id: row.get(1)?,
                start_time: row.get(2)?,
                end_time: row.get(3)?,
                start_lat: row.get(4)?,
                start_lon: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    pub fn list_site_sessions(&self, site_id: i64, limit: usize) -> Result<Vec<SiteSession>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, site_id, start_time, end_time, start_lat, start_lon
             FROM site_sessions WHERE site_id = ?1 ORDER BY id DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], |row| {
            Ok(SiteSession {
                id: row.get(0)?,
                site_id: row.get(1)?,
                start_time: row.get(2)?,
                end_time: row.get(3)?,
                start_lat: row.get(4)?,
                start_lon: row.get(5)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn site_dashboard(&self, site_id: i64) -> Result<SiteDashboard, rusqlite::Error> {
        let conn = self.read_conn();
        let total_sessions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM site_sessions WHERE site_id = ?1", rusqlite::params![site_id], |r| r.get(0),
        )?;
        // Single query: compute all dashboard stats in one pass using the site_sessions CTE
        let (total_grants, unique_tgids, unique_uids, encrypted_grants): (i64, i64, i64, i64) = conn.query_row(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT COUNT(*),
                    COUNT(DISTINCT cg.tgid),
                    COUNT(DISTINCT CASE WHEN cg.uid > 0 THEN cg.uid END),
                    SUM(CASE WHEN nt.encrypted = 'encrypted' THEN 1 ELSE 0 END)
             FROM channel_grants cg
             LEFT JOIN network_talkgroups nt ON nt.tgid = cg.tgid AND nt.system = cg.system
             WHERE cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                 WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)",
            rusqlite::params![site_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, Option<i64>>(3)?.unwrap_or(0))),
        )?;
        let global_grants: i64 = conn.query_row(
            "SELECT COUNT(*) FROM channel_grants", [], |r| r.get(0),
        )?;
        let active_session = {
            let mut stmt = conn.prepare(
                "SELECT id, site_id, start_time, end_time, start_lat, start_lon
                 FROM site_sessions WHERE site_id = ?1 AND end_time IS NULL ORDER BY id DESC LIMIT 1"
            )?;
            let mut rows = stmt.query_map(rusqlite::params![site_id], |row| {
                Ok(SiteSession {
                    id: row.get(0)?,
                    site_id: row.get(1)?,
                    start_time: row.get(2)?,
                    end_time: row.get(3)?,
                    start_lat: row.get(4)?,
                    start_lon: row.get(5)?,
                })
            })?;
            match rows.next() {
                Some(r) => Some(r?),
                None => None,
            }
        };
        Ok(SiteDashboard {
            total_sessions, total_grants, unique_tgids, unique_uids,
            encrypted_grants, global_grants, active_session,
        })
    }

    pub fn site_talkgroups(&self, site_id: i64, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT cg.tgid, nt.name, nt.department, nt.encrypted, COUNT(*) as grants, MAX(cg.timestamp) as last_seen
             FROM channel_grants cg
             LEFT JOIN network_talkgroups nt ON nt.tgid = cg.tgid AND nt.system = cg.system
             WHERE cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                 WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)
             GROUP BY cg.tgid ORDER BY grants DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], |row| {
            Ok(serde_json::json!({
                "tgid": row.get::<_, i32>(0)?,
                "name": row.get::<_, Option<String>>(1)?,
                "department": row.get::<_, Option<String>>(2)?,
                "encrypted": row.get::<_, Option<String>>(3)?,
                "grants": row.get::<_, i64>(4)?,
                "last_seen": row.get::<_, Option<String>>(5)?,
            }))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn site_radio_ids(&self, site_id: i64, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT cg.uid, COUNT(*) as observations, MIN(cg.timestamp) as first_seen, MAX(cg.timestamp) as last_seen,
                    a.callsign, o.name as org_name
             FROM channel_grants cg
             LEFT JOIN actor_radio_ids ari ON ari.radio_id = cg.uid AND ari.system = cg.system
             LEFT JOIN actors a ON a.id = ari.actor_id
             LEFT JOIN organizations o ON o.id = a.organization_id
             WHERE cg.uid IS NOT NULL AND cg.uid > 0
               AND cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                 WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)
             GROUP BY cg.uid ORDER BY observations DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], |row| {
            Ok(serde_json::json!({
                "uid": row.get::<_, i32>(0)?,
                "observations": row.get::<_, i64>(1)?,
                "first_seen": row.get::<_, Option<String>>(2)?,
                "last_seen": row.get::<_, Option<String>>(3)?,
                "actor": row.get::<_, Option<String>>(4)?,
                "organization": row.get::<_, Option<String>>(5)?,
            }))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn site_encryption_timeline(&self, site_id: i64, hours: i64) -> Result<Vec<SiteEncryptionBucket>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT strftime('%Y-%m-%d %H:00', cg.timestamp) as hour_bucket,
                    SUM(CASE WHEN nt.encrypted = 'encrypted' THEN 1 ELSE 0 END) as encrypted,
                    SUM(CASE WHEN nt.encrypted != 'encrypted' OR nt.encrypted IS NULL THEN 1 ELSE 0 END) as clear
             FROM channel_grants cg
             LEFT JOIN network_talkgroups nt ON nt.tgid = cg.tgid AND nt.system = cg.system
             WHERE cg.timestamp >= datetime('now', ?2)
               AND cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                 WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)
             GROUP BY hour_bucket ORDER BY hour_bucket"
        )?;
        let offset = format!("-{} hours", hours);
        let rows = stmt.query_map(rusqlite::params![site_id, offset], |row| {
            Ok(SiteEncryptionBucket {
                hour: row.get(0)?,
                encrypted: row.get(1)?,
                clear: row.get(2)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn site_activity_by_hour(&self, site_id: i64) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT CAST(strftime('%H', cg.timestamp) AS INTEGER) as hour_of_day, COUNT(*) as grants
             FROM channel_grants cg
             WHERE cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                 WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)
             GROUP BY hour_of_day ORDER BY hour_of_day"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id], |row| {
            Ok(serde_json::json!({
                "hour": row.get::<_, i32>(0)?,
                "grants": row.get::<_, i64>(1)?,
            }))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn site_grants(&self, site_id: i64, limit: usize) -> Result<Vec<ChannelGrant>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT cg.id, cg.system, cg.tgid, cg.uid, cg.voice_freq, cg.grant_type, cg.timestamp
             FROM channel_grants cg
             WHERE cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                 WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)
             ORDER BY cg.timestamp DESC LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], Self::row_to_grant)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn link_radio_to_actor(&self, uid: i32, system: &str, actor_name: &str) -> Result<serde_json::Value, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO actors (callsign, identifier, description, actor_type, first_seen, last_seen)
             VALUES (?1, ?2, ?3, 'radio_unit', datetime('now'), datetime('now'))",
            rusqlite::params![actor_name, format!("UID:{}", uid), format!("Radio ID {} on {}", uid, system)],
        )?;
        let actor_id: i64 = conn.query_row(
            "SELECT id FROM actors WHERE identifier = ?1 ORDER BY id DESC LIMIT 1",
            rusqlite::params![format!("UID:{}", uid)], |r| r.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO actor_radio_ids (actor_id, radio_id, system, first_seen, last_seen)
             VALUES (?1, ?2, ?3, datetime('now'), datetime('now'))",
            rusqlite::params![actor_id, uid, system],
        )?;
        Ok(serde_json::json!({ "actor_id": actor_id }))
    }

    // ── SIGEX: HUMINT Observations ──

    pub fn create_humint_observation(
        &self, observer: Option<&str>, observation: &str, actor_id: Option<i64>,
        site_id: Option<i64>, freq_mhz: Option<f64>, tgid: Option<i32>, confidence: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO humint_observations (observer, observation, actor_id, site_id, freq_mhz, tgid, confidence, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
            rusqlite::params![observer, observation, actor_id, site_id, freq_mhz, tgid, confidence],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_humint_observations(&self, limit: usize) -> Result<Vec<HumintObservation>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, observer, observation, actor_id, site_id, freq_mhz, tgid, confidence, timestamp
             FROM humint_observations ORDER BY id DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(HumintObservation {
                id: row.get(0)?,
                observer: row.get(1)?,
                observation: row.get(2)?,
                actor_id: row.get(3)?,
                site_id: row.get(4)?,
                freq_mhz: row.get(5)?,
                tgid: row.get(6)?,
                confidence: row.get(7)?,
                timestamp: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── SIGEX: Dashboard Stats ──

    pub fn sigex_dashboard(&self) -> Result<SigexDashboard, rusqlite::Error> {
        let conn = self.read_conn();
        let total_sessions: i64 = conn.query_row("SELECT COUNT(*) FROM traffic_sessions", [], |r| r.get(0))?;
        let active_sessions: i64 = conn.query_row("SELECT COUNT(*) FROM traffic_sessions WHERE end_time IS NULL", [], |r| r.get(0))?;
        let total_events: i64 = conn.query_row("SELECT COUNT(*) FROM sigex_events", [], |r| r.get(0))?;
        let unacked_events: i64 = conn.query_row("SELECT COUNT(*) FROM sigex_events WHERE acknowledged = 0", [], |r| r.get(0))?;
        let total_actors: i64 = conn.query_row("SELECT COUNT(*) FROM actors", [], |r| r.get(0))?;
        let total_organizations: i64 = conn.query_row("SELECT COUNT(*) FROM organizations", [], |r| r.get(0))?;
        let total_sites: i64 = conn.query_row("SELECT COUNT(*) FROM intel_sites", [], |r| r.get(0))?;
        let events_today: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sigex_events WHERE timestamp >= date('now')", [], |r| r.get(0),
        )?;
        Ok(SigexDashboard {
            total_sessions,
            active_sessions,
            total_events,
            unacked_events,
            total_actors,
            total_organizations,
            total_sites,
            events_today,
        })
    }

    pub fn daily_collection_stats(&self) -> Result<DailyCollectionStats, rusqlite::Error> {
        let conn = self.read_conn();

        // Single compound query for all scalar counters — one round trip, minimal lock time
        let (signals_today, signals_total, grants_today, unique_tgs_today,
             unique_uids_today, encrypted_grants_today, key_rotations_today,
             active_wx_alerts, sessions_today, recordings_today, recordings_size_today
        ) = conn.query_row(
            "SELECT
               (SELECT COUNT(*) FROM signals WHERE last_seen >= date('now')),
               (SELECT COUNT(*) FROM signals),
               (SELECT COUNT(*) FROM channel_grants WHERE timestamp >= date('now')),
               (SELECT COUNT(DISTINCT tgid) FROM channel_grants WHERE timestamp >= date('now')),
               (SELECT COUNT(DISTINCT uid) FROM channel_grants WHERE timestamp >= date('now')),
               (SELECT COUNT(DISTINCT g.id) FROM channel_grants g JOIN network_talkgroups t ON g.tgid = t.tgid AND g.system = t.system WHERE g.timestamp >= date('now') AND t.encrypted = 1),
               (SELECT COUNT(*) FROM key_rotation_events WHERE timestamp >= date('now')),
               (SELECT COUNT(*) FROM wx_alerts WHERE expires_at IS NULL OR expires_at >= datetime('now')),
               (SELECT COUNT(*) FROM traffic_sessions WHERE start_time >= date('now')),
               (SELECT COUNT(*) FROM recordings WHERE start_time >= date('now')),
               (SELECT COALESCE(SUM(file_size_bytes), 0) FROM recordings WHERE start_time >= date('now'))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                     r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)),
        )?;

        // Two small GROUP BY queries
        let mut band_stmt = conn.prepare(
            "SELECT band, COUNT(*) FROM signals WHERE last_seen >= date('now') GROUP BY band ORDER BY COUNT(*) DESC"
        )?;
        let signals_by_band: Vec<BandCount> = band_stmt.query_map([], |r| {
            Ok(BandCount { band: r.get(0)?, count: r.get(1)? })
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut cls_stmt = conn.prepare(
            "SELECT cls, COUNT(*) FROM signals WHERE last_seen >= date('now') GROUP BY cls ORDER BY COUNT(*) DESC"
        )?;
        let signals_by_cls: Vec<ClsCount> = cls_stmt.query_map([], |r| {
            Ok(ClsCount { cls: r.get(0)?, count: r.get(1)? })
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut tg_stmt = conn.prepare(
            "SELECT g.tgid, t.name, t.department, COUNT(*) as cnt, \
             MAX(CASE WHEN g.grant_type = 'encrypted' THEN 1 ELSE 0 END) as enc \
             FROM channel_grants g \
             LEFT JOIN network_talkgroups t ON g.tgid = t.tgid AND g.system = t.system \
             WHERE g.timestamp >= date('now') \
             GROUP BY g.tgid ORDER BY cnt DESC LIMIT 10"
        )?;
        let top_talkgroups: Vec<TopTalkgroup> = tg_stmt.query_map([], |r| {
            Ok(TopTalkgroup {
                tgid: r.get(0)?,
                name: r.get(1)?,
                department: r.get(2)?,
                grants: r.get(3)?,
                encrypted: r.get::<_, i64>(4)? != 0,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        // Drop conn lock here (end of function)
        Ok(DailyCollectionStats {
            signals_today,
            signals_total,
            signals_by_band,
            signals_by_cls,
            grants_today,
            unique_tgs_today,
            unique_uids_today,
            encrypted_grants_today,
            key_rotations_today,
            active_wx_alerts,
            sessions_today,
            recordings_today,
            recordings_size_today,
            top_talkgroups,
        })
    }

    // ── SIGEX: Anomaly Events ──

    pub fn insert_anomaly_event(
        &self, event_type: &str, freq_mhz: Option<f64>, channel_id: Option<i64>,
        tgid: Option<i32>, uid: Option<i32>, system: Option<&str>,
        severity: &str, description: &str, anomaly_score: Option<f64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO anomaly_events (event_type, freq_mhz, channel_id, tgid, uid, system, severity, description, anomaly_score, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
            rusqlite::params![event_type, freq_mhz, channel_id, tgid, uid, system, severity, description, anomaly_score],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_anomaly_events(&self, limit: usize) -> Result<Vec<AnomalyEvent>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, event_type, freq_mhz, channel_id, tgid, uid, system, severity, description, anomaly_score, timestamp
             FROM anomaly_events ORDER BY id DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(AnomalyEvent {
                id: row.get(0)?,
                event_type: row.get(1)?,
                freq_mhz: row.get(2)?,
                channel_id: row.get(3)?,
                tgid: row.get(4)?,
                uid: row.get(5)?,
                system: row.get(6)?,
                severity: row.get(7)?,
                description: row.get(8)?,
                anomaly_score: row.get(9)?,
                timestamp: row.get(10)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Activity Baselines ──

    pub fn list_baselines(&self, limit: usize) -> Result<Vec<ActivityBaseline>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, freq_mhz, channel_id, tgid, system, hour_of_day, day_of_week,
                    avg_sessions, stddev_sessions, avg_duration, avg_unique_uids, sample_days, last_computed,
                    profile_name
             FROM activity_baselines ORDER BY last_computed DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(ActivityBaseline {
                id: row.get(0)?,
                freq_mhz: row.get(1)?,
                channel_id: row.get(2)?,
                tgid: row.get(3)?,
                system: row.get(4)?,
                hour_of_day: row.get(5)?,
                day_of_week: row.get(6)?,
                avg_sessions: row.get(7)?,
                stddev_sessions: row.get(8)?,
                avg_duration: row.get(9)?,
                avg_unique_uids: row.get(10)?,
                sample_days: row.get(11)?,
                last_computed: row.get(12)?,
                profile_name: row.get(13)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_baseline_profiles(&self) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT profile_name FROM activity_baselines ORDER BY profile_name"
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn compute_baseline_snapshot(&self) -> Result<usize, rusqlite::Error> {
        self.compute_baseline_snapshot_named("default")
    }

    pub fn compute_baseline_snapshot_named(&self, profile_name: &str) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        // Remove existing entries for this profile
        conn.execute(
            "DELETE FROM activity_baselines WHERE profile_name = ?1",
            rusqlite::params![profile_name],
        )?;
        // Compute hour-of-week activity from traffic_sessions over the last 30 days
        let inserted = conn.execute(
            "INSERT INTO activity_baselines (freq_mhz, channel_id, tgid, system, hour_of_day, day_of_week,
                avg_sessions, stddev_sessions, avg_duration, avg_unique_uids, sample_days, last_computed, profile_name)
             SELECT
                freq_mhz, channel_id, tgid, system,
                CAST(strftime('%H', start_time) AS INTEGER) as hod,
                CAST(strftime('%w', start_time) AS INTEGER) as dow,
                COUNT(*) * 1.0 / MAX(1, COUNT(DISTINCT date(start_time))) as avg_sess,
                0.0 as stddev_sess,
                AVG(COALESCE(duration_sec, 0)) as avg_dur,
                COUNT(DISTINCT uid) * 1.0 / MAX(1, COUNT(DISTINCT date(start_time))) as avg_uids,
                COUNT(DISTINCT date(start_time)) as sample_d,
                datetime('now'),
                ?1
             FROM traffic_sessions
             WHERE start_time >= datetime('now', '-30 days')
             GROUP BY freq_mhz, channel_id, tgid, system, hod, dow",
            rusqlite::params![profile_name],
        )?;
        Ok(inserted)
    }

    pub fn baseline_count(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.read_conn();
        conn.query_row("SELECT COUNT(*) FROM activity_baselines", [], |r| r.get(0))
    }

    pub fn baseline_last_computed(&self) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT MAX(last_computed) FROM activity_baselines", [], |r| r.get(0),
        )?;
        Ok(result)
    }

    pub fn clear_baselines(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute("DELETE FROM activity_baselines", [])?;
        Ok(n)
    }

    // ── SIGEX: Encryption Keys ──

    pub fn upsert_encryption_key(
        &self, tgid: i32, system: &str, algorithm_id: Option<i32>,
        algorithm_name: Option<&str>, key_id: Option<i32>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        // Check for existing entry with same tgid+system+alg+key
        let existing: Option<i64> = conn.query_row(
            "SELECT id FROM encryption_keys WHERE tgid = ?1 AND system = ?2
             AND COALESCE(algorithm_id, -1) = COALESCE(?3, -1)
             AND COALESCE(key_id, -1) = COALESCE(?4, -1)",
            rusqlite::params![tgid, system, algorithm_id, key_id],
            |row| row.get(0),
        ).ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE encryption_keys SET last_seen = datetime('now'), session_count = session_count + 1 WHERE id = ?1",
                rusqlite::params![id],
            )?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO encryption_keys (tgid, system, algorithm_id, algorithm_name, key_id, first_seen, last_seen, session_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'), 1)",
                rusqlite::params![tgid, system, algorithm_id, algorithm_name, key_id],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }

    pub fn list_encryption_keys(&self, limit: usize, system_filter: Option<&str>) -> Result<Vec<EncryptionKeyEntry>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(sys) = system_filter {
            (
                "SELECT id, tgid, system, algorithm_id, algorithm_name, key_id, first_seen, last_seen, session_count
                 FROM encryption_keys WHERE system = ?1 ORDER BY tgid, last_seen DESC LIMIT ?2".into(),
                vec![Box::new(sys.to_string()) as Box<dyn rusqlite::types::ToSql>, Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, tgid, system, algorithm_id, algorithm_name, key_id, first_seen, last_seen, session_count
                 FROM encryption_keys ORDER BY tgid, last_seen DESC LIMIT ?1".into(),
                vec![Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(EncryptionKeyEntry {
                id: row.get(0)?,
                tgid: row.get(1)?,
                system: row.get(2)?,
                algorithm_id: row.get(3)?,
                algorithm_name: row.get(4)?,
                key_id: row.get(5)?,
                first_seen: row.get(6)?,
                last_seen: row.get(7)?,
                session_count: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert_key_rotation(
        &self, tgid: i32, system: &str, old_key_id: Option<i32>, new_key_id: Option<i32>,
        old_algorithm: Option<i32>, new_algorithm: Option<i32>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO key_rotation_events (tgid, system, old_key_id, new_key_id, old_algorithm, new_algorithm, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
            rusqlite::params![tgid, system, old_key_id, new_key_id, old_algorithm, new_algorithm],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_key_rotations(&self, limit: usize, tgid_filter: Option<i32>) -> Result<Vec<KeyRotationEvent>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(tgid) = tgid_filter {
            (
                "SELECT id, tgid, system, old_key_id, new_key_id, old_algorithm, new_algorithm, timestamp
                 FROM key_rotation_events WHERE tgid = ?1 ORDER BY id DESC LIMIT ?2".into(),
                vec![Box::new(tgid) as Box<dyn rusqlite::types::ToSql>, Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, tgid, system, old_key_id, new_key_id, old_algorithm, new_algorithm, timestamp
                 FROM key_rotation_events ORDER BY id DESC LIMIT ?1".into(),
                vec![Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(KeyRotationEvent {
                id: row.get(0)?,
                tgid: row.get(1)?,
                system: row.get(2)?,
                old_key_id: row.get(3)?,
                new_key_id: row.get(4)?,
                old_algorithm: row.get(5)?,
                new_algorithm: row.get(6)?,
                timestamp: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Get encryption posture summary per TGID.
    /// All queries use a single lock acquisition to avoid deadlock.
    pub fn encryption_posture(&self, limit: usize) -> Result<Vec<EncryptionPosture>, rusqlite::Error> {
        let conn = self.read_conn();

        // Get distinct TGIDs
        let mut tg_stmt = conn.prepare(
            "SELECT DISTINCT tgid, system FROM encryption_keys ORDER BY tgid LIMIT ?1"
        )?;
        let tg_rows: Vec<(i32, String)> = tg_stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?.collect::<Result<Vec<_>, _>>()?;

        // Fetch all encryption keys at once (single query, no nested lock)
        let mut key_stmt = conn.prepare(
            "SELECT tgid, system, algorithm_id, algorithm_name, key_id, session_count
             FROM encryption_keys ORDER BY tgid"
        )?;
        let all_keys: Vec<(i32, String, Option<i32>, Option<String>, Option<i32>, i64)> =
            key_stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
            })?.collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for (tgid, system) in tg_rows {
            let mut algorithms = Vec::new();
            let mut key_ids = Vec::new();
            let mut total = 0i64;
            let mut encrypted = 0i64;
            let mut clear = 0i64;

            for &(ref k_tgid, ref k_sys, alg_id_opt, ref alg_name, kid, sessions) in &all_keys {
                if *k_tgid != tgid || *k_sys != system { continue; }
                total += sessions;
                let alg_id = alg_id_opt.unwrap_or(0x80);
                if alg_id == 0x80 || alg_id == 0 {
                    clear += sessions;
                } else {
                    encrypted += sessions;
                }
                if let Some(name) = alg_name {
                    if !algorithms.contains(name) {
                        algorithms.push(name.clone());
                    }
                }
                if let Some(k) = kid {
                    if !key_ids.contains(&k) {
                        key_ids.push(k);
                    }
                }
            }

            let posture = if encrypted == 0 { "clear" }
                else if clear == 0 { "encrypted" }
                else { "mixed" }.to_string();

            let last_rotation: Option<String> = conn.query_row(
                "SELECT timestamp FROM key_rotation_events WHERE tgid = ?1 AND system = ?2 ORDER BY id DESC LIMIT 1",
                rusqlite::params![tgid, system],
                |row| row.get(0),
            ).ok();

            result.push(EncryptionPosture {
                tgid, system, posture, algorithms, key_ids,
                total_sessions: total, encrypted_sessions: encrypted,
                clear_sessions: clear, last_rotation,
            });
        }
        Ok(result)
    }

    // ── SIGEX: Radio ID Sightings ──

    pub fn upsert_radio_id_sighting(
        &self, uid: i32, tgid: Option<i32>, system: Option<&str>, _freq_mhz: Option<f64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let existing: Option<i64> = conn.query_row(
            "SELECT id FROM uid_fingerprint_map WHERE uid = ?1 AND COALESCE(system, '') = COALESCE(?2, '')",
            rusqlite::params![uid, system],
            |row| row.get(0),
        ).ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE uid_fingerprint_map SET tgid = COALESCE(?1, tgid), observation_count = observation_count + 1, last_seen = datetime('now') WHERE id = ?2",
                rusqlite::params![tgid, id],
            )?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO uid_fingerprint_map (uid, fingerprint_id, tgid, system, observation_count, first_seen, last_seen)
                 VALUES (?1, '', ?2, ?3, 1, datetime('now'), datetime('now'))",
                rusqlite::params![uid, tgid, system],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }

    pub fn list_radio_id_sightings(&self, limit: usize) -> Result<Vec<RadioIdSighting>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, uid, tgid, system, NULL as freq_mhz, first_seen, last_seen, observation_count
             FROM uid_fingerprint_map ORDER BY last_seen DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(RadioIdSighting {
                id: row.get(0)?,
                uid: row.get(1)?,
                tgid: row.get(2)?,
                system: row.get(3)?,
                freq_mhz: row.get(4)?,
                first_seen: row.get(5)?,
                last_seen: row.get(6)?,
                observation_count: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Network: Sites ──

    pub fn upsert_network_site(
        &self, system: &str, wacn: Option<i64>, system_id: Option<i64>,
        rfss_id: Option<i64>, site_id: Option<i64>, control_channel: Option<f64>,
        alt_control: Option<&str>, voice_channels: Option<&str>, adjacent_sites: Option<&str>,
        name: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        // Check for existing site with same system + rfss + site
        let existing: Option<i64> = conn.query_row(
            "SELECT id FROM network_sites WHERE system = ?1 AND COALESCE(rfss_id, -1) = COALESCE(?2, -1) AND COALESCE(site_id, -1) = COALESCE(?3, -1)",
            rusqlite::params![system, rfss_id, site_id],
            |row| row.get(0),
        ).ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE network_sites SET wacn = COALESCE(?1, wacn), system_id = COALESCE(?2, system_id),
                 control_channel = COALESCE(?3, control_channel), alt_control = COALESCE(?4, alt_control),
                 voice_channels = COALESCE(?5, voice_channels), adjacent_sites = COALESCE(?6, adjacent_sites),
                 name = COALESCE(?7, name),
                 last_seen = datetime('now') WHERE id = ?8",
                rusqlite::params![wacn, system_id, control_channel, alt_control, voice_channels, adjacent_sites, name, id],
            )?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO network_sites (system, wacn, system_id, rfss_id, site_id, control_channel, alt_control, voice_channels, adjacent_sites, name, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'), datetime('now'))",
                rusqlite::params![system, wacn, system_id, rfss_id, site_id, control_channel, alt_control, voice_channels, adjacent_sites, name],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }

    pub fn list_network_sites(&self, system_filter: Option<&str>, limit: i64) -> Result<Vec<NetworkSite>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(sys) = system_filter {
            (
                "SELECT id, system, wacn, system_id, rfss_id, site_id, name, control_channel, alt_control, voice_channels, adjacent_sites, first_seen, last_seen
                 FROM network_sites WHERE system = ?1 ORDER BY last_seen DESC LIMIT ?2".into(),
                vec![Box::new(sys.to_string()) as Box<dyn rusqlite::types::ToSql>, Box::new(limit)],
            )
        } else {
            (
                "SELECT id, system, wacn, system_id, rfss_id, site_id, name, control_channel, alt_control, voice_channels, adjacent_sites, first_seen, last_seen
                 FROM network_sites ORDER BY last_seen DESC LIMIT ?1".into(),
                vec![Box::new(limit) as Box<dyn rusqlite::types::ToSql>],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(NetworkSite {
                id: row.get(0)?,
                system: row.get(1)?,
                wacn: row.get(2)?,
                system_id: row.get(3)?,
                rfss_id: row.get(4)?,
                site_id: row.get(5)?,
                name: row.get(6)?,
                control_channel: row.get(7)?,
                alt_control: row.get(8)?,
                voice_channels: row.get(9)?,
                adjacent_sites: row.get(10)?,
                first_seen: row.get(11)?,
                last_seen: row.get(12)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Get all CC frequencies for a P25 system (primary + alternates from all sites).
    /// Returns a deduplicated, sorted Vec of frequencies in MHz.
    pub fn get_all_cc_frequencies(&self, system: &str) -> Result<Vec<f64>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT control_channel, alt_control FROM network_sites WHERE system = ?1",
        )?;
        let mut freqs: Vec<f64> = Vec::new();
        let rows = stmt.query_map(rusqlite::params![system], |row| {
            let primary: Option<f64> = row.get(0)?;
            let alt_json: Option<String> = row.get(1)?;
            Ok((primary, alt_json))
        })?;
        for row in rows {
            let (primary, alt_json) = row?;
            if let Some(freq) = primary {
                if freq > 0.0 {
                    freqs.push(freq);
                }
            }
            if let Some(json_str) = alt_json {
                // alt_control is a JSON array of objects: [{"freq_mhz": 772.60625}, ...]
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json_str) {
                    for item in arr {
                        if let Some(f) = item.get("freq_mhz").and_then(|v| v.as_f64()) {
                            if f > 0.0 {
                                freqs.push(f);
                            }
                        }
                    }
                }
            }
        }
        freqs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        freqs.dedup_by(|a, b| (*a - *b).abs() < 0.0001);
        Ok(freqs)
    }

    // ── Network: Talkgroups ──

    pub fn upsert_network_talkgroup(
        &self, system: &str, tgid: i32, encrypted: bool, algorithm: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let existing: Option<i64> = conn.query_row(
            "SELECT id FROM network_talkgroups WHERE system = ?1 AND tgid = ?2",
            rusqlite::params![system, tgid],
            |row| row.get(0),
        ).ok();

        let enc_str = if encrypted { "encrypted" } else { "clear" };

        if let Some(id) = existing {
            // Preserve name/department/tag from seed data — only update grant stats
            conn.prepare_cached(
                "UPDATE network_talkgroups SET total_grants = total_grants + 1,
                 encrypted = CASE WHEN ?1 != encrypted AND encrypted != 'unknown' THEN 'mixed' ELSE ?1 END,
                 algorithm = COALESCE(?2, algorithm),
                 last_seen = datetime('now') WHERE id = ?3",
            )?.execute(rusqlite::params![enc_str, algorithm, id])?;
            Ok(id)
        } else {
            conn.prepare_cached(
                "INSERT INTO network_talkgroups (system, tgid, encrypted, algorithm, total_grants, unique_uids, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, 1, 0, datetime('now'), datetime('now'))",
            )?.execute(rusqlite::params![system, tgid, enc_str, algorithm])?;
            Ok(conn.last_insert_rowid())
        }
    }

    pub fn list_network_talkgroups(&self, system_filter: Option<&str>, limit: i64) -> Result<Vec<NetworkTalkgroup>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(sys) = system_filter {
            (
                "SELECT id, system, tgid, name, department, tag, encrypted, algorithm, total_grants, unique_uids, first_seen, last_seen, priority, scan_enabled
                 FROM network_talkgroups WHERE system = ?1 ORDER BY total_grants DESC LIMIT ?2".into(),
                vec![Box::new(sys.to_string()) as Box<dyn rusqlite::types::ToSql>, Box::new(limit)],
            )
        } else {
            (
                "SELECT id, system, tgid, name, department, tag, encrypted, algorithm, total_grants, unique_uids, first_seen, last_seen, priority, scan_enabled
                 FROM network_talkgroups ORDER BY total_grants DESC LIMIT ?1".into(),
                vec![Box::new(limit) as Box<dyn rusqlite::types::ToSql>],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(NetworkTalkgroup {
                id: row.get(0)?,
                system: row.get(1)?,
                tgid: row.get(2)?,
                name: row.get(3)?,
                department: row.get(4)?,
                tag: row.get(5)?,
                encrypted: row.get(6)?,
                algorithm: row.get(7)?,
                total_grants: row.get(8)?,
                unique_uids: row.get(9)?,
                first_seen: row.get(10)?,
                last_seen: row.get(11)?,
                priority: row.get(12)?,
                scan_enabled: row.get(13)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_talkgroup_priority(&self, tgid: i32, system: &str, priority: i32) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE network_talkgroups SET priority = ?1 WHERE tgid = ?2 AND system = ?3",
            rusqlite::params![priority, tgid, system],
        )?;
        Ok(n > 0)
    }

    pub fn set_talkgroup_scan_enabled(&self, tgid: i32, system: &str, enabled: bool) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE network_talkgroups SET scan_enabled = ?1 WHERE tgid = ?2 AND system = ?3",
            rusqlite::params![enabled as i32, tgid, system],
        )?;
        Ok(n > 0)
    }

    pub fn set_department_scan_enabled(&self, department: &str, system: &str, enabled: bool) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE network_talkgroups SET scan_enabled = ?1 WHERE department = ?2 AND system = ?3",
            rusqlite::params![enabled as i32, department, system],
        )?;
        Ok(n)
    }

    pub fn list_departments(&self, system: &str) -> Result<Vec<DepartmentSummary>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(department, ''), COUNT(*), SUM(scan_enabled)
             FROM network_talkgroups WHERE system = ?1
             GROUP BY department ORDER BY department"
        )?;
        let rows = stmt.query_map(rusqlite::params![system], |row| {
            Ok(DepartmentSummary {
                department: row.get(0)?,
                tg_count: row.get(1)?,
                watched_count: row.get(2)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_watched_tgids(&self, system: &str) -> Result<Vec<u32>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tgid FROM network_talkgroups WHERE system = ?1 AND scan_enabled = 1"
        )?;
        let rows = stmt.query_map(rusqlite::params![system], |row| {
            row.get::<_, i32>(0).map(|t| t as u32)
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_talkgroup_priority(&self, tgid: i32, system: &str) -> Result<i32, rusqlite::Error> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COALESCE(priority, 0) FROM network_talkgroups WHERE tgid = ?1 AND system = ?2",
            rusqlite::params![tgid, system],
            |row| row.get(0),
        ).or(Ok(0))
    }

    /// Load all tgid→priority mappings for hot-path priority cache.
    pub fn load_priority_cache(&self, system: &str) -> Result<std::collections::HashMap<u32, u8>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tgid, COALESCE(priority, 0) FROM network_talkgroups WHERE system = ?1"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map(rusqlite::params![system], |row| {
            Ok((row.get::<_, i32>(0)? as u32, row.get::<_, i32>(1)? as u8))
        })?;
        for r in rows {
            let (tgid, prio) = r?;
            map.insert(tgid, prio);
        }
        Ok(map)
    }

    pub fn load_dept_cache(&self, system: &str) -> Result<std::collections::HashMap<u32, String>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tgid, COALESCE(department, '') FROM network_talkgroups WHERE system = ?1"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map(rusqlite::params![system], |row| {
            Ok((row.get::<_, i32>(0)? as u32, row.get::<_, String>(1)?))
        })?;
        for r in rows {
            let (tgid, dept) = r?;
            if !dept.is_empty() {
                map.insert(tgid, dept);
            }
        }
        Ok(map)
    }

    // ── Network: Channel Grants ──

    const GRANT_COLS: &str = "id, system, tgid, uid, voice_freq, grant_type, timestamp";

    fn row_to_grant(row: &rusqlite::Row) -> Result<ChannelGrant, rusqlite::Error> {
        Ok(ChannelGrant {
            id: row.get(0)?,
            system: row.get(1)?,
            tgid: row.get(2)?,
            uid: row.get(3)?,
            voice_freq: row.get(4)?,
            grant_type: row.get(5)?,
            timestamp: row.get(6)?,
        })
    }

    pub fn insert_channel_grant(
        &self, system: &str, tgid: i32, uid: Option<i32>, voice_freq: Option<f64>, grant_type: Option<&str>,
        operation_id: Option<i64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.prepare_cached(
            "INSERT INTO channel_grants (system, tgid, uid, voice_freq, grant_type, timestamp, operation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), ?6)",
        )?.execute(rusqlite::params![system, tgid, uid, voice_freq, grant_type, operation_id])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_channel_grants(&self, system_filter: Option<&str>, tgid_filter: Option<i32>, since: Option<&str>, limit: i64) -> Result<Vec<ChannelGrant>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut sql = format!("SELECT {} FROM channel_grants WHERE 1=1", Self::GRANT_COLS);
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;
        if let Some(sys) = system_filter {
            sql.push_str(&format!(" AND system = ?{}", idx));
            params.push(Box::new(sys.to_string()));
            idx += 1;
        }
        if let Some(tgid) = tgid_filter {
            sql.push_str(&format!(" AND tgid = ?{}", idx));
            params.push(Box::new(tgid));
            idx += 1;
        }
        if let Some(since_ts) = since {
            sql.push_str(&format!(" AND timestamp >= ?{}", idx));
            params.push(Box::new(since_ts.to_string()));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", idx));
        params.push(Box::new(limit));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), Self::row_to_grant)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Network: Radio Affiliations ──

    pub fn insert_radio_affiliation(
        &self, system: &str, uid: i32, tgid: i32, event_type: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO radio_affiliations (system, uid, tgid, event_type, timestamp)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            rusqlite::params![system, uid, tgid, event_type],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_radio_affiliations(&self, system_filter: Option<&str>, uid_filter: Option<i32>, limit: i64) -> Result<Vec<RadioAffiliation>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut sql = String::from(
            "SELECT id, system, uid, tgid, event_type, timestamp FROM radio_affiliations WHERE 1=1"
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;
        if let Some(sys) = system_filter {
            sql.push_str(&format!(" AND system = ?{}", idx));
            params.push(Box::new(sys.to_string()));
            idx += 1;
        }
        if let Some(uid) = uid_filter {
            sql.push_str(&format!(" AND uid = ?{}", idx));
            params.push(Box::new(uid));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", idx));
        params.push(Box::new(limit));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(RadioAffiliation {
                id: row.get(0)?,
                system: row.get(1)?,
                uid: row.get(2)?,
                tgid: row.get(3)?,
                event_type: row.get(4)?,
                timestamp: row.get(5)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Network: Summary ──

    pub fn network_summary(&self, system_filter: Option<&str>) -> Result<NetworkSummary, rusqlite::Error> {
        let conn = self.read_conn();
        let (where_clause, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(sys) = system_filter {
            ("WHERE system = ?1", vec![Box::new(sys.to_string()) as Box<dyn rusqlite::types::ToSql>])
        } else {
            ("", vec![])
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let total_sites: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM network_sites {}", where_clause),
            param_refs.as_slice(), |r| r.get(0),
        )?;
        let total_talkgroups: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM network_talkgroups {}", where_clause),
            param_refs.as_slice(), |r| r.get(0),
        )?;
        let total_grants: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM channel_grants {}", where_clause),
            param_refs.as_slice(), |r| r.get(0),
        )?;
        let total_affiliations: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM radio_affiliations {}", where_clause),
            param_refs.as_slice(), |r| r.get(0),
        )?;
        let unique_uids: i64 = conn.query_row(
            &format!("SELECT COUNT(DISTINCT uid) FROM radio_affiliations {}", where_clause),
            param_refs.as_slice(), |r| r.get(0),
        )?;
        let grants_last_hour: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM channel_grants {} {} timestamp >= datetime('now', '-1 hour')",
                where_clause,
                if where_clause.is_empty() { "WHERE" } else { "AND" }),
            param_refs.as_slice(), |r| r.get(0),
        )?;

        Ok(NetworkSummary {
            total_sites,
            total_talkgroups,
            total_grants,
            total_affiliations,
            unique_uids,
            grants_last_hour,
        })
    }

    /// List signals, optionally filtered by band and/or class.
    pub fn list_signals(
        &self,
        band_filter: Option<&str>,
        cls_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Signal>, rusqlite::Error> {
        let conn = self.read_conn();

        let mut sql = String::from(
            "SELECT id, freq, name, cls, band, mode, first_seen, last_seen, total_hits FROM signals WHERE 1=1"
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(band) = band_filter {
            sql.push_str(&format!(" AND band = ?{}", idx));
            params.push(Box::new(band.to_string()));
            idx += 1;
        }
        if let Some(cls) = cls_filter {
            sql.push_str(&format!(" AND cls = ?{}", idx));
            params.push(Box::new(cls.to_string()));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY total_hits DESC LIMIT ?{}", idx));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let signals = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(Signal {
                id: row.get(0)?,
                freq: row.get(1)?,
                name: row.get(2)?,
                cls: row.get(3)?,
                band: row.get(4)?,
                mode: row.get(5)?,
                first_seen: row.get(6)?,
                last_seen: row.get(7)?,
                total_hits: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(signals)
    }

    // --- SDR Devices ---

    /// Upsert a device record on enumeration. Updates last_seen and metadata on existing rows.
    pub fn upsert_device(
        &self,
        serial: &str,
        manufacturer: &str,
        product: &str,
        tuner: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sdr_devices (serial, manufacturer, product, tuner, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))
             ON CONFLICT(serial) DO UPDATE SET
                manufacturer = CASE WHEN ?2 != '' THEN ?2 ELSE sdr_devices.manufacturer END,
                product      = CASE WHEN ?3 != '' THEN ?3 ELSE sdr_devices.product END,
                tuner        = CASE WHEN ?4 != '' THEN ?4 ELSE sdr_devices.tuner END,
                last_seen    = datetime('now')",
            rusqlite::params![serial, manufacturer, product, tuner],
        )?;
        Ok(())
    }

    /// Set the user-chosen display name for a device serial.
    pub fn set_device_name(&self, serial: &str, name: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sdr_devices (serial, user_name) VALUES (?1, ?2)
             ON CONFLICT(serial) DO UPDATE SET user_name = ?2",
            rusqlite::params![serial, name],
        )?;
        Ok(())
    }

    /// Bulk-fetch non-empty user_name for all known devices (serial → name).
    pub fn get_device_names(&self) -> Result<std::collections::HashMap<String, String>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT serial, user_name FROM sdr_devices WHERE user_name != ''"
        )?;
        let map = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?.collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        Ok(map)
    }

    /// Get all known device serials (for generating unique EEPROM serials).
    pub fn get_device_serials(&self) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT serial FROM sdr_devices")?;
        let serials = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(serials)
    }

    // ── WX Alerts (SAME decode) ──────────────────────────────────────

    pub fn insert_wx_alert(
        &self,
        originator: &str, event_code: &str, event_name: &str,
        severity: &str, locations: &str, duration_mins: Option<i32>,
        issued_utc: Option<&str>, station: Option<&str>,
        raw_header: &str, confidence: Option<f64>,
        expires_at: Option<&str>, freq_mhz: Option<f64>,
        receiver_lat: Option<f64>, receiver_lon: Option<f64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO wx_alerts (originator, event_code, event_name, severity, locations,
             duration_mins, issued_utc, station, raw_header, confidence,
             expires_at, freq_mhz, receiver_lat, receiver_lon)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                originator, event_code, event_name, severity, locations,
                duration_mins, issued_utc, station, raw_header, confidence,
                expires_at, freq_mhz, receiver_lat, receiver_lon,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_wx_alerts(&self, limit: usize) -> Result<Vec<WxAlert>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, originator, event_code, event_name, severity, locations,
                    duration_mins, issued_utc, station, raw_header, confidence,
                    received_at, expires_at, freq_mhz, receiver_lat, receiver_lon
             FROM wx_alerts ORDER BY received_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |r| {
            Ok(WxAlert {
                id: r.get(0)?,
                originator: r.get(1)?,
                event_code: r.get(2)?,
                event_name: r.get(3)?,
                severity: r.get(4)?,
                locations: r.get(5)?,
                duration_mins: r.get(6)?,
                issued_utc: r.get(7)?,
                station: r.get(8)?,
                raw_header: r.get(9)?,
                confidence: r.get(10)?,
                received_at: r.get(11)?,
                expires_at: r.get(12)?,
                freq_mhz: r.get(13)?,
                receiver_lat: r.get(14)?,
                receiver_lon: r.get(15)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_wx_alert(&self, id: i64) -> Result<Option<WxAlert>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, originator, event_code, event_name, severity, locations,
                    duration_mins, issued_utc, station, raw_header, confidence,
                    received_at, expires_at, freq_mhz, receiver_lat, receiver_lon
             FROM wx_alerts WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], |r| {
            Ok(WxAlert {
                id: r.get(0)?,
                originator: r.get(1)?,
                event_code: r.get(2)?,
                event_name: r.get(3)?,
                severity: r.get(4)?,
                locations: r.get(5)?,
                duration_mins: r.get(6)?,
                issued_utc: r.get(7)?,
                station: r.get(8)?,
                raw_header: r.get(9)?,
                confidence: r.get(10)?,
                received_at: r.get(11)?,
                expires_at: r.get(12)?,
                freq_mhz: r.get(13)?,
                receiver_lat: r.get(14)?,
                receiver_lon: r.get(15)?,
            })
        })?;
        match rows.next() {
            Some(Ok(alert)) => Ok(Some(alert)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    // ── Department Detail Queries ──

    pub fn department_detail(&self, department: &str, system: &str) -> Result<DepartmentDetail, rusqlite::Error> {
        let conn = self.read_conn();
        // Fetch talkgroups for this department
        let mut stmt = conn.prepare(
            "SELECT id, system, tgid, name, department, tag, encrypted, algorithm,
                    total_grants, unique_uids, first_seen, last_seen, priority, scan_enabled
             FROM network_talkgroups
             WHERE system = ?1 AND COALESCE(department, '') = ?2
             ORDER BY tgid"
        )?;
        let talkgroups: Vec<NetworkTalkgroup> = stmt.query_map(rusqlite::params![system, department], |row| {
            Ok(NetworkTalkgroup {
                id: row.get(0)?,
                system: row.get(1)?,
                tgid: row.get(2)?,
                name: row.get(3)?,
                department: row.get(4)?,
                tag: row.get(5)?,
                encrypted: row.get(6)?,
                algorithm: row.get(7)?,
                total_grants: row.get(8)?,
                unique_uids: row.get(9)?,
                first_seen: row.get(10)?,
                last_seen: row.get(11)?,
                priority: row.get(12)?,
                scan_enabled: row.get(13)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        // Aggregate stats from talkgroup rows
        let tg_count = talkgroups.len() as i64;
        let watched_count = talkgroups.iter().filter(|t| t.scan_enabled).count() as i64;
        let total_grants: i64 = talkgroups.iter().map(|t| t.total_grants).sum();
        let encrypted_tgs = talkgroups.iter().filter(|t| t.encrypted != "clear" && t.encrypted != "").count() as i64;
        let first_seen = talkgroups.iter().map(|t| &t.first_seen).filter(|s| !s.is_empty()).min().cloned();
        let last_seen = talkgroups.iter().map(|t| &t.last_seen).filter(|s| !s.is_empty()).max().cloned();

        // Unique UIDs across all TGs in department via channel_grants
        let unique_uids: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT cg.uid)
             FROM channel_grants cg
             JOIN network_talkgroups nt ON cg.tgid = nt.tgid AND cg.system = nt.system
             WHERE nt.system = ?1 AND COALESCE(nt.department, '') = ?2 AND cg.uid IS NOT NULL",
            rusqlite::params![system, department],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(DepartmentDetail {
            department: department.to_string(),
            tg_count,
            watched_count,
            total_grants,
            unique_uids,
            encrypted_tgs,
            first_seen,
            last_seen,
            talkgroups,
        })
    }

    pub fn department_activity(&self, department: &str, system: &str) -> Result<Vec<ActivityBucket>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT CAST(strftime('%w', cg.timestamp) AS INTEGER),
                    CAST(strftime('%H', cg.timestamp) AS INTEGER),
                    COUNT(*)
             FROM channel_grants cg
             JOIN network_talkgroups nt ON cg.tgid = nt.tgid AND cg.system = nt.system
             WHERE nt.system = ?1 AND COALESCE(nt.department, '') = ?2
             GROUP BY 1, 2 ORDER BY 1, 2"
        )?;
        let buckets = stmt.query_map(rusqlite::params![system, department], |row| {
            Ok(ActivityBucket {
                day_of_week: row.get(0)?,
                hour: row.get(1)?,
                grant_count: row.get(2)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(buckets)
    }

    pub fn department_radios(&self, department: &str, system: &str, limit: i64) -> Result<Vec<DepartmentRadio>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT cg.uid, COUNT(*), COUNT(DISTINCT cg.tgid),
                    MIN(cg.timestamp), MAX(cg.timestamp)
             FROM channel_grants cg
             JOIN network_talkgroups nt ON cg.tgid = nt.tgid AND cg.system = nt.system
             WHERE nt.system = ?1 AND COALESCE(nt.department, '') = ?2 AND cg.uid IS NOT NULL
             GROUP BY cg.uid ORDER BY 2 DESC LIMIT ?3"
        )?;
        let radios = stmt.query_map(rusqlite::params![system, department, limit], |row| {
            Ok(DepartmentRadio {
                uid: row.get(0)?,
                observation_count: row.get(1)?,
                tg_count: row.get(2)?,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(radios)
    }

    // --- Recordings ---

    // --- Recordings ---

    const RECORDING_COLS: &str =
        "id, rec_type, freq_mhz, modulation, label, sample_rate, channels, file_path, \
         file_size_bytes, duration_sec, start_time, end_time, trigger_type, \
         tgid, device_key, receiver_lat, receiver_lon, notes, site_id, site_session_id, operation_id, \
         source_unit, encrypted, algorithm, key_id";

    fn row_to_recording(row: &rusqlite::Row) -> Result<Recording, rusqlite::Error> {
        Ok(Recording {
            id: row.get(0)?,
            rec_type: row.get(1)?,
            freq_mhz: row.get(2)?,
            modulation: row.get(3)?,
            label: row.get(4)?,
            sample_rate: row.get(5)?,
            channels: row.get(6)?,
            file_path: row.get(7)?,
            file_size_bytes: row.get(8)?,
            duration_sec: row.get(9)?,
            start_time: row.get(10)?,
            end_time: row.get(11)?,
            trigger_type: row.get(12)?,
            tgid: row.get(13)?,
            device_key: row.get(14)?,
            receiver_lat: row.get(15)?,
            receiver_lon: row.get(16)?,
            notes: row.get(17)?,
            site_id: row.get(18)?,
            site_session_id: row.get(19)?,
            operation_id: row.get(20)?,
            source_unit: row.get(21)?,
            encrypted: row.get::<_, Option<bool>>(22)?.unwrap_or(false),
            algorithm: row.get(23)?,
            key_id: row.get(24)?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_recording(
        &self,
        rec_type: &str,
        freq_mhz: f64,
        modulation: Option<&str>,
        label: Option<&str>,
        sample_rate: i32,
        file_path: &str,
        trigger_type: &str,
        tgid: Option<i32>,
        device_key: Option<&str>,
        lat: Option<f64>,
        lon: Option<f64>,
        site_id: Option<i64>,
        site_session_id: Option<i64>,
        operation_id: Option<i64>,
        source_unit: Option<i32>,
        encrypted: bool,
        algorithm: Option<&str>,
        key_id: Option<i32>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO recordings (rec_type, freq_mhz, modulation, label, sample_rate, file_path, \
             trigger_type, tgid, device_key, receiver_lat, receiver_lon, site_id, site_session_id, \
             operation_id, source_unit, encrypted, algorithm, key_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            rusqlite::params![rec_type, freq_mhz, modulation, label, sample_rate, file_path,
                trigger_type, tgid, device_key, lat, lon, site_id, site_session_id,
                operation_id, source_unit, encrypted, algorithm, key_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finalize_recording(&self, id: i64, file_size_bytes: i64, duration_sec: f64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "UPDATE recordings SET file_size_bytes = ?1, duration_sec = ?2, end_time = datetime('now') WHERE id = ?3",
            rusqlite::params![file_size_bytes, duration_sec, id],
        )?;
        Ok(changes > 0)
    }

    pub fn list_recordings(&self, rec_type: Option<&str>, limit: usize) -> Result<Vec<Recording>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match rec_type {
            Some(rt) => (
                format!("SELECT {} FROM recordings WHERE rec_type = ?1 ORDER BY start_time DESC LIMIT ?2", Self::RECORDING_COLS),
                vec![Box::new(rt.to_string()), Box::new(limit as i64)],
            ),
            None => (
                format!("SELECT {} FROM recordings ORDER BY start_time DESC LIMIT ?1", Self::RECORDING_COLS),
                vec![Box::new(limit as i64)],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), Self::row_to_recording)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// List IQ captures, optionally filtered by status ("recording" or "complete").
    pub fn list_iq_captures(&self, status: Option<&str>, limit: usize) -> Result<Vec<Recording>, rusqlite::Error> {
        let conn = self.read_conn();
        let status_clause = match status {
            Some("recording") => " AND end_time IS NULL",
            Some("complete") => " AND end_time IS NOT NULL",
            _ => "",
        };
        let sql = format!(
            "SELECT {} FROM recordings WHERE rec_type = 'iq'{} ORDER BY start_time DESC LIMIT ?1",
            Self::RECORDING_COLS, status_clause
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], Self::row_to_recording)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_recording(&self, id: i64) -> Result<Option<Recording>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!("SELECT {} FROM recordings WHERE id = ?1", Self::RECORDING_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(rusqlite::params![id], Self::row_to_recording)?;
        match rows.next() {
            Some(ch) => Ok(Some(ch?)),
            None => Ok(None),
        }
    }

    pub fn list_site_recordings(&self, site_id: i64, limit: usize) -> Result<Vec<Recording>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = format!(
            "SELECT {} FROM recordings WHERE site_id = ?1 ORDER BY start_time DESC LIMIT ?2",
            Self::RECORDING_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], Self::row_to_recording)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_recording(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "DELETE FROM recordings WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(changes > 0)
    }

    pub fn recording_stats(&self) -> Result<RecordingStats, rusqlite::Error> {
        let conn = self.read_conn();
        let total_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recordings", [], |r| r.get(0),
        )?;
        let total_size_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(file_size_bytes), 0) FROM recordings", [], |r| r.get(0),
        )?;
        let total_duration_sec: f64 = conn.query_row(
            "SELECT COALESCE(SUM(duration_sec), 0.0) FROM recordings", [], |r| r.get(0),
        )?;
        let audio_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recordings WHERE rec_type = 'audio'", [], |r| r.get(0),
        )?;
        let iq_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recordings WHERE rec_type = 'iq'", [], |r| r.get(0),
        )?;
        Ok(RecordingStats {
            total_count,
            total_size_bytes,
            total_duration_sec,
            audio_count,
            iq_count,
        })
    }

    /// List auto-recorded clips (trigger_type LIKE 'auto_%') with optional filters.
    pub fn list_clips(
        &self,
        tgid: Option<i32>,
        site_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Recording>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut conditions = vec!["trigger_type LIKE 'auto_%'".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 1;

        if let Some(tg) = tgid {
            conditions.push(format!("tgid = ?{}", param_idx));
            params_vec.push(Box::new(tg));
            param_idx += 1;
        }
        if let Some(sid) = site_id {
            conditions.push(format!("site_id = ?{}", param_idx));
            params_vec.push(Box::new(sid));
            param_idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT {} FROM recordings WHERE {} ORDER BY start_time DESC LIMIT ?{}",
            Self::RECORDING_COLS, where_clause, param_idx
        );
        params_vec.push(Box::new(limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter()
            .map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), Self::row_to_recording)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Clip browsing queries ---

    /// Auto-clip summary stats with optional site scoping.
    pub fn clip_stats(&self, site_id: Option<i64>) -> Result<ClipStats, rusqlite::Error> {
        let conn = self.read_conn();
        let site_filter = if site_id.is_some() { " AND site_id = ?1" } else { "" };
        let params: Vec<Box<dyn rusqlite::types::ToSql>> = match site_id {
            Some(sid) => vec![Box::new(sid)],
            None => vec![],
        };
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let total_clips: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM recordings WHERE trigger_type LIKE 'auto_%'{}", site_filter),
            params_refs.as_slice(), |r| r.get(0),
        )?;
        let p25_clips: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM recordings WHERE trigger_type = 'auto_p25'{}", site_filter),
            params_refs.as_slice(), |r| r.get(0),
        )?;
        let analog_clips: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM recordings WHERE trigger_type = 'auto_squelch'{}", site_filter),
            params_refs.as_slice(), |r| r.get(0),
        )?;
        let today_clips: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM recordings WHERE trigger_type LIKE 'auto_%' AND date(start_time) = date('now'){}", site_filter),
            params_refs.as_slice(), |r| r.get(0),
        )?;
        let today_size_bytes: i64 = conn.query_row(
            &format!("SELECT COALESCE(SUM(file_size_bytes), 0) FROM recordings WHERE trigger_type LIKE 'auto_%' AND date(start_time) = date('now'){}", site_filter),
            params_refs.as_slice(), |r| r.get(0),
        )?;
        let today_duration_sec: f64 = conn.query_row(
            &format!("SELECT COALESCE(SUM(duration_sec), 0.0) FROM recordings WHERE trigger_type LIKE 'auto_%' AND date(start_time) = date('now'){}", site_filter),
            params_refs.as_slice(), |r| r.get(0),
        )?;
        Ok(ClipStats {
            total_clips, p25_clips, analog_clips,
            today_clips, today_size_bytes, today_duration_sec,
        })
    }

    /// P25 talkgroup tree data — grouped clip counts by TG, joined to network_talkgroups for names.
    pub fn clip_tg_groups(&self, site_id: Option<i64>, department: Option<&str>) -> Result<Vec<TgGroup>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = "SELECT r.tgid, nt.name, nt.department, \
                   COUNT(*) as clip_count, \
                   COALESCE(SUM(r.duration_sec), 0.0) as total_duration, \
                   COALESCE(SUM(r.file_size_bytes), 0) as total_size, \
                   COUNT(DISTINCT r.source_unit) as unique_uids, \
                   MAX(r.start_time) as last_clip_time \
                   FROM recordings r \
                   LEFT JOIN network_talkgroups nt ON nt.tgid = r.tgid \
                   WHERE r.trigger_type = 'auto_p25' \
                   AND (?1 IS NULL OR r.site_id = ?1) \
                   AND (?2 IS NULL OR nt.department = ?2) \
                   GROUP BY r.tgid ORDER BY last_clip_time DESC";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params![site_id, department], |row| {
            Ok(TgGroup {
                tgid: row.get(0)?,
                tg_name: row.get(1)?,
                department: row.get(2)?,
                clip_count: row.get(3)?,
                total_duration: row.get(4)?,
                total_size: row.get(5)?,
                unique_uids: row.get(6)?,
                last_clip_time: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Analog frequency tree data — grouped clip counts by frequency.
    pub fn clip_freq_groups(&self, site_id: Option<i64>) -> Result<Vec<FreqGroup>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = "SELECT ROUND(r.freq_mhz, 3) as freq, r.modulation, \
                   COUNT(*) as clip_count, \
                   COALESCE(SUM(r.duration_sec), 0.0) as total_duration, \
                   COALESCE(SUM(r.file_size_bytes), 0) as total_size, \
                   MAX(r.start_time) as last_clip_time \
                   FROM recordings r \
                   WHERE r.trigger_type = 'auto_squelch' \
                   AND (?1 IS NULL OR r.site_id = ?1) \
                   GROUP BY ROUND(r.freq_mhz, 3) ORDER BY last_clip_time DESC";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params![site_id], |row| {
            Ok(FreqGroup {
                freq_mhz: row.get(0)?,
                modulation: row.get(1)?,
                clip_count: row.get(2)?,
                total_duration: row.get(3)?,
                total_size: row.get(4)?,
                last_clip_time: row.get(5)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Source unit (UID) filter for a specific talkgroup.
    pub fn clip_source_units(&self, tgid: i32, site_id: Option<i64>) -> Result<Vec<SourceUnitInfo>, rusqlite::Error> {
        let conn = self.read_conn();
        let sql = "SELECT source_unit, COUNT(*) as clip_count, \
                   COALESCE(SUM(duration_sec), 0.0) as total_duration, \
                   MAX(start_time) as last_seen \
                   FROM recordings \
                   WHERE trigger_type = 'auto_p25' AND tgid = ?1 \
                   AND (?2 IS NULL OR site_id = ?2) AND source_unit IS NOT NULL \
                   GROUP BY source_unit ORDER BY clip_count DESC";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params![tgid, site_id], |row| {
            Ok(SourceUnitInfo {
                source_unit: row.get(0)?,
                clip_count: row.get(1)?,
                total_duration: row.get(2)?,
                last_seen: row.get(3)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Enhanced clip list with comprehensive filters.
    #[allow(clippy::too_many_arguments)]
    pub fn list_clips_enhanced(
        &self,
        tgid: Option<i32>,
        source_unit: Option<i32>,
        site_id: Option<i64>,
        freq_mhz: Option<f64>,
        trigger_type: Option<&str>,
        encrypted: Option<bool>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Recording>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut conditions = vec!["trigger_type LIKE 'auto_%'".to_string()];
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(tg) = tgid {
            conditions.push(format!("tgid = ?{}", idx));
            params_vec.push(Box::new(tg));
            idx += 1;
        }
        if let Some(uid) = source_unit {
            conditions.push(format!("source_unit = ?{}", idx));
            params_vec.push(Box::new(uid));
            idx += 1;
        }
        if let Some(sid) = site_id {
            conditions.push(format!("site_id = ?{}", idx));
            params_vec.push(Box::new(sid));
            idx += 1;
        }
        if let Some(f) = freq_mhz {
            conditions.push(format!("ROUND(freq_mhz, 3) = ROUND(?{}, 3)", idx));
            params_vec.push(Box::new(f));
            idx += 1;
        }
        if let Some(tt) = trigger_type {
            conditions.push(format!("trigger_type = ?{}", idx));
            params_vec.push(Box::new(tt.to_string()));
            idx += 1;
        }
        if let Some(enc) = encrypted {
            conditions.push(format!("encrypted = ?{}", idx));
            params_vec.push(Box::new(enc));
            idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT {} FROM recordings WHERE {} ORDER BY start_time DESC LIMIT ?{} OFFSET ?{}",
            Self::RECORDING_COLS, where_clause, idx, idx + 1
        );
        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), Self::row_to_recording)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Distinct departments from network_talkgroups.
    pub fn list_tg_departments(&self) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT department FROM network_talkgroups \
             WHERE department IS NOT NULL AND department != '' \
             ORDER BY department"
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Query Engine ---

    /// Get schema information for all tables.
    pub fn query_schema(&self) -> Result<Vec<TableSchema>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut tables = Vec::new();

        // Get all non-internal tables
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        )?;
        let table_names: Vec<String> = stmt.query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for table_name in &table_names {
            let row_count: i64 = conn.query_row(
                &format!("SELECT COUNT(*) FROM \"{}\"", table_name), [], |r| r.get(0),
            ).unwrap_or(0);

            let mut col_stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table_name))?;
            let columns: Vec<ColumnInfo> = col_stmt.query_map([], |row| {
                Ok(ColumnInfo {
                    name: row.get(1)?,
                    col_type: row.get(2)?,
                    notnull: row.get::<_, bool>(3)?,
                    pk: row.get::<_, bool>(5)?,
                })
            })?.filter_map(|r| r.ok()).collect();

            tables.push(TableSchema {
                table: table_name.clone(),
                columns,
                row_count,
            });
        }

        Ok(tables)
    }

    /// Execute a read-only SQL query. Returns columns + rows as JSON values.
    pub fn execute_query(&self, sql: &str) -> Result<QueryResult, String> {
        // Security: reject write operations
        let trimmed = sql.trim().to_uppercase();
        if trimmed.starts_with("INSERT") || trimmed.starts_with("UPDATE")
            || trimmed.starts_with("DELETE") || trimmed.starts_with("DROP")
            || trimmed.starts_with("ALTER") || trimmed.starts_with("CREATE")
            || trimmed.starts_with("ATTACH") || trimmed.starts_with("DETACH")
            || trimmed.starts_with("REPLACE") || trimmed.starts_with("PRAGMA")
        {
            return Err("Read-only queries only. Write operations are not allowed.".into());
        }

        let conn = self.read_conn();
        let start = std::time::Instant::now();

        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let col_count = stmt.column_count();
        let columns: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let mut rows = Vec::new();
        let mut raw_rows = stmt.query([]).map_err(|e| e.to_string())?;

        const MAX_ROWS: usize = 10_000;
        while let Some(row) = raw_rows.next().map_err(|e| e.to_string())? {
            if rows.len() >= MAX_ROWS { break; }
            let mut values = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let val = match row.get_ref(i) {
                    Ok(rusqlite::types::ValueRef::Null) => serde_json::Value::Null,
                    Ok(rusqlite::types::ValueRef::Integer(n)) => serde_json::json!(n),
                    Ok(rusqlite::types::ValueRef::Real(f)) => serde_json::json!(f),
                    Ok(rusqlite::types::ValueRef::Text(s)) => {
                        serde_json::Value::String(String::from_utf8_lossy(s).into_owned())
                    }
                    Ok(rusqlite::types::ValueRef::Blob(b)) => {
                        serde_json::Value::String(format!("<blob {} bytes>", b.len()))
                    }
                    Err(_) => serde_json::Value::Null,
                };
                values.push(val);
            }
            rows.push(values);
        }

        let elapsed_ms = start.elapsed().as_millis() as u64;
        let row_count = rows.len();

        Ok(QueryResult {
            columns,
            rows,
            row_count,
            elapsed_ms,
        })
    }

    // --- Saved Queries ---

    pub fn save_query(&self, name: &str, sql_text: &str, chart_config: Option<&str>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO saved_queries (name, sql_text, chart_config) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, sql_text, chart_config],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_saved_queries(&self) -> Result<Vec<SavedQuery>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, sql_text, chart_config, created_at FROM saved_queries ORDER BY created_at DESC"
        )?;
        let results = stmt.query_map([], |row| {
            Ok(SavedQuery {
                id: row.get(0)?,
                name: row.get(1)?,
                sql_text: row.get(2)?,
                chart_config: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(results)
    }

    pub fn delete_saved_query(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute("DELETE FROM saved_queries WHERE id = ?1", rusqlite::params![id])?;
        Ok(changes > 0)
    }

    // ── Antenna CRUD ──────────────────────────────────────────────────

    pub fn list_antennas(&self) -> Result<Vec<Antenna>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, antenna_type, connector, freq_min_mhz, freq_max_mhz,
                    gain_dbi, notes, active, created_at
             FROM antennas ORDER BY name"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Antenna {
                id: r.get(0)?,
                name: r.get(1)?,
                antenna_type: r.get(2)?,
                connector: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "SMA".into()),
                freq_min_mhz: r.get(4)?,
                freq_max_mhz: r.get(5)?,
                gain_dbi: r.get(6)?,
                notes: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                active: r.get::<_, i32>(8)? != 0,
                created_at: r.get(9)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn get_antenna(&self, id: i64) -> Result<Option<Antenna>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, antenna_type, connector, freq_min_mhz, freq_max_mhz,
                    gain_dbi, notes, active, created_at
             FROM antennas WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], |r| {
            Ok(Antenna {
                id: r.get(0)?,
                name: r.get(1)?,
                antenna_type: r.get(2)?,
                connector: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "SMA".into()),
                freq_min_mhz: r.get(4)?,
                freq_max_mhz: r.get(5)?,
                gain_dbi: r.get(6)?,
                notes: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                active: r.get::<_, i32>(8)? != 0,
                created_at: r.get(9)?,
            })
        })?;
        match rows.next() {
            Some(Ok(a)) => Ok(Some(a)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn create_antenna(
        &self, name: &str, antenna_type: &str, connector: &str,
        freq_min_mhz: Option<f64>, freq_max_mhz: Option<f64>,
        gain_dbi: Option<f64>, notes: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO antennas (name, antenna_type, connector, freq_min_mhz, freq_max_mhz, gain_dbi, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![name, antenna_type, connector, freq_min_mhz, freq_max_mhz, gain_dbi, notes],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_antenna(
        &self, id: i64, name: &str, antenna_type: &str, connector: &str,
        freq_min_mhz: Option<f64>, freq_max_mhz: Option<f64>,
        gain_dbi: Option<f64>, notes: &str, active: bool,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute(
            "UPDATE antennas SET name=?2, antenna_type=?3, connector=?4,
             freq_min_mhz=?5, freq_max_mhz=?6, gain_dbi=?7, notes=?8, active=?9
             WHERE id=?1",
            rusqlite::params![id, name, antenna_type, connector, freq_min_mhz, freq_max_mhz, gain_dbi, notes, active as i32],
        )?;
        Ok(changes > 0)
    }

    pub fn delete_antenna(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let changes = conn.execute("DELETE FROM antennas WHERE id = ?1", rusqlite::params![id])?;
        Ok(changes > 0)
    }

    pub fn list_antenna_freq_ranges(&self, antenna_id: i64) -> Result<Vec<AntennaFreqRange>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, antenna_id, freq_min_mhz, freq_max_mhz, gain_dbi, label
             FROM antenna_freq_ranges WHERE antenna_id = ?1 ORDER BY freq_min_mhz"
        )?;
        let rows = stmt.query_map(rusqlite::params![antenna_id], |r| {
            Ok(AntennaFreqRange {
                id: r.get(0)?,
                antenna_id: r.get(1)?,
                freq_min_mhz: r.get(2)?,
                freq_max_mhz: r.get(3)?,
                gain_dbi: r.get(4)?,
                label: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn set_antenna_freq_ranges(
        &self, antenna_id: i64,
        ranges: &[(f64, f64, Option<f64>, String)],
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM antenna_freq_ranges WHERE antenna_id = ?1", rusqlite::params![antenna_id])?;
        for (min_f, max_f, gain, label) in ranges {
            conn.execute(
                "INSERT INTO antenna_freq_ranges (antenna_id, freq_min_mhz, freq_max_mhz, gain_dbi, label)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![antenna_id, min_f, max_f, gain, label],
            )?;
        }
        Ok(())
    }

    pub fn get_device_antenna(&self, serial: &str) -> Result<Option<DeviceAntennaAssignment>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, device_serial, antenna_id, assigned_at
             FROM device_antenna_assignments WHERE device_serial = ?1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![serial], |r| {
            Ok(DeviceAntennaAssignment {
                id: r.get(0)?,
                device_serial: r.get(1)?,
                antenna_id: r.get(2)?,
                assigned_at: r.get(3)?,
            })
        })?;
        match rows.next() {
            Some(Ok(a)) => Ok(Some(a)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn set_device_antenna(&self, serial: &str, antenna_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO device_antenna_assignments (device_serial, antenna_id)
             VALUES (?1, ?2)
             ON CONFLICT(device_serial) DO UPDATE SET antenna_id = ?2, assigned_at = datetime('now')",
            rusqlite::params![serial, antenna_id],
        )?;
        Ok(())
    }

    pub fn clear_device_antenna(&self, serial: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM device_antenna_assignments WHERE device_serial = ?1",
            rusqlite::params![serial],
        )?;
        Ok(())
    }

    /// Returns serial → (antenna_id, freq_min_mhz, freq_max_mhz) for all assigned devices.
    pub fn get_antenna_map(&self) -> Result<std::collections::HashMap<String, (i64, f64, f64)>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT daa.device_serial, daa.antenna_id, a.freq_min_mhz, a.freq_max_mhz
             FROM device_antenna_assignments daa
             JOIN antennas a ON a.id = daa.antenna_id
             WHERE a.active = 1 AND a.freq_min_mhz IS NOT NULL AND a.freq_max_mhz IS NOT NULL"
        )?;
        let map = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, i64>(1)?, row.get::<_, f64>(2)?, row.get::<_, f64>(3)?),
            ))
        })?.filter_map(|r| r.ok()).collect();
        Ok(map)
    }

    // ── Observation Target CRUD ──────────────────────────────────────────

    pub fn create_observation_target(
        &self,
        target_type: &str,
        target_key: &str,
        label: Option<&str>,
        site_id: Option<i64>,
        priority: i32,
        notes: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO observation_targets (target_type, target_key, target_label, site_id, priority, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![target_type, target_key, label, site_id, priority, notes],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_observation_targets(
        &self,
        site_id: Option<i64>,
        target_type: Option<&str>,
    ) -> Result<Vec<ObservationTarget>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut sql = "SELECT id, target_type, target_key, target_label, site_id, priority, notes, created_at, COALESCE(coverage_target_hours, 4.0) FROM observation_targets WHERE 1=1".to_string();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;
        if let Some(sid) = site_id {
            sql.push_str(&format!(" AND site_id = ?{}", idx));
            params.push(Box::new(sid));
            idx += 1;
        }
        if let Some(tt) = target_type {
            sql.push_str(&format!(" AND target_type = ?{}", idx));
            params.push(Box::new(tt.to_string()));
        }
        sql.push_str(" ORDER BY priority DESC, created_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(ObservationTarget {
                id: row.get(0)?,
                target_type: row.get(1)?,
                target_key: row.get(2)?,
                target_label: row.get(3)?,
                site_id: row.get(4)?,
                priority: row.get(5)?,
                notes: row.get(6)?,
                created_at: row.get(7)?,
                coverage_target_hours: row.get(8)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn delete_observation_target(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        // Delete associated alerts and observations first
        conn.execute("DELETE FROM observation_alerts WHERE target_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM observations WHERE target_id = ?1", rusqlite::params![id])?;
        let n = conn.execute("DELETE FROM observation_targets WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }

    pub fn update_observation_target(
        &self,
        id: i64,
        label: Option<&str>,
        priority: Option<i32>,
        notes: Option<&str>,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE observation_targets SET
                target_label = COALESCE(?2, target_label),
                priority = COALESCE(?3, priority),
                notes = COALESCE(?4, notes)
             WHERE id = ?1",
            rusqlite::params![id, label, priority, notes],
        )?;
        Ok(n > 0)
    }

    /// List collection targets with computed coverage (observation duration in last 24h / target hours).
    pub fn list_collection_targets(&self, site_id: Option<i64>) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let targets = self.list_observation_targets(site_id, None)?;
        let conn = self.read_conn();
        let mut results = Vec::new();
        for t in &targets {
            let coverage_hours: f64 = conn.query_row(
                "SELECT COALESCE(SUM(duration_sec), 0.0) / 3600.0
                 FROM observations
                 WHERE target_id = ?1 AND start_time >= datetime('now', '-24 hours')",
                rusqlite::params![t.id],
                |row| row.get(0),
            ).unwrap_or(0.0);
            let obs_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM observations WHERE target_id = ?1",
                rusqlite::params![t.id],
                |row| row.get(0),
            ).unwrap_or(0);
            let coverage_pct = if t.coverage_target_hours > 0.0 {
                (coverage_hours / t.coverage_target_hours).min(1.0)
            } else {
                0.0
            };
            results.push(serde_json::json!({
                "id": t.id,
                "target_type": t.target_type,
                "target_key": t.target_key,
                "target_label": t.target_label,
                "site_id": t.site_id,
                "priority": t.priority,
                "notes": t.notes,
                "created_at": t.created_at,
                "coverage_target_hours": t.coverage_target_hours,
                "coverage_pct": coverage_pct,
                "observations": obs_count,
            }));
        }
        Ok(results)
    }

    // ── Collection Requirements ────────────────────────────────────────

    pub fn list_collection_requirements(&self, site_id: Option<i64>) -> Result<Vec<CollectionRequirement>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match site_id {
            Some(sid) => (
                "SELECT id, label, check_type, check_config_json, site_id, met, last_checked, created_at
                 FROM collection_requirements WHERE site_id = ?1 OR site_id IS NULL ORDER BY created_at".into(),
                vec![Box::new(sid)],
            ),
            None => (
                "SELECT id, label, check_type, check_config_json, site_id, met, last_checked, created_at
                 FROM collection_requirements ORDER BY created_at".into(),
                vec![],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(CollectionRequirement {
                id: row.get(0)?,
                label: row.get(1)?,
                check_type: row.get(2)?,
                check_config_json: row.get(3)?,
                site_id: row.get(4)?,
                met: row.get::<_, i32>(5).unwrap_or(0) != 0,
                last_checked: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn create_collection_requirement(&self, label: &str, check_type: &str, site_id: Option<i64>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO collection_requirements (label, check_type, site_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![label, check_type, site_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn toggle_collection_requirement(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE collection_requirements SET met = 1 - met, last_checked = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_collection_requirement(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "DELETE FROM collection_requirements WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(n > 0)
    }

    // ── Observation CRUD ─────────────────────────────────────────────────

    pub fn create_observation(
        &self,
        target_id: i64,
        site_id: Option<i64>,
        site_session_id: Option<i64>,
        operation_id: Option<i64>,
        start_time: &str,
        lat: Option<f64>,
        lon: Option<f64>,
        device_key: Option<&str>,
        freq_mhz: Option<f64>,
        tgid: Option<i32>,
        uid: Option<i32>,
        encrypted: bool,
        signal_dbfs: Option<f64>,
        observation_type: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO observations (target_id, site_id, site_session_id, operation_id,
                start_time, receiver_lat, receiver_lon, device_key, freq_mhz,
                tgid, uid, encrypted, signal_dbfs, observation_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                target_id, site_id, site_session_id, operation_id,
                start_time, lat, lon, device_key, freq_mhz,
                tgid, uid, encrypted as i32, signal_dbfs, observation_type
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn complete_observation(&self, id: i64, end_time: &str, duration_sec: f64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE observations SET end_time = ?2, duration_sec = ?3 WHERE id = ?1",
            rusqlite::params![id, end_time, duration_sec],
        )?;
        Ok(n > 0)
    }

    pub fn list_observations(
        &self,
        target_id: Option<i64>,
        site_id: Option<i64>,
        limit: usize,
        since: Option<&str>,
    ) -> Result<Vec<Observation>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut sql = "SELECT o.id, o.target_id, o.site_id, o.site_session_id, o.operation_id,
                o.start_time, o.end_time, o.duration_sec, o.receiver_lat, o.receiver_lon,
                o.device_key, o.freq_mhz, o.tgid, o.uid, o.encrypted, o.signal_dbfs,
                o.observation_type, o.metadata_json,
                ot.target_type, ot.target_key, ot.target_label
             FROM observations o
             LEFT JOIN observation_targets ot ON ot.id = o.target_id
             WHERE 1=1".to_string();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;
        if let Some(tid) = target_id {
            sql.push_str(&format!(" AND o.target_id = ?{}", idx));
            params.push(Box::new(tid));
            idx += 1;
        }
        if let Some(sid) = site_id {
            sql.push_str(&format!(" AND o.site_id = ?{}", idx));
            params.push(Box::new(sid));
            idx += 1;
        }
        if let Some(s) = since {
            sql.push_str(&format!(" AND o.start_time >= ?{}", idx));
            params.push(Box::new(s.to_string()));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY o.start_time DESC LIMIT ?{}", idx));
        params.push(Box::new(limit as i64));
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(Observation {
                id: row.get(0)?,
                target_id: row.get(1)?,
                site_id: row.get(2)?,
                site_session_id: row.get(3)?,
                operation_id: row.get(4)?,
                start_time: row.get(5)?,
                end_time: row.get(6)?,
                duration_sec: row.get(7)?,
                receiver_lat: row.get(8)?,
                receiver_lon: row.get(9)?,
                device_key: row.get(10)?,
                freq_mhz: row.get(11)?,
                tgid: row.get(12)?,
                uid: row.get(13)?,
                encrypted: row.get::<_, i32>(14).unwrap_or(0) != 0,
                signal_dbfs: row.get(15)?,
                observation_type: row.get(16)?,
                metadata_json: row.get(17)?,
                target_type: row.get(18)?,
                target_key: row.get(19)?,
                target_label: row.get(20)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    // ── Observation Alert CRUD ───────────────────────────────────────────

    pub fn create_observation_alert(
        &self,
        target_id: i64,
        alert_type: &str,
        threshold_json: Option<&str>,
        cooldown_sec: i32,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO observation_alerts (target_id, alert_type, threshold_json, cooldown_sec)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![target_id, alert_type, threshold_json, cooldown_sec],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_observation_alerts(
        &self,
        target_id: Option<i64>,
    ) -> Result<Vec<ObservationAlert>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(tid) = target_id {
            (
                "SELECT id, target_id, alert_type, threshold_json, cooldown_sec, enabled, last_fired, fire_count, created_at
                 FROM observation_alerts WHERE target_id = ?1 ORDER BY created_at DESC".into(),
                vec![Box::new(tid)],
            )
        } else {
            (
                "SELECT id, target_id, alert_type, threshold_json, cooldown_sec, enabled, last_fired, fire_count, created_at
                 FROM observation_alerts ORDER BY created_at DESC".into(),
                vec![],
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(ObservationAlert {
                id: row.get(0)?,
                target_id: row.get(1)?,
                alert_type: row.get(2)?,
                threshold_json: row.get(3)?,
                cooldown_sec: row.get(4)?,
                enabled: row.get::<_, i32>(5).unwrap_or(1) != 0,
                last_fired: row.get(6)?,
                fire_count: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn toggle_observation_alert(&self, id: i64, enabled: bool) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE observation_alerts SET enabled = ?2 WHERE id = ?1",
            rusqlite::params![id, enabled as i32],
        )?;
        Ok(n > 0)
    }

    pub fn fire_observation_alert(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE observation_alerts SET last_fired = datetime('now'), fire_count = fire_count + 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(n > 0)
    }

    // ── Auto-IQ Rule CRUD ────────────────────────────────────────────────

    pub fn create_auto_iq_rule(
        &self,
        trigger_type: &str,
        trigger_config_json: Option<&str>,
        site_id: Option<i64>,
        max_duration_sec: i32,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO auto_iq_rules (trigger_type, trigger_config_json, site_id, max_duration_sec)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![trigger_type, trigger_config_json, site_id, max_duration_sec],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_auto_iq_rules(&self, site_id: Option<i64>) -> Result<Vec<AutoIqRule>, rusqlite::Error> {
        let conn = self.read_conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(sid) = site_id {
            (
                "SELECT id, trigger_type, trigger_config_json, enabled, max_duration_sec, site_id, created_at
                 FROM auto_iq_rules WHERE site_id = ?1 OR site_id IS NULL ORDER BY created_at DESC".into(),
                vec![Box::new(sid)],
            )
        } else {
            (
                "SELECT id, trigger_type, trigger_config_json, enabled, max_duration_sec, site_id, created_at
                 FROM auto_iq_rules ORDER BY created_at DESC".into(),
                vec![],
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(AutoIqRule {
                id: row.get(0)?,
                trigger_type: row.get(1)?,
                trigger_config_json: row.get(2)?,
                enabled: row.get::<_, i32>(3).unwrap_or(1) != 0,
                max_duration_sec: row.get(4)?,
                site_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn toggle_auto_iq_rule(&self, id: i64, enabled: bool) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE auto_iq_rules SET enabled = ?2 WHERE id = ?1",
            rusqlite::params![id, enabled as i32],
        )?;
        Ok(n > 0)
    }

    pub fn delete_auto_iq_rule(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let n = conn.execute("DELETE FROM auto_iq_rules WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }

    /// Check if any signal_hits exist near `freq_mhz` (±15 kHz) since `since` timestamp.
    pub fn check_recent_signal_activity(&self, freq_mhz: f64, since: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM signal_hits WHERE abs(freq_mhz - ?1) < 0.015 AND timestamp >= ?2",
            rusqlite::params![freq_mhz, since],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Return voice_freq of the most recent encrypted grant since `since`, if any.
    pub fn check_recent_encrypted_grant(&self, since: &str) -> Result<Option<f64>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT g.voice_freq FROM channel_grants g
             JOIN network_talkgroups t ON g.tgid = t.tgid AND g.system = t.system
             WHERE t.encrypted = '1' AND g.timestamp >= ?1
             ORDER BY g.id DESC LIMIT 1"
        )?;
        let freq: Option<f64> = stmt.query_row(rusqlite::params![since], |row| row.get(0)).ok();
        Ok(freq)
    }

    /// Check if any traffic_sessions exist for `channel_id` since `since` timestamp.
    pub fn check_recent_channel_activity(&self, channel_id: i64, since: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM traffic_sessions WHERE channel_id = ?1 AND start_time >= ?2",
            rusqlite::params![channel_id, since],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // ── RF Fingerprint Methods ─────────────────────────────────────────

    /// Upsert a radio fingerprint into the radio_fingerprints table.
    /// Generates fingerprint_id from 3D bucketing: CFO + IQ amplitude + IQ phase.
    pub fn upsert_radio_fingerprint(
        &self,
        cfo_hz: f64,
        iq_amplitude_imbal: f64,
        iq_phase_imbal: f64,
        avg_power_db: f64,
        power_variance: f64,
        sample_count: i32,
        freq_mhz: f64,
        cfo_bucket_hz: f64,
        iq_resolution: f64,
    ) -> Result<(i64, i64), rusqlite::Error> {
        // Generate a 3D fingerprint ID from quantized CFO + IQ signature
        let cfo_bucket = if cfo_bucket_hz > 0.0 { (cfo_hz / cfo_bucket_hz).round() as i64 } else { 0 };
        let iq_bucket = if iq_resolution > 0.0 { (iq_amplitude_imbal / iq_resolution).round() as i64 } else { 0 };
        let phase_bucket = if iq_resolution > 0.0 { (iq_phase_imbal / iq_resolution).round() as i64 } else { 0 };
        let fp_id = format!("FP-{:+06}:{:04}:{:04}", cfo_bucket, iq_bucket, phase_bucket);

        let conn = self.conn();
        let existing: Option<(i64, i64)> = conn.query_row(
            "SELECT id, capture_count FROM radio_fingerprints WHERE fingerprint_id = ?1",
            rusqlite::params![&fp_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();

        if let Some((id, capture_count)) = existing {
            conn.execute(
                "UPDATE radio_fingerprints SET
                    freq_offset_hz = ?2, iq_imbalance = ?3, phase_noise = ?4,
                    confidence = ?5, capture_count = capture_count + 1,
                    freq_mhz = ?6, sample_count = ?7,
                    last_seen = datetime('now')
                 WHERE id = ?1",
                rusqlite::params![id, cfo_hz, iq_amplitude_imbal, iq_phase_imbal, avg_power_db, freq_mhz, sample_count],
            )?;
            Ok((id, capture_count + 1))
        } else {
            conn.execute(
                "INSERT INTO radio_fingerprints
                    (fingerprint_id, freq_offset_hz, iq_imbalance, phase_noise, evm, confidence,
                     freq_mhz, sample_count, capture_count, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, datetime('now'), datetime('now'))",
                rusqlite::params![
                    &fp_id, cfo_hz, iq_amplitude_imbal, iq_phase_imbal,
                    power_variance, avg_power_db, freq_mhz, sample_count
                ],
            )?;
            Ok((conn.last_insert_rowid(), 1))
        }
    }

    /// List radio fingerprints ordered by last_seen descending.
    pub fn list_radio_fingerprints(&self, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, fingerprint_id, uid, freq_offset_hz, iq_imbalance, phase_noise,
                    evm, confidence, capture_count, first_seen, last_seen,
                    freq_mhz, sample_count
             FROM radio_fingerprints ORDER BY last_seen DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "fingerprint_id": row.get::<_, String>(1)?,
                "uid": row.get::<_, Option<i32>>(2)?,
                "cfo_hz": row.get::<_, Option<f64>>(3)?,
                "iq_imbalance": row.get::<_, Option<f64>>(4)?,
                "phase_noise": row.get::<_, Option<f64>>(5)?,
                "evm": row.get::<_, Option<f64>>(6)?,
                "confidence": row.get::<_, Option<f64>>(7)?,
                "capture_count": row.get::<_, i32>(8)?,
                "first_seen": row.get::<_, String>(9)?,
                "last_seen": row.get::<_, String>(10)?,
                "freq_mhz": row.get::<_, Option<f64>>(11)?,
                "sample_count": row.get::<_, Option<i32>>(12)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Get a single fingerprint by id.
    pub fn get_fingerprint(&self, id: i64) -> Result<Option<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT id, fingerprint_id, uid, freq_offset_hz, iq_imbalance, phase_noise,
                    evm, confidence, capture_count, first_seen, last_seen,
                    freq_mhz, sample_count
             FROM radio_fingerprints WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "fingerprint_id": row.get::<_, String>(1)?,
                    "uid": row.get::<_, Option<i32>>(2)?,
                    "cfo_hz": row.get::<_, Option<f64>>(3)?,
                    "iq_imbalance": row.get::<_, Option<f64>>(4)?,
                    "phase_noise": row.get::<_, Option<f64>>(5)?,
                    "evm": row.get::<_, Option<f64>>(6)?,
                    "confidence": row.get::<_, Option<f64>>(7)?,
                    "capture_count": row.get::<_, i32>(8)?,
                    "first_seen": row.get::<_, String>(9)?,
                    "last_seen": row.get::<_, String>(10)?,
                    "freq_mhz": row.get::<_, Option<f64>>(11)?,
                    "sample_count": row.get::<_, Option<i32>>(12)?,
                }))
            }
        ).ok();
        Ok(result)
    }

    /// Match fingerprints by CFO and IQ imbalance within tolerance.
    pub fn match_fingerprint(&self, cfo_hz: f64, iq_imbal: f64, tolerance: f64) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, fingerprint_id, uid, freq_offset_hz, iq_imbalance, capture_count, last_seen
             FROM radio_fingerprints
             WHERE abs(freq_offset_hz - ?1) < ?3 AND abs(iq_imbalance - ?2) < ?3
             ORDER BY capture_count DESC LIMIT 10"
        )?;
        let rows = stmt.query_map(rusqlite::params![cfo_hz, iq_imbal, tolerance], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "fingerprint_id": row.get::<_, String>(1)?,
                "uid": row.get::<_, Option<i32>>(2)?,
                "cfo_hz": row.get::<_, Option<f64>>(3)?,
                "iq_imbalance": row.get::<_, Option<f64>>(4)?,
                "capture_count": row.get::<_, i32>(5)?,
                "last_seen": row.get::<_, String>(6)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// List channels as emitters with optional fingerprint data.
    pub fn list_emitters(&self, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.freq_mhz, c.label, c.cls, c.band, c.total_hits, c.first_seen, c.last_seen,
                    rf.fingerprint_id, rf.freq_offset_hz, rf.iq_imbalance, rf.confidence, rf.capture_count,
                    rf.uid,
                    (SELECT COUNT(DISTINCT m.fingerprint_id) FROM uid_fingerprint_map m WHERE m.uid = rf.uid) AS uid_fp_count
             FROM channels c
             LEFT JOIN radio_fingerprints rf ON rf.id = (
                 SELECT rf2.id FROM radio_fingerprints rf2
                 WHERE rf2.freq_mhz IS NOT NULL AND abs(c.freq_mhz - rf2.freq_mhz) < 0.015
                 ORDER BY rf2.capture_count DESC LIMIT 1
             )
             ORDER BY c.total_hits DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "freq": row.get::<_, Option<f64>>(1)?,
                "name": row.get::<_, String>(2)?,
                "cls": row.get::<_, String>(3)?,
                "band": row.get::<_, String>(4)?,
                "hits": row.get::<_, i64>(5)?,
                "first_seen": row.get::<_, Option<String>>(6)?,
                "last_seen": row.get::<_, Option<String>>(7)?,
                "fp_id": row.get::<_, Option<String>>(8)?,
                "cfo": row.get::<_, Option<f64>>(9)?,
                "iq_imbalance": row.get::<_, Option<f64>>(10)?,
                "confidence": row.get::<_, Option<f64>>(11)?,
                "fp_count": row.get::<_, Option<i32>>(12)?,
                "uid": row.get::<_, Option<i32>>(13)?,
                "uid_fp_count": row.get::<_, Option<i32>>(14)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    // ── UID-Fingerprint Correlation Methods ─────────────────────────────

    /// Find the most recent channel grant on a voice frequency within a time window.
    pub fn find_recent_grant_by_freq(&self, freq_mhz: f64, window_sec: i64) -> Result<Option<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id, system, tgid, uid, voice_freq, grant_type, timestamp
             FROM channel_grants
             WHERE voice_freq IS NOT NULL AND abs(voice_freq - ?1) < 0.002
               AND timestamp >= datetime('now', '-' || ?2 || ' seconds')
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![freq_mhz, window_sec],
            |row| Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "system": row.get::<_, String>(1)?,
                "tgid": row.get::<_, i32>(2)?,
                "uid": row.get::<_, Option<i32>>(3)?,
                "voice_freq": row.get::<_, Option<f64>>(4)?,
                "grant_type": row.get::<_, Option<String>>(5)?,
                "timestamp": row.get::<_, String>(6)?,
            }))
        ).optional()
    }

    /// Link a UID to a fingerprint_id. Upserts uid_fingerprint_map and sets radio_fingerprints.uid.
    pub fn link_uid_to_fingerprint(&self, uid: i32, fingerprint_id: &str, tgid: Option<i32>, system: Option<&str>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        let existing: Option<i64> = conn.query_row(
            "SELECT id FROM uid_fingerprint_map WHERE uid = ?1 AND fingerprint_id = ?2",
            rusqlite::params![uid, fingerprint_id],
            |row| row.get(0),
        ).ok();

        let id = if let Some(id) = existing {
            conn.execute(
                "UPDATE uid_fingerprint_map SET observation_count = observation_count + 1, last_seen = datetime('now') WHERE id = ?1",
                rusqlite::params![id],
            )?;
            id
        } else {
            conn.execute(
                "INSERT INTO uid_fingerprint_map (uid, fingerprint_id, tgid, system, observation_count, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, 1, datetime('now'), datetime('now'))",
                rusqlite::params![uid, fingerprint_id, tgid, system],
            )?;
            conn.last_insert_rowid()
        };

        // Set UID on the fingerprint record itself
        conn.execute(
            "UPDATE radio_fingerprints SET uid = ?2 WHERE fingerprint_id = ?1 AND uid IS NULL",
            rusqlite::params![fingerprint_id, uid],
        )?;

        Ok(id)
    }

    /// Get all UIDs associated with a fingerprint_id.
    pub fn get_uids_for_fingerprint(&self, fingerprint_id: &str) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT uid, tgid, system, observation_count, first_seen, last_seen
             FROM uid_fingerprint_map WHERE fingerprint_id = ?1
             ORDER BY observation_count DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![fingerprint_id], |row| {
            Ok(serde_json::json!({
                "uid": row.get::<_, i32>(0)?,
                "tgid": row.get::<_, Option<i32>>(1)?,
                "system": row.get::<_, Option<String>>(2)?,
                "observation_count": row.get::<_, i32>(3)?,
                "first_seen": row.get::<_, String>(4)?,
                "last_seen": row.get::<_, String>(5)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Get all fingerprints associated with a UID.
    pub fn get_fingerprints_for_uid(&self, uid: i32) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT m.fingerprint_id, m.observation_count, m.first_seen, m.last_seen,
                    r.freq_offset_hz, r.iq_imbalance, r.freq_mhz, r.capture_count
             FROM uid_fingerprint_map m
             LEFT JOIN radio_fingerprints r ON r.fingerprint_id = m.fingerprint_id
             WHERE m.uid = ?1
             ORDER BY m.observation_count DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![uid], |row| {
            Ok(serde_json::json!({
                "fingerprint_id": row.get::<_, String>(0)?,
                "observation_count": row.get::<_, i32>(1)?,
                "first_seen": row.get::<_, String>(2)?,
                "last_seen": row.get::<_, String>(3)?,
                "cfo_hz": row.get::<_, Option<f64>>(4)?,
                "iq_imbalance": row.get::<_, Option<f64>>(5)?,
                "freq_mhz": row.get::<_, Option<f64>>(6)?,
                "capture_count": row.get::<_, Option<i32>>(7)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// List UID-fingerprint links with full fingerprint data.
    pub fn list_uid_fingerprint_links(&self, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT m.uid, m.fingerprint_id, m.tgid, m.system, m.observation_count,
                    m.first_seen, m.last_seen,
                    r.freq_offset_hz, r.iq_imbalance, r.freq_mhz, r.capture_count
             FROM uid_fingerprint_map m
             LEFT JOIN radio_fingerprints r ON r.fingerprint_id = m.fingerprint_id
             ORDER BY m.last_seen DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(serde_json::json!({
                "uid": row.get::<_, i32>(0)?,
                "fingerprint_id": row.get::<_, String>(1)?,
                "tgid": row.get::<_, Option<i32>>(2)?,
                "system": row.get::<_, Option<String>>(3)?,
                "observation_count": row.get::<_, i32>(4)?,
                "first_seen": row.get::<_, String>(5)?,
                "last_seen": row.get::<_, String>(6)?,
                "cfo_hz": row.get::<_, Option<f64>>(7)?,
                "iq_imbalance": row.get::<_, Option<f64>>(8)?,
                "freq_mhz": row.get::<_, Option<f64>>(9)?,
                "capture_count": row.get::<_, Option<i32>>(10)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Count total UID-fingerprint links.
    pub fn count_uid_fingerprint_links(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.read_conn();
        conn.query_row("SELECT COUNT(*) FROM uid_fingerprint_map", [], |row| row.get(0))
    }

    /// Count distinct fingerprint IDs.
    pub fn count_distinct_fingerprints(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.read_conn();
        conn.query_row("SELECT COUNT(DISTINCT fingerprint_id) FROM radio_fingerprints", [], |row| row.get(0))
    }

    /// Get a fingerprint by its fingerprint_id string.
    pub fn get_fingerprint_by_fp_id(&self, fp_id: &str) -> Result<Option<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id, fingerprint_id, uid, freq_offset_hz, iq_imbalance, phase_noise,
                    evm, confidence, capture_count, first_seen, last_seen, freq_mhz, sample_count
             FROM radio_fingerprints WHERE fingerprint_id = ?1",
            rusqlite::params![fp_id],
            |row| Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "fingerprint_id": row.get::<_, String>(1)?,
                "uid": row.get::<_, Option<i32>>(2)?,
                "cfo_hz": row.get::<_, Option<f64>>(3)?,
                "iq_imbalance": row.get::<_, Option<f64>>(4)?,
                "phase_noise": row.get::<_, Option<f64>>(5)?,
                "evm": row.get::<_, Option<f64>>(6)?,
                "confidence": row.get::<_, Option<f64>>(7)?,
                "capture_count": row.get::<_, i32>(8)?,
                "first_seen": row.get::<_, String>(9)?,
                "last_seen": row.get::<_, String>(10)?,
                "freq_mhz": row.get::<_, Option<f64>>(11)?,
                "sample_count": row.get::<_, Option<i32>>(12)?,
            }))
        ).optional()
    }

    /// Fingerprint pipeline summary stats.
    pub fn fingerprint_stats(&self) -> Result<serde_json::Value, rusqlite::Error> {
        let conn = self.read_conn();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM radio_fingerprints", [], |r| r.get(0))?;
        let unique: i64 = conn.query_row("SELECT COUNT(DISTINCT fingerprint_id) FROM radio_fingerprints", [], |r| r.get(0))?;
        let uid_links: i64 = conn.query_row("SELECT COUNT(*) FROM uid_fingerprint_map", [], |r| r.get(0))?;
        let linked_uids: i64 = conn.query_row("SELECT COUNT(DISTINCT uid) FROM uid_fingerprint_map", [], |r| r.get(0))?;
        Ok(serde_json::json!({
            "total_fingerprints": total,
            "unique_emitters": unique,
            "uid_links": uid_links,
            "linked_uids": linked_uids,
        }))
    }

    /// Get compound fingerprint detail: fingerprint + UIDs + recent grants on that freq.
    pub fn get_fingerprint_detail(&self, fingerprint_id: &str) -> Result<serde_json::Value, rusqlite::Error> {
        let fp = self.get_fingerprint_by_fp_id(fingerprint_id)?;
        let uids = self.get_uids_for_fingerprint(fingerprint_id)?;

        // Get recent grants on this frequency if we have one
        let mut recent_grants = Vec::new();
        if let Some(ref fp_val) = fp {
            if let Some(freq) = fp_val.get("freq_mhz").and_then(|v| v.as_f64()) {
                let conn = self.read_conn();
                let mut stmt = conn.prepare(
                    "SELECT id, src_unit, talkgroup, voice_freq, timestamp, encrypted
                     FROM channel_grants
                     WHERE voice_freq IS NOT NULL AND abs(voice_freq - ?1) < 0.002
                     ORDER BY id DESC LIMIT 20"
                )?;
                let rows = stmt.query_map(rusqlite::params![freq], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, i64>(0)?,
                        "src_unit": row.get::<_, Option<i32>>(1)?,
                        "talkgroup": row.get::<_, Option<i32>>(2)?,
                        "voice_freq": row.get::<_, Option<f64>>(3)?,
                        "timestamp": row.get::<_, Option<String>>(4)?,
                        "encrypted": row.get::<_, Option<bool>>(5)?,
                    }))
                })?.filter_map(|r| r.ok()).collect::<Vec<_>>();
                recent_grants = rows;
            }
        }

        Ok(serde_json::json!({
            "fingerprint": fp,
            "uids": uids,
            "recent_grants": recent_grants,
        }))
    }

    /// Get 24h activity heatmap: signal_hits grouped by band + hour.
    /// Returns `{ "band_name": [24 hourly counts] }`.
    pub fn get_activity_heatmap(&self) -> Result<serde_json::Value, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT s.band, CAST(strftime('%H', sh.timestamp) AS INTEGER) AS hour, COUNT(*) AS cnt
             FROM signal_hits sh
             JOIN signals s ON s.id = sh.signal_id
             WHERE sh.timestamp >= datetime('now', '-24 hours')
             GROUP BY s.band, hour
             ORDER BY s.band, hour"
        )?;
        let mut map: std::collections::HashMap<String, Vec<i64>> = std::collections::HashMap::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let band: String = row.get(0)?;
            let hour: i64 = row.get(1)?;
            let cnt: i64 = row.get(2)?;
            let entry = map.entry(band).or_insert_with(|| vec![0i64; 24]);
            if hour >= 0 && hour < 24 {
                entry[hour as usize] = cnt;
            }
        }
        Ok(serde_json::json!(map))
    }

    /// Get channels first seen in the last 24 hours.
    pub fn get_recent_first_seen(&self, limit: i64) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT freq_mhz, label, cls, band, first_seen
             FROM channels
             WHERE first_seen >= datetime('now', '-24 hours')
             ORDER BY first_seen DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(serde_json::json!({
                "freq": row.get::<_, Option<f64>>(0)?,
                "name": row.get::<_, String>(1)?,
                "cls": row.get::<_, String>(2)?,
                "band": row.get::<_, String>(3)?,
                "first_seen": row.get::<_, Option<String>>(4)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    // ── Enhanced Intelligence Queries ────────────────────────────────────

    pub fn list_encrypted_grants(&self, site_id: i64, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT cg.id, cg.tgid, cg.uid, cg.voice_freq, cg.timestamp, cg.grant_type,
                    nt.name, nt.department, nt.algorithm, nt.encrypted,
                    ek.algorithm_name, ek.key_id
             FROM channel_grants cg
             LEFT JOIN network_talkgroups nt ON nt.tgid = cg.tgid AND nt.system = cg.system
             LEFT JOIN encryption_keys ek ON ek.tgid = cg.tgid AND ek.system = cg.system
             WHERE (nt.encrypted = 'encrypted' OR ek.algorithm_id IS NOT NULL)
               AND cg.timestamp >= (SELECT MIN(start_time) FROM site_sessions WHERE site_id = ?1)
               AND EXISTS (SELECT 1 FROM site_sessions ss WHERE ss.site_id = ?1
                AND cg.timestamp >= ss.start_time
                AND (ss.end_time IS NULL OR cg.timestamp <= ss.end_time))
             ORDER BY cg.timestamp DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "tgid": row.get::<_, i32>(1)?,
                "uid": row.get::<_, Option<i32>>(2)?,
                "voice_freq": row.get::<_, Option<f64>>(3)?,
                "timestamp": row.get::<_, String>(4)?,
                "grant_type": row.get::<_, Option<String>>(5)?,
                "tg_name": row.get::<_, Option<String>>(6)?,
                "department": row.get::<_, Option<String>>(7)?,
                "algorithm": row.get::<_, Option<String>>(8)?,
                "enc_status": row.get::<_, Option<String>>(9)?,
                "algorithm_name": row.get::<_, Option<String>>(10)?,
                "key_id": row.get::<_, Option<i32>>(11)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn site_radio_ids_enriched(&self, site_id: i64, limit: usize) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT cg.uid, COUNT(*) as observations,
                    MIN(cg.timestamp) as first_seen, MAX(cg.timestamp) as last_seen,
                    GROUP_CONCAT(DISTINCT nt.department) as departments,
                    GROUP_CONCAT(DISTINCT cg.tgid) as tgids,
                    GROUP_CONCAT(DISTINCT nt.name) as tg_names,
                    a.callsign, o.name as org_name
             FROM channel_grants cg
             LEFT JOIN network_talkgroups nt ON nt.tgid = cg.tgid AND nt.system = cg.system
             LEFT JOIN actor_radio_ids ari ON ari.radio_id = cg.uid AND ari.system = cg.system
             LEFT JOIN actors a ON a.id = ari.actor_id
             LEFT JOIN organizations o ON o.id = a.organization_id
             WHERE cg.uid IS NOT NULL AND cg.uid > 0
               AND cg.timestamp >= (SELECT MIN(start_time) FROM site_sessions WHERE site_id = ?1)
               AND EXISTS (SELECT 1 FROM site_sessions ss WHERE ss.site_id = ?1
                AND cg.timestamp >= ss.start_time
                AND (ss.end_time IS NULL OR cg.timestamp <= ss.end_time))
             GROUP BY cg.uid ORDER BY observations DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id, limit as i64], |row| {
            Ok(serde_json::json!({
                "uid": row.get::<_, i32>(0)?,
                "observations": row.get::<_, i64>(1)?,
                "first_seen": row.get::<_, String>(2)?,
                "last_seen": row.get::<_, String>(3)?,
                "departments": row.get::<_, Option<String>>(4)?,
                "tgids": row.get::<_, Option<String>>(5)?,
                "tg_names": row.get::<_, Option<String>>(6)?,
                "callsign": row.get::<_, Option<String>>(7)?,
                "org_name": row.get::<_, Option<String>>(8)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn site_activity_pattern_detailed(&self, site_id: i64) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "WITH ranges AS (
                SELECT start_time, COALESCE(end_time, datetime('now')) AS end_time
                FROM site_sessions WHERE site_id = ?1
             )
             SELECT CAST(strftime('%H', cg.timestamp) AS INTEGER) as hour,
                    CAST(strftime('%w', cg.timestamp) AS INTEGER) as dow,
                    COUNT(*) as grants,
                    SUM(CASE WHEN nt.encrypted = 'encrypted' THEN 1 ELSE 0 END) as encrypted_grants
             FROM channel_grants cg
             LEFT JOIN network_talkgroups nt ON nt.tgid = cg.tgid AND nt.system = cg.system
             WHERE cg.timestamp >= (SELECT MIN(start_time) FROM ranges)
               AND EXISTS (SELECT 1 FROM ranges r
                WHERE cg.timestamp >= r.start_time AND cg.timestamp <= r.end_time)
             GROUP BY hour, dow ORDER BY dow, hour"
        )?;
        let rows = stmt.query_map(rusqlite::params![site_id], |row| {
            Ok(serde_json::json!({
                "hour": row.get::<_, i32>(0)?,
                "dow": row.get::<_, i32>(1)?,
                "grants": row.get::<_, i64>(2)?,
                "encrypted_grants": row.get::<_, i64>(3)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn encryption_posture_detailed(&self, _site_id: i64) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT nt.tgid, nt.name, nt.department, nt.encrypted, nt.algorithm,
                    ek.algorithm_name, ek.key_id,
                    COUNT(cg.id) as grant_count,
                    MAX(cg.timestamp) as last_activity,
                    (SELECT COUNT(*) FROM key_rotation_events kr WHERE kr.tgid = nt.tgid) as rotation_count,
                    (SELECT MAX(kr.timestamp) FROM key_rotation_events kr WHERE kr.tgid = nt.tgid) as last_rotation
             FROM network_talkgroups nt
             LEFT JOIN channel_grants cg ON cg.tgid = nt.tgid
             LEFT JOIN encryption_keys ek ON ek.tgid = nt.tgid AND ek.system = nt.system
             WHERE nt.encrypted = 'encrypted'
             GROUP BY nt.tgid ORDER BY grant_count DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "tgid": row.get::<_, i32>(0)?,
                "name": row.get::<_, Option<String>>(1)?,
                "department": row.get::<_, Option<String>>(2)?,
                "encrypted": row.get::<_, Option<String>>(3)?,
                "algorithm": row.get::<_, Option<String>>(4)?,
                "algorithm_name": row.get::<_, Option<String>>(5)?,
                "key_id": row.get::<_, Option<i32>>(6)?,
                "grant_count": row.get::<_, i64>(7)?,
                "last_activity": row.get::<_, Option<String>>(8)?,
                "rotation_count": row.get::<_, i64>(9)?,
                "last_rotation": row.get::<_, Option<String>>(10)?,
            }))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Count active (open) observations.
    pub fn active_observation_count(&self) -> Result<u32, rusqlite::Error> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM observations WHERE end_time IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(count as u32)
    }

    // ── Channel Grants Retention ──

    /// Compact old channel_grants into hourly aggregates, then delete raw rows.
    /// Returns (aggregated_hours, deleted_rows).
    pub fn compact_channel_grants(&self, retention_days: i64) -> Result<(i64, i64), rusqlite::Error> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let cutoff = format!("-{} days", retention_days);

        // Aggregate old grants into hourly buckets
        let aggregated = tx.execute(
            "INSERT OR REPLACE INTO channel_grants_hourly (system, tgid, grant_type, hour, grant_count, unique_uids, encrypted_count, operation_id)
             SELECT
                 COALESCE(g.system, ''),
                 g.tgid,
                 COALESCE(g.grant_type, ''),
                 strftime('%Y-%m-%dT%H:00:00', g.timestamp) AS hour,
                 COUNT(*) AS grant_count,
                 COUNT(DISTINCT g.uid) AS unique_uids,
                 SUM(CASE WHEN t.encrypted = 1 THEN 1 ELSE 0 END) AS encrypted_count,
                 MAX(g.operation_id) AS operation_id
             FROM channel_grants g
             LEFT JOIN network_talkgroups t ON g.tgid = t.tgid AND g.system = t.system
             WHERE g.timestamp < datetime('now', ?1)
             GROUP BY COALESCE(g.system, ''), g.tgid, COALESCE(g.grant_type, ''), strftime('%Y-%m-%dT%H:00:00', g.timestamp)",
            rusqlite::params![cutoff],
        )? as i64;

        // Delete the raw rows that were aggregated
        let deleted = tx.execute(
            "DELETE FROM channel_grants WHERE timestamp < datetime('now', ?1)",
            rusqlite::params![cutoff],
        )? as i64;

        tx.commit()?;
        Ok((aggregated, deleted))
    }

    // ── Dashboard Cache ──

    /// Store a dashboard cache entry.
    pub fn update_dashboard_cache(&self, key: &str, site_id: i64, data: &serde_json::Value) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let json_str = data.to_string();
        conn.execute(
            "INSERT OR REPLACE INTO dashboard_cache (cache_key, site_id, data_json, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![key, site_id, json_str],
        )?;
        Ok(())
    }

    /// Get a cached dashboard entry.
    pub fn get_dashboard_cache(&self, key: &str, site_id: i64) -> Result<Option<serde_json::Value>, rusqlite::Error> {
        let conn = self.read_conn();
        let result: Option<String> = conn.query_row(
            "SELECT data_json FROM dashboard_cache WHERE cache_key = ?1 AND site_id = ?2",
            rusqlite::params![key, site_id],
            |row| row.get(0),
        ).optional()?;
        Ok(result.and_then(|s| serde_json::from_str(&s).ok()))
    }

    /// Refresh site dashboard cache — runs the expensive queries and stores results.
    pub fn refresh_site_dashboard_cache(&self, site_id: i64) -> Result<(), rusqlite::Error> {
        // Aggregate key metrics for the site
        let conn = self.read_conn();

        // Total grants for this site's session time ranges
        let total_grants: i64 = conn.query_row(
            "SELECT COUNT(*) FROM channel_grants g
             JOIN site_sessions ss ON g.timestamp BETWEEN ss.start_time AND COALESCE(ss.end_time, datetime('now'))
             WHERE ss.site_id = ?1",
            rusqlite::params![site_id],
            |r| r.get(0),
        ).unwrap_or(0);

        // Unique talkgroups
        let unique_tgs: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT g.tgid) FROM channel_grants g
             JOIN site_sessions ss ON g.timestamp BETWEEN ss.start_time AND COALESCE(ss.end_time, datetime('now'))
             WHERE ss.site_id = ?1",
            rusqlite::params![site_id],
            |r| r.get(0),
        ).unwrap_or(0);

        // Unique radio IDs
        let unique_uids: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT g.uid) FROM channel_grants g
             JOIN site_sessions ss ON g.timestamp BETWEEN ss.start_time AND COALESCE(ss.end_time, datetime('now'))
             WHERE ss.site_id = ?1 AND g.uid IS NOT NULL",
            rusqlite::params![site_id],
            |r| r.get(0),
        ).unwrap_or(0);

        // Encrypted percentage
        let encrypted_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM channel_grants g
             JOIN site_sessions ss ON g.timestamp BETWEEN ss.start_time AND COALESCE(ss.end_time, datetime('now'))
             WHERE ss.site_id = ?1 AND g.encrypted = 1",
            rusqlite::params![site_id],
            |r| r.get(0),
        ).unwrap_or(0);

        drop(conn); // Release read lock before writing

        let data = serde_json::json!({
            "total_grants": total_grants,
            "unique_talkgroups": unique_tgs,
            "unique_radio_ids": unique_uids,
            "encrypted_count": encrypted_count,
            "encrypted_pct": if total_grants > 0 { encrypted_count as f64 / total_grants as f64 * 100.0 } else { 0.0 },
        });

        self.update_dashboard_cache("site_dashboard", site_id, &data)?;
        Ok(())
    }

    // ── RF Fingerprinting (typed) ──────────────────────────────

    /// List fingerprints as typed structs (uses existing v8+v33 schema).
    pub fn list_fingerprints_typed(&self, limit: i64) -> Result<Vec<RadioFingerprint>, rusqlite::Error> {
        let conn = self.read_pool.acquire();
        let mut stmt = conn.prepare(
            "SELECT id, fingerprint_id, uid, freq_offset_hz, iq_imbalance, evm, phase_noise,
                    confidence, capture_count, freq_mhz, sample_count, first_seen, last_seen
             FROM radio_fingerprints ORDER BY last_seen DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |r| {
            Ok(RadioFingerprint {
                id: r.get(0)?,
                fingerprint_id: r.get(1)?,
                uid: r.get(2)?,
                freq_offset_hz: r.get(3)?,
                iq_imbalance: r.get(4)?,
                evm: r.get(5)?,
                phase_noise: r.get(6)?,
                confidence: r.get(7)?,
                capture_count: r.get::<_, Option<i32>>(8)?.unwrap_or(1),
                freq_mhz: r.get(9)?,
                sample_count: r.get(10)?,
                first_seen: r.get(11)?,
                last_seen: r.get(12)?,
            })
        })?;
        rows.collect()
    }

    /// Match fingerprints by CFO (freq_offset_hz) and IQ imbalance within tolerance.
    pub fn match_fingerprint_typed(&self, cfo_hz: f64, iq_imbal: f64, tolerance: f64) -> Result<Vec<RadioFingerprint>, rusqlite::Error> {
        let conn = self.read_pool.acquire();
        let mut stmt = conn.prepare(
            "SELECT id, fingerprint_id, uid, freq_offset_hz, iq_imbalance, evm, phase_noise,
                    confidence, capture_count, freq_mhz, sample_count, first_seen, last_seen
             FROM radio_fingerprints
             WHERE freq_offset_hz IS NOT NULL AND iq_imbalance IS NOT NULL
               AND ABS(freq_offset_hz - ?1) < ?3 AND ABS(iq_imbalance - ?2) < ?3
             ORDER BY ABS(freq_offset_hz - ?1) + ABS(iq_imbalance - ?2) ASC
             LIMIT 20"
        )?;
        let rows = stmt.query_map(rusqlite::params![cfo_hz, iq_imbal, tolerance], |r| {
            Ok(RadioFingerprint {
                id: r.get(0)?,
                fingerprint_id: r.get(1)?,
                uid: r.get(2)?,
                freq_offset_hz: r.get(3)?,
                iq_imbalance: r.get(4)?,
                evm: r.get(5)?,
                phase_noise: r.get(6)?,
                confidence: r.get(7)?,
                capture_count: r.get::<_, Option<i32>>(8)?.unwrap_or(1),
                freq_mhz: r.get(9)?,
                sample_count: r.get(10)?,
                first_seen: r.get(11)?,
                last_seen: r.get(12)?,
            })
        })?;
        rows.collect()
    }

    /// List UID-fingerprint links as typed structs.
    pub fn list_uid_fp_links_typed(&self, limit: i64) -> Result<Vec<UidFingerprintLink>, rusqlite::Error> {
        let conn = self.read_pool.acquire();
        let mut stmt = conn.prepare(
            "SELECT id, uid, fingerprint_id, tgid, system, observation_count, first_seen, last_seen
             FROM uid_fingerprint_map ORDER BY last_seen DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |r| {
            Ok(UidFingerprintLink {
                id: r.get(0)?,
                uid: r.get(1)?,
                fingerprint_id: r.get(2)?,
                tgid: r.get(3)?,
                system: r.get(4)?,
                observation_count: r.get::<_, Option<i32>>(5)?.unwrap_or(1),
                first_seen: r.get(6)?,
                last_seen: r.get(7)?,
            })
        })?;
        rows.collect()
    }

    /// Get UIDs linked to a specific fingerprint_id string.
    pub fn get_uids_for_fp_typed(&self, fingerprint_id: &str) -> Result<Vec<UidFingerprintLink>, rusqlite::Error> {
        let conn = self.read_pool.acquire();
        let mut stmt = conn.prepare(
            "SELECT id, uid, fingerprint_id, tgid, system, observation_count, first_seen, last_seen
             FROM uid_fingerprint_map WHERE fingerprint_id = ?1
             ORDER BY observation_count DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![fingerprint_id], |r| {
            Ok(UidFingerprintLink {
                id: r.get(0)?,
                uid: r.get(1)?,
                fingerprint_id: r.get(2)?,
                tgid: r.get(3)?,
                system: r.get(4)?,
                observation_count: r.get::<_, Option<i32>>(5)?.unwrap_or(1),
                first_seen: r.get(6)?,
                last_seen: r.get(7)?,
            })
        })?;
        rows.collect()
    }

    /// Typed fingerprint stats.
    pub fn fingerprint_stats_typed(&self) -> Result<FingerprintStatsTyped, rusqlite::Error> {
        let conn = self.read_pool.acquire();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM radio_fingerprints", [], |r| r.get(0))?;
        let unique: i64 = conn.query_row("SELECT COUNT(DISTINCT fingerprint_id) FROM radio_fingerprints", [], |r| r.get(0))?;
        let uid_links: i64 = conn.query_row("SELECT COUNT(*) FROM uid_fingerprint_map", [], |r| r.get(0))?;
        let linked_uids: i64 = conn.query_row("SELECT COUNT(DISTINCT uid) FROM uid_fingerprint_map", [], |r| r.get(0))?;
        let avg_conf: f64 = conn.query_row(
            "SELECT COALESCE(AVG(confidence), 0.0) FROM radio_fingerprints", [], |r| r.get(0)
        )?;
        Ok(FingerprintStatsTyped {
            total_fingerprints: total,
            unique_emitters: unique,
            uid_links,
            linked_uids,
            avg_confidence: avg_conf,
        })
    }
}
