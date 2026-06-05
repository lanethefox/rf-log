-- RF-SENTINEL schema v1

CREATE TABLE IF NOT EXISTS emitters (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    freq_mhz    REAL    NOT NULL,
    emitter_type TEXT,
    id_match    TEXT,
    confidence  REAL    DEFAULT 0,
    first_seen  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'UNKNOWN', -- KNOWN | UNKNOWN | NEW | GONE
    notes       TEXT,
    fingerprint_json TEXT
);

CREATE TABLE IF NOT EXISTS emitter_reference (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT    NOT NULL,
    emitter_type TEXT    NOT NULL,
    freq_min_mhz REAL    NOT NULL,
    freq_max_mhz REAL    NOT NULL,
    pri_min_us   REAL,
    pri_max_us   REAL,
    pw_min_us    REAL,
    pw_max_us    REAL,
    notes        TEXT
);

CREATE TABLE IF NOT EXISTS pdw_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    emitter_id  INTEGER REFERENCES emitters(id),
    toa         REAL    NOT NULL,
    pw_us       REAL    NOT NULL,
    freq_mhz    REAL    NOT NULL,
    amplitude_dbfs REAL,
    pri_us      REAL
);

CREATE TABLE IF NOT EXISTS baselines (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    location    TEXT,
    captured_at INTEGER NOT NULL,
    freq_start_mhz REAL NOT NULL,
    freq_end_mhz   REAL NOT NULL,
    bin_count   INTEGER NOT NULL,
    lat         REAL,
    lon         REAL,
    notes       TEXT
);

CREATE TABLE IF NOT EXISTS baseline_bins (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    baseline_id  INTEGER NOT NULL REFERENCES baselines(id) ON DELETE CASCADE,
    bin_index    INTEGER NOT NULL,
    freq_mhz     REAL    NOT NULL,
    mean         REAL    NOT NULL,
    std_dev      REAL    NOT NULL,
    min_val      REAL    NOT NULL,
    max_val      REAL    NOT NULL,
    sample_count INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS anomalies (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at INTEGER NOT NULL,
    freq_mhz    REAL    NOT NULL,
    kind        TEXT    NOT NULL,
    delta_db    REAL,
    z_score     REAL,
    baseline_id INTEGER REFERENCES baselines(id),
    severity    TEXT    NOT NULL DEFAULT 'WARNING',
    acknowledged INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS surveys (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    location    TEXT,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    operator    TEXT,
    status      TEXT    NOT NULL DEFAULT 'IN_PROGRESS',
    checklist_json TEXT,
    lat         REAL,
    lon         REAL
);

CREATE TABLE IF NOT EXISTS survey_findings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    survey_id   INTEGER NOT NULL REFERENCES surveys(id) ON DELETE CASCADE,
    emitter_id  INTEGER REFERENCES emitters(id),
    category    TEXT    NOT NULL,
    freq_mhz    REAL,
    notes       TEXT,
    photo_path  TEXT,
    found_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS reports (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    template    TEXT    NOT NULL,
    title       TEXT    NOT NULL,
    location    TEXT,
    created_at  INTEGER NOT NULL,
    content_json TEXT,
    export_hash TEXT,
    author      TEXT
);

CREATE TABLE IF NOT EXISTS harmonic_groups (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at     INTEGER NOT NULL,
    fundamental_mhz REAL    NOT NULL,
    harmonics_json  TEXT    NOT NULL,
    source_hypothesis TEXT
);

CREATE TABLE IF NOT EXISTS wireless_devices (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    bssid       TEXT,
    ssid        TEXT,
    device_type TEXT    NOT NULL DEFAULT 'WIFI',
    channel     INTEGER,
    signal_dbm  REAL,
    encryption  TEXT,
    known       INTEGER NOT NULL DEFAULT 0,
    first_seen  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL,
    sensor_id   TEXT,
    notes       TEXT
);

CREATE TABLE IF NOT EXISTS cellular_observations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    observed_at INTEGER NOT NULL,
    freq_mhz    REAL    NOT NULL,
    signal_dbm  REAL,
    expected    INTEGER NOT NULL DEFAULT 1,
    notes       TEXT,
    sensor_id   TEXT
);

CREATE TABLE IF NOT EXISTS drone_signatures (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    manufacturer TEXT    NOT NULL,
    model        TEXT    NOT NULL,
    freq_ranges_json TEXT NOT NULL,
    bandwidth_mhz REAL,
    notes        TEXT,
    builtin      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS drone_detections (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at     INTEGER NOT NULL,
    detection_method TEXT   NOT NULL,
    manufacturer    TEXT,
    model           TEXT,
    confidence      REAL,
    signal_dbm      REAL,
    freq_mhz        REAL,
    sensor_id       TEXT    NOT NULL DEFAULT 'workstation',
    track_id        INTEGER
);

CREATE TABLE IF NOT EXISTS drone_remote_id (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    received_at     INTEGER NOT NULL,
    ua_type         TEXT,
    serial_number   TEXT,
    session_id      TEXT,
    lat             REAL,
    lon             REAL,
    altitude_m      REAL,
    speed_ms        REAL,
    heading_deg     REAL,
    operator_lat    REAL,
    operator_lon    REAL,
    sensor_id       TEXT    NOT NULL DEFAULT 'workstation',
    track_id        INTEGER
);

CREATE TABLE IF NOT EXISTS drone_tracks (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    first_seen          INTEGER NOT NULL,
    last_seen           INTEGER NOT NULL,
    detection_methods   TEXT,
    peak_signal_dbm     REAL,
    manufacturer        TEXT,
    model               TEXT,
    serial_number       TEXT,
    whitelisted         INTEGER NOT NULL DEFAULT 0,
    notes               TEXT
);

CREATE TABLE IF NOT EXISTS drone_whitelist (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    serial_number TEXT    NOT NULL UNIQUE,
    owner         TEXT,
    purpose       TEXT,
    notes         TEXT,
    added_at      INTEGER NOT NULL
);

-- Seed built-in emitter reference library
INSERT OR IGNORE INTO emitter_reference (name, emitter_type, freq_min_mhz, freq_max_mhz, pri_min_us, pri_max_us, pw_min_us, pw_max_us, notes)
VALUES
('WSR-88D NEXRAD', 'WEATHER_RADAR', 2700, 3000, 750, 3100, 1.5, 4.7, 'S-band weather radar, FAA/NWS'),
('ASR-9 Airport Surveillance', 'AIRPORT_RADAR', 2700, 2900, 1000, 1300, 1.0, 1.2, 'S-band ASR, 12.5 RPM'),
('ASR-11 DASR', 'AIRPORT_RADAR', 2700, 2900, 833, 1000, 1.0, 1.2, 'Digital ASR'),
('Marine X-band Radar', 'MARINE_RADAR', 9300, 9500, 500, 1500, 0.07, 1.0, 'X-band ship/harbor radar'),
('Marine S-band Radar', 'MARINE_RADAR', 2900, 3100, 500, 1500, 0.3, 1.0, 'S-band marine radar'),
('ARSR-4 Air Route Surveillance', 'ATC_RADAR', 1215, 1350, 2000, 4000, 2.0, 4.0, 'L-band long-range ATC'),
('TDWR Terminal Doppler', 'WEATHER_RADAR', 5600, 5650, 500, 1000, 1.1, 2.0, 'C-band terminal weather'),
('Speed Radar (X-band)', 'TRAFFIC_RADAR', 10500, 10550, NULL, NULL, NULL, NULL, 'Police/traffic CW X-band'),
('Speed Radar (K-band)', 'TRAFFIC_RADAR', 24050, 24250, NULL, NULL, NULL, NULL, 'Police/traffic CW K-band'),
('Speed Radar (Ka-band)', 'TRAFFIC_RADAR', 33400, 36000, NULL, NULL, NULL, NULL, 'Police/traffic CW Ka-band');
