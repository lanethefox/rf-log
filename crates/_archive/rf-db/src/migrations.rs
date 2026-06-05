use rusqlite::Connection;
use std::path::Path;

pub fn run(conn: &Connection, data_dir: &Path) -> Result<(), rusqlite::Error> {
    // Enable WAL mode, foreign keys, and busy timeout (avoids SQLITE_BUSY under contention)
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;"
    )?;

    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS operations (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                config_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY,
                operation_id INTEGER NOT NULL REFERENCES operations(id),
                start TEXT NOT NULL,
                end_time TEXT,
                mode TEXT NOT NULL DEFAULT 'scan'
            );
            PRAGMA user_version = 1;"
        )?;
        tracing::info!("Database migrated to v1");
    }

    if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS signals (
                id INTEGER PRIMARY KEY,
                freq REAL NOT NULL,
                name TEXT NOT NULL,
                cls TEXT NOT NULL,
                band TEXT NOT NULL,
                mode TEXT,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                total_hits INTEGER NOT NULL DEFAULT 1,
                UNIQUE(freq)
            );
            CREATE TABLE IF NOT EXISTS signal_hits (
                id INTEGER PRIMARY KEY,
                signal_id INTEGER NOT NULL REFERENCES signals(id),
                power REAL NOT NULL,
                timestamp TEXT NOT NULL,
                session_id INTEGER REFERENCES sessions(id)
            );
            CREATE INDEX IF NOT EXISTS idx_signal_hits_signal ON signal_hits(signal_id);
            CREATE INDEX IF NOT EXISTS idx_signal_hits_time ON signal_hits(timestamp);
            PRAGMA user_version = 2;"
        )?;
        tracing::info!("Database migrated to v2 (signals, signal_hits)");
    }

    if version < 3 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS debug_log (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                band TEXT NOT NULL,
                center_freq REAL,
                gain REAL,
                threshold REAL,
                noise_floor REAL,
                peak_power REAL,
                peak_freq REAL,
                n_signals INTEGER DEFAULT 0,
                psd_min REAL,
                psd_max REAL,
                psd_mean REAL
            );
            CREATE INDEX IF NOT EXISTS idx_debug_log_time ON debug_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_debug_log_band ON debug_log(band);
            PRAGMA user_version = 3;"
        )?;
        tracing::info!("Database migrated to v3 (debug_log)");
    }

    if version < 4 {
        conn.execute_batch(
            "ALTER TABLE signals ADD COLUMN source TEXT DEFAULT 'rtl-sdr';
            ALTER TABLE signals ADD COLUMN tgid INTEGER;
            ALTER TABLE signals ADD COLUMN radio_id INTEGER;
            ALTER TABLE signals ADD COLUMN system_name TEXT;
            ALTER TABLE signals ADD COLUMN department TEXT;
            ALTER TABLE signals ADD COLUMN channel_name TEXT;
            ALTER TABLE signal_hits ADD COLUMN source TEXT;
            CREATE INDEX IF NOT EXISTS idx_signals_source ON signals(source);
            CREATE INDEX IF NOT EXISTS idx_signals_tgid ON signals(tgid);
            PRAGMA user_version = 4;"
        )?;
        tracing::info!("Database migrated to v4 (source + talkgroup columns)");
    }

    if version < 5 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scan_packages (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS scan_package_items (
                id INTEGER PRIMARY KEY,
                package_id INTEGER NOT NULL REFERENCES scan_packages(id),
                target_type TEXT NOT NULL,
                target_index INTEGER NOT NULL,
                target_name TEXT DEFAULT '',
                tgid INTEGER,
                UNIQUE(package_id, target_type, target_index)
            );
            CREATE INDEX IF NOT EXISTS idx_scan_package_items_pkg ON scan_package_items(package_id);
            PRAGMA user_version = 5;"
        )?;
        tracing::info!("Database migrated to v5 (scan_packages + scan_package_items)");
    }

    if version < 6 {
        conn.execute_batch(
            "ALTER TABLE scan_package_items ADD COLUMN freq_mhz REAL;
            PRAGMA user_version = 6;"
        )?;

        // Seed default scan packages from Portland frequency database
        seed_default_packages(conn)?;
        tracing::info!("Database migrated to v6 (freq_mhz column + default packages)");
    }

    if version < 7 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS custom_channels (
                id INTEGER PRIMARY KEY,
                freq REAL NOT NULL,
                name TEXT NOT NULL,
                cls TEXT NOT NULL DEFAULT 'UNK',
                band TEXT NOT NULL,
                mode TEXT,
                notes TEXT DEFAULT '',
                created_at TEXT DEFAULT (datetime('now')),
                UNIQUE(freq)
            );
            CREATE INDEX IF NOT EXISTS idx_custom_channels_band ON custom_channels(band);
            PRAGMA user_version = 7;"
        )?;
        tracing::info!("Database migrated to v7 (custom_channels)");
    }

    if version < 8 {
        // Stub networks table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS networks (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                network_type TEXT NOT NULL DEFAULT 'analog',
                system_id TEXT,
                nac TEXT,
                color_code INTEGER,
                control_channels TEXT DEFAULT '[]',
                site_count INTEGER DEFAULT 0,
                description TEXT DEFAULT '',
                notes TEXT DEFAULT '',
                owner TEXT DEFAULT '',
                channel_count INTEGER DEFAULT 0,
                encryption_posture TEXT DEFAULT 'unknown',
                source TEXT DEFAULT 'manual',
                first_seen TEXT,
                last_seen TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now')),
                UNIQUE(network_type, system_id)
            );"
        )?;

        // Channels table — core taxonomy entity
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channels (
                id INTEGER PRIMARY KEY,
                channel_type TEXT NOT NULL DEFAULT 'analog',
                freq_mhz REAL,
                tgid INTEGER,
                timeslot INTEGER,
                color_code INTEGER,
                label TEXT NOT NULL DEFAULT '',
                cls TEXT NOT NULL DEFAULT 'UNK',
                band TEXT NOT NULL DEFAULT '',
                mode TEXT,
                tag TEXT DEFAULT '',
                notes TEXT DEFAULT '',
                network_id INTEGER REFERENCES networks(id),
                total_hits INTEGER DEFAULT 0,
                total_seconds REAL DEFAULT 0.0,
                avg_power REAL,
                last_power REAL,
                first_seen TEXT,
                last_seen TEXT,
                encryption_seen INTEGER DEFAULT 0,
                encryption_current INTEGER DEFAULT 0,
                source TEXT DEFAULT 'freq_db',
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now')),
                UNIQUE(channel_type, freq_mhz, tgid, timeslot)
            );
            CREATE INDEX IF NOT EXISTS idx_channels_freq ON channels(freq_mhz);
            CREATE INDEX IF NOT EXISTS idx_channels_tgid ON channels(tgid);
            CREATE INDEX IF NOT EXISTS idx_channels_cls ON channels(cls);
            CREATE INDEX IF NOT EXISTS idx_channels_network ON channels(network_id);"
        )?;

        // Link signals to channels
        conn.execute_batch(
            "ALTER TABLE signals ADD COLUMN channel_id INTEGER REFERENCES channels(id);
            CREATE INDEX IF NOT EXISTS idx_signals_channel ON signals(channel_id);"
        )?;

        // Seed channels from portland-frequencies.json
        seed_channels_from_freq_db(conn, data_dir)?;

        // Migrate custom_channels → channels
        migrate_custom_channels(conn)?;

        conn.execute_batch("PRAGMA user_version = 8;")?;
        tracing::info!("Database migrated to v8 (networks stub, channels, signals.channel_id)");
    }

    if version < 9 {
        // ── Tier 1: Passive Spectrum Analysis ──

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS traffic_sessions (
                id              INTEGER PRIMARY KEY,
                uid             INTEGER,
                tgid            INTEGER,
                system          TEXT,
                freq_mhz        REAL,
                channel_id      INTEGER REFERENCES channels(id),
                start_time      TEXT NOT NULL,
                end_time        TEXT,
                duration_sec    REAL,
                hit_count       INTEGER DEFAULT 1,
                avg_signal      REAL,
                encrypted       INTEGER DEFAULT 0,
                modulation      TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_traffic_sessions_time ON traffic_sessions(start_time);
            CREATE INDEX IF NOT EXISTS idx_traffic_sessions_freq ON traffic_sessions(freq_mhz);
            CREATE INDEX IF NOT EXISTS idx_traffic_sessions_channel ON traffic_sessions(channel_id);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS activity_baselines (
                id              INTEGER PRIMARY KEY,
                freq_mhz        REAL,
                channel_id      INTEGER REFERENCES channels(id),
                tgid            INTEGER,
                system          TEXT,
                hour_of_day     INTEGER NOT NULL,
                day_of_week     INTEGER NOT NULL,
                avg_sessions    REAL,
                stddev_sessions REAL,
                avg_duration    REAL,
                avg_unique_uids REAL,
                sample_days     INTEGER,
                last_computed   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_baselines_channel ON activity_baselines(channel_id, hour_of_day, day_of_week);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS anomaly_events (
                id              INTEGER PRIMARY KEY,
                event_type      TEXT NOT NULL,
                freq_mhz        REAL,
                channel_id      INTEGER REFERENCES channels(id),
                tgid            INTEGER,
                uid             INTEGER,
                system          TEXT,
                severity        TEXT NOT NULL DEFAULT 'info',
                description     TEXT NOT NULL,
                anomaly_score   REAL,
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_anomaly_time ON anomaly_events(timestamp);
            CREATE INDEX IF NOT EXISTS idx_anomaly_severity ON anomaly_events(severity);"
        )?;

        // ── Tier 2: Targeted IQ Exploitation ──

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS radio_fingerprints (
                id              INTEGER PRIMARY KEY,
                fingerprint_id  TEXT NOT NULL,
                uid             INTEGER,
                freq_offset_hz  REAL,
                timing_jitter   REAL,
                evm             REAL,
                power_ramp_sig  BLOB,
                phase_noise     REAL,
                iq_imbalance    REAL,
                spectral_mask   BLOB,
                confidence      REAL,
                capture_count   INTEGER DEFAULT 1,
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_fingerprints_fid ON radio_fingerprints(fingerprint_id);
            CREATE INDEX IF NOT EXISTS idx_fingerprints_uid ON radio_fingerprints(uid);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS uid_fingerprint_map (
                id              INTEGER PRIMARY KEY,
                uid             INTEGER NOT NULL,
                fingerprint_id  TEXT NOT NULL,
                tgid            INTEGER,
                system          TEXT,
                observation_count INTEGER DEFAULT 1,
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_uid_fp_uid ON uid_fingerprint_map(uid);
            CREATE INDEX IF NOT EXISTS idx_uid_fp_fid ON uid_fingerprint_map(fingerprint_id);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS encryption_keys (
                id              INTEGER PRIMARY KEY,
                tgid            INTEGER NOT NULL,
                system          TEXT NOT NULL,
                algorithm_id    INTEGER,
                algorithm_name  TEXT,
                key_id          INTEGER,
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL,
                session_count   INTEGER DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_enc_keys_tgid ON encryption_keys(tgid, system);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS key_rotation_events (
                id              INTEGER PRIMARY KEY,
                tgid            INTEGER NOT NULL,
                system          TEXT NOT NULL,
                old_key_id      INTEGER,
                new_key_id      INTEGER,
                old_algorithm   INTEGER,
                new_algorithm   INTEGER,
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_key_rot_time ON key_rotation_events(timestamp);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS speculative_results (
                id              INTEGER PRIMARY KEY,
                freq_mhz        REAL NOT NULL,
                tgid            INTEGER,
                protocol        TEXT,
                classification  TEXT NOT NULL,
                confidence      REAL,
                frames_extracted INTEGER,
                metrics_json    TEXT,
                iq_path         TEXT,
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_spec_time ON speculative_results(timestamp);"
        )?;

        // ── Tier 3: Sustained Collection ──

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS network_sites (
                id              INTEGER PRIMARY KEY,
                system          TEXT NOT NULL,
                wacn            INTEGER,
                system_id       INTEGER,
                rfss_id         INTEGER,
                site_id         INTEGER,
                control_channel REAL,
                alt_control     TEXT,
                voice_channels  TEXT,
                adjacent_sites  TEXT,
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_net_sites_sys ON network_sites(system);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS network_talkgroups (
                id              INTEGER PRIMARY KEY,
                system          TEXT NOT NULL,
                tgid            INTEGER NOT NULL,
                name            TEXT,
                encrypted       TEXT DEFAULT 'unknown',
                algorithm       TEXT,
                total_grants    INTEGER DEFAULT 0,
                unique_uids     INTEGER DEFAULT 0,
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_net_tg_sys ON network_talkgroups(system, tgid);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_grants (
                id              INTEGER PRIMARY KEY,
                system          TEXT NOT NULL,
                tgid            INTEGER NOT NULL,
                uid             INTEGER,
                voice_freq      REAL,
                grant_type      TEXT,
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_grants_time ON channel_grants(timestamp);
            CREATE INDEX IF NOT EXISTS idx_grants_tgid ON channel_grants(tgid);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS radio_affiliations (
                id              INTEGER PRIMARY KEY,
                system          TEXT NOT NULL,
                uid             INTEGER NOT NULL,
                tgid            INTEGER NOT NULL,
                event_type      TEXT NOT NULL,
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_affil_time ON radio_affiliations(timestamp);
            CREATE INDEX IF NOT EXISTS idx_affil_uid ON radio_affiliations(uid);"
        )?;

        // ── Cross-tier: Unified SIGEX Event Log ──

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sigex_events (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                module          TEXT NOT NULL,
                event_type      TEXT NOT NULL,
                severity        TEXT DEFAULT 'info',
                summary         TEXT NOT NULL,
                details         TEXT,
                system          TEXT,
                tgid            INTEGER,
                uid             INTEGER,
                freq_mhz        REAL,
                channel_id      INTEGER REFERENCES channels(id),
                timestamp       TEXT NOT NULL,
                acknowledged    INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_sigex_events_time ON sigex_events(timestamp);
            CREATE INDEX IF NOT EXISTS idx_sigex_events_module ON sigex_events(module, event_type);
            CREATE INDEX IF NOT EXISTS idx_sigex_events_severity ON sigex_events(severity, acknowledged);"
        )?;

        // ── Intelligence Model ──

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS actors (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                callsign        TEXT,
                identifier      TEXT,
                description     TEXT,
                organization_id INTEGER REFERENCES organizations(id),
                actor_type      TEXT DEFAULT 'unknown',
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL,
                created_at      TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS organizations (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL,
                abbreviation    TEXT,
                org_type        TEXT,
                parent_id       INTEGER REFERENCES organizations(id),
                jurisdiction    TEXT,
                notes           TEXT
            );

            CREATE TABLE IF NOT EXISTS actor_radio_ids (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                actor_id        INTEGER NOT NULL REFERENCES actors(id),
                radio_id        INTEGER NOT NULL,
                system          TEXT,
                confidence      REAL DEFAULT 1.0,
                source          TEXT DEFAULT 'sigex',
                first_seen      TEXT NOT NULL,
                last_seen       TEXT NOT NULL,
                UNIQUE(actor_id, radio_id, system)
            );

            CREATE TABLE IF NOT EXISTS actor_attributes (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                actor_id        INTEGER NOT NULL REFERENCES actors(id),
                key             TEXT NOT NULL,
                value           TEXT NOT NULL,
                source          TEXT DEFAULT 'manual',
                timestamp       TEXT DEFAULT (datetime('now')),
                UNIQUE(actor_id, key)
            );

            CREATE TABLE IF NOT EXISTS org_talkgroups (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                organization_id INTEGER NOT NULL REFERENCES organizations(id),
                system          TEXT NOT NULL,
                tgid            INTEGER NOT NULL,
                tg_name         TEXT,
                encrypted       TEXT DEFAULT 'unknown',
                UNIQUE(organization_id, system, tgid)
            );

            CREATE INDEX IF NOT EXISTS idx_actors_org ON actors(organization_id);
            CREATE INDEX IF NOT EXISTS idx_actor_radio_ids_radio ON actor_radio_ids(radio_id, system);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS intel_sites (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL,
                site_type       TEXT,
                latitude        REAL,
                longitude       REAL,
                elevation_m     REAL,
                address         TEXT,
                notes           TEXT
            );

            CREATE TABLE IF NOT EXISTS site_frequencies (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                site_id         INTEGER NOT NULL REFERENCES intel_sites(id),
                freq_mhz        REAL NOT NULL,
                system          TEXT,
                tgid            INTEGER,
                signal_type     TEXT,
                UNIQUE(site_id, freq_mhz)
            );

            CREATE TABLE IF NOT EXISTS site_organizations (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                site_id         INTEGER NOT NULL REFERENCES intel_sites(id),
                organization_id INTEGER NOT NULL REFERENCES organizations(id),
                relationship    TEXT,
                UNIQUE(site_id, organization_id)
            );

            CREATE TABLE IF NOT EXISTS site_observations (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                site_id         INTEGER NOT NULL REFERENCES intel_sites(id),
                actor_id        INTEGER REFERENCES actors(id),
                radio_id        INTEGER,
                freq_mhz        REAL,
                observation_type TEXT,
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_site_obs_time ON site_observations(timestamp);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS humint_observations (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                observer        TEXT,
                observation     TEXT NOT NULL,
                actor_id        INTEGER REFERENCES actors(id),
                site_id         INTEGER REFERENCES intel_sites(id),
                freq_mhz        REAL,
                tgid            INTEGER,
                confidence      TEXT DEFAULT 'medium',
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_humint_time ON humint_observations(timestamp);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS alert_patterns (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL,
                description     TEXT,
                severity        TEXT DEFAULT 'warning',
                enabled         INTEGER DEFAULT 1,
                pattern_json    TEXT NOT NULL,
                cooldown_sec    INTEGER DEFAULT 3600
            );

            CREATE TABLE IF NOT EXISTS pattern_matches (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_id      INTEGER NOT NULL REFERENCES alert_patterns(id),
                match_score     REAL,
                match_details   TEXT,
                actors_involved TEXT,
                sites_involved  TEXT,
                freqs_involved  TEXT,
                timestamp       TEXT NOT NULL,
                acknowledged    INTEGER DEFAULT 0,
                escalated       INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_pattern_matches_time ON pattern_matches(timestamp);"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS key_rotation_schedules (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                system          TEXT NOT NULL,
                tgid            INTEGER,
                organization_id INTEGER REFERENCES organizations(id),
                schedule_type   TEXT NOT NULL,
                typical_day     INTEGER,
                typical_hour    INTEGER,
                confidence      REAL,
                last_rotation   TEXT,
                next_expected   TEXT,
                notes           TEXT
            );

            CREATE TABLE IF NOT EXISTS sigex_reports (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                report_type     TEXT NOT NULL,
                title           TEXT,
                content_json    TEXT NOT NULL,
                generated_by    TEXT,
                pattern_match_id INTEGER REFERENCES pattern_matches(id),
                site_id         INTEGER REFERENCES intel_sites(id),
                timestamp       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_reports_type ON sigex_reports(report_type, timestamp);"
        )?;

        conn.execute_batch("PRAGMA user_version = 9;")?;
        tracing::info!("Database migrated to v9 (SIGEX tables: traffic, fingerprints, encryption, network, events, intelligence model)");
    }

    if version < 10 {
        conn.execute_batch(
            "ALTER TABLE network_talkgroups ADD COLUMN department TEXT;
             ALTER TABLE network_talkgroups ADD COLUMN tag TEXT;
             DROP INDEX IF EXISTS idx_net_tg_sys;
             CREATE UNIQUE INDEX idx_net_tg_sys ON network_talkgroups(system, tgid);
             PRAGMA user_version = 10;"
        )?;
        tracing::info!("Database migrated to v10 (talkgroup department/tag columns + unique index)");
    }

    if version < 11 {
        conn.execute_batch(
            "ALTER TABLE network_sites ADD COLUMN name TEXT;
             PRAGMA user_version = 11;"
        )?;
        tracing::info!("Database migrated to v11 (network_sites.name column)");
    }

    if version < 12 {
        conn.execute_batch(
            "ALTER TABLE network_talkgroups ADD COLUMN priority INTEGER DEFAULT 0;
             ALTER TABLE network_talkgroups ADD COLUMN scan_enabled INTEGER DEFAULT 1;
             CREATE INDEX IF NOT EXISTS idx_net_tg_dept ON network_talkgroups(department);
             PRAGMA user_version = 12;"
        )?;
        tracing::info!("Database migrated to v12 (talkgroup priority + scan_enabled + department index)");
    }

    if version < 13 {
        reseed_channels_from_freq_db(conn, data_dir)?;
        conn.execute_batch("PRAGMA user_version = 13;")?;
        tracing::info!("Database migrated to v13 (reseed channels from updated frequency database)");
    }

    if version < 14 {
        conn.execute_batch(
            "ALTER TABLE debug_log ADD COLUMN device_key TEXT DEFAULT '';
             ALTER TABLE debug_log ADD COLUMN sample_rate REAL DEFAULT 0.0;
             ALTER TABLE debug_log ADD COLUMN modulation TEXT DEFAULT '';
             ALTER TABLE debug_log ADD COLUMN snr_margin REAL DEFAULT 0.0;
             ALTER TABLE debug_log ADD COLUMN agc INTEGER DEFAULT 0;
             ALTER TABLE debug_log ADD COLUMN ppm REAL DEFAULT 0.0;
             CREATE INDEX IF NOT EXISTS idx_debug_log_device ON debug_log(device_key);
             PRAGMA user_version = 14;"
        )?;
        tracing::info!("Database migrated to v14 (debug_log: device_key, sample_rate, modulation, snr_margin, agc, ppm)");
    }

    if version < 15 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sdr_devices (
                serial       TEXT PRIMARY KEY,
                user_name    TEXT DEFAULT '',
                manufacturer TEXT DEFAULT '',
                product      TEXT DEFAULT '',
                tuner        TEXT DEFAULT '',
                first_seen   TEXT DEFAULT (datetime('now')),
                last_seen    TEXT DEFAULT (datetime('now'))
            );
            PRAGMA user_version = 15;"
        )?;
        tracing::info!("Database migrated to v15 (sdr_devices table)");
    }

    if version < 16 {
        conn.execute_batch("PRAGMA user_version = 16;")?;
        tracing::info!("Database migrated to v16 (no-op, SDS100 tables removed)");
    }

    if version < 17 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS wx_alerts (
                id              INTEGER PRIMARY KEY,
                originator      TEXT NOT NULL,
                event_code      TEXT NOT NULL,
                event_name      TEXT NOT NULL,
                severity        TEXT NOT NULL,
                locations       TEXT NOT NULL,
                duration_mins   INTEGER,
                issued_utc      TEXT,
                station         TEXT,
                raw_header      TEXT NOT NULL,
                confidence      REAL,
                received_at     TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at      TEXT,
                freq_mhz        REAL,
                receiver_lat    REAL,
                receiver_lon    REAL
            );
            CREATE INDEX IF NOT EXISTS idx_wx_alerts_time ON wx_alerts(received_at);
            CREATE INDEX IF NOT EXISTS idx_wx_alerts_severity ON wx_alerts(severity);
            PRAGMA user_version = 17;"
        )?;
        tracing::info!("Database migrated to v17 (wx_alerts table)");
    }

    if version < 18 {
        conn.execute_batch(
            "ALTER TABLE intel_sites ADD COLUMN geofence_radius_m REAL DEFAULT 500.0;

             CREATE TABLE IF NOT EXISTS site_sessions (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 site_id     INTEGER NOT NULL REFERENCES intel_sites(id),
                 start_time  TEXT NOT NULL,
                 end_time    TEXT,
                 start_lat   REAL,
                 start_lon   REAL
             );
             CREATE INDEX IF NOT EXISTS idx_site_sessions_site ON site_sessions(site_id);
             CREATE INDEX IF NOT EXISTS idx_site_sessions_active ON site_sessions(end_time);

             PRAGMA user_version = 18;"
        )?;
        tracing::info!("Database migrated to v18 (intel_sites.geofence_radius_m + site_sessions table)");
    }

    if version < 19 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS recordings (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                rec_type        TEXT NOT NULL DEFAULT 'audio',
                freq_mhz        REAL NOT NULL,
                modulation      TEXT,
                label           TEXT,
                sample_rate     INTEGER NOT NULL,
                channels        INTEGER NOT NULL DEFAULT 1,
                file_path       TEXT NOT NULL,
                file_size_bytes INTEGER DEFAULT 0,
                duration_sec    REAL DEFAULT 0.0,
                start_time      TEXT NOT NULL DEFAULT (datetime('now')),
                end_time        TEXT,
                trigger_type    TEXT DEFAULT 'manual',
                tgid            INTEGER,
                device_key      TEXT,
                receiver_lat    REAL,
                receiver_lon    REAL,
                notes           TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_recordings_time ON recordings(start_time);
            CREATE INDEX IF NOT EXISTS idx_recordings_freq ON recordings(freq_mhz);
            CREATE INDEX IF NOT EXISTS idx_recordings_type ON recordings(rec_type);
            PRAGMA user_version = 19;"
        )?;
        tracing::info!("Database migrated to v19 (recordings table)");
    }

    if version < 20 {
        conn.execute_batch(
            "ALTER TABLE recordings ADD COLUMN site_id INTEGER REFERENCES intel_sites(id);
             ALTER TABLE recordings ADD COLUMN site_session_id INTEGER REFERENCES site_sessions(id);
             CREATE INDEX IF NOT EXISTS idx_recordings_site ON recordings(site_id);
             PRAGMA user_version = 20;"
        )?;
        tracing::info!("Database migrated to v20 (recordings: site_id + site_session_id columns)");
    }

    if version < 21 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS operators (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                callsign     TEXT NOT NULL UNIQUE,
                display_name TEXT DEFAULT '',
                notes        TEXT DEFAULT '',
                last_login   TEXT,
                created_at   TEXT DEFAULT (datetime('now'))
            );

            ALTER TABLE operations ADD COLUMN status TEXT DEFAULT 'active';
            ALTER TABLE operations ADD COLUMN description TEXT DEFAULT '';
            ALTER TABLE operations ADD COLUMN started_at TEXT;
            ALTER TABLE operations ADD COLUMN stopped_at TEXT;
            ALTER TABLE operations ADD COLUMN created_by INTEGER REFERENCES operators(id);

            ALTER TABLE sessions ADD COLUMN operator_id INTEGER REFERENCES operators(id);

            ALTER TABLE signal_hits ADD COLUMN operation_id INTEGER REFERENCES operations(id);
            ALTER TABLE traffic_sessions ADD COLUMN operation_id INTEGER REFERENCES operations(id);
            ALTER TABLE channel_grants ADD COLUMN operation_id INTEGER REFERENCES operations(id);
            ALTER TABLE recordings ADD COLUMN operation_id INTEGER REFERENCES operations(id);
            ALTER TABLE sigex_events ADD COLUMN operation_id INTEGER REFERENCES operations(id);

            CREATE INDEX IF NOT EXISTS idx_signal_hits_op ON signal_hits(operation_id);
            CREATE INDEX IF NOT EXISTS idx_traffic_sessions_op ON traffic_sessions(operation_id);
            CREATE INDEX IF NOT EXISTS idx_channel_grants_op ON channel_grants(operation_id);
            CREATE INDEX IF NOT EXISTS idx_recordings_op ON recordings(operation_id);
            CREATE INDEX IF NOT EXISTS idx_sigex_events_op ON sigex_events(operation_id);

            PRAGMA user_version = 21;"
        )?;
        tracing::info!("Database migrated to v21 (operators, operation lifecycle, data scoping)");
    }

    if version < 22 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS saved_queries (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL,
                sql_text   TEXT NOT NULL,
                chart_config TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );
            PRAGMA user_version = 22;"
        )?;
        tracing::info!("Database migrated to v22 (saved queries)");
    }

    if version < 25 {
        // Drop old antenna tables from rolled-back v23/v24 schema (different column layout)
        conn.execute_batch(
            "DROP TABLE IF EXISTS antenna_swr_data;
             DROP TABLE IF EXISTS environment_config;
             DROP TABLE IF EXISTS environment_assignments;
             DROP TABLE IF EXISTS environment_configs;
             DROP TABLE IF EXISTS device_antenna_assignments;
             DROP TABLE IF EXISTS antenna_freq_ranges;
             DROP INDEX IF EXISTS idx_antennas_device;
             DROP TABLE IF EXISTS antennas;"
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS antennas (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                antenna_type TEXT NOT NULL DEFAULT 'unknown',
                connector TEXT DEFAULT 'SMA',
                freq_min_mhz REAL,
                freq_max_mhz REAL,
                gain_dbi REAL,
                notes TEXT DEFAULT '',
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS antenna_freq_ranges (
                id INTEGER PRIMARY KEY,
                antenna_id INTEGER NOT NULL REFERENCES antennas(id) ON DELETE CASCADE,
                freq_min_mhz REAL NOT NULL,
                freq_max_mhz REAL NOT NULL,
                gain_dbi REAL,
                label TEXT DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_afr_antenna ON antenna_freq_ranges(antenna_id);

            CREATE TABLE IF NOT EXISTS device_antenna_assignments (
                id INTEGER PRIMARY KEY,
                device_serial TEXT NOT NULL,
                antenna_id INTEGER NOT NULL REFERENCES antennas(id) ON DELETE CASCADE,
                assigned_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(device_serial)
            );
            CREATE INDEX IF NOT EXISTS idx_daa_serial ON device_antenna_assignments(device_serial);

            CREATE TABLE IF NOT EXISTS environment_configs (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS environment_assignments (
                id INTEGER PRIMARY KEY,
                config_id INTEGER NOT NULL REFERENCES environment_configs(id) ON DELETE CASCADE,
                device_serial TEXT NOT NULL,
                antenna_id INTEGER NOT NULL REFERENCES antennas(id) ON DELETE CASCADE,
                UNIQUE(config_id, device_serial)
            );

            PRAGMA user_version = 25;"
        )?;
        tracing::info!("Database migrated to v25 (antenna schema: clean rebuild)");
    }

    if version < 26 {
        conn.execute_batch(
            "ALTER TABLE recordings ADD COLUMN source_unit INTEGER;
             ALTER TABLE recordings ADD COLUMN encrypted INTEGER DEFAULT 0;
             ALTER TABLE recordings ADD COLUMN algorithm TEXT;
             ALTER TABLE recordings ADD COLUMN key_id INTEGER;
             CREATE INDEX IF NOT EXISTS idx_recordings_tgid ON recordings(tgid);
             CREATE INDEX IF NOT EXISTS idx_recordings_trigger ON recordings(trigger_type);
             PRAGMA user_version = 26;"
        )?;
        tracing::info!("Database migrated to v26 (recordings: source_unit, encrypted, algorithm, key_id + indexes)");
    }

    if version < 27 {
        conn.execute_batch(
            "ALTER TABLE operations ADD COLUMN profile TEXT NOT NULL DEFAULT 'test';
             PRAGMA user_version = 27;"
        )?;
        tracing::info!("Database migrated to v27 (operations.profile column)");
    }

    if version < 28 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS observation_targets (
                id INTEGER PRIMARY KEY,
                target_type TEXT NOT NULL,
                target_key TEXT NOT NULL,
                target_label TEXT,
                site_id INTEGER REFERENCES intel_sites(id),
                priority INTEGER DEFAULT 0,
                notes TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                UNIQUE(target_type, target_key, site_id)
            );
            CREATE INDEX IF NOT EXISTS idx_obs_targets_type ON observation_targets(target_type);
            CREATE INDEX IF NOT EXISTS idx_obs_targets_site ON observation_targets(site_id);

            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY,
                target_id INTEGER REFERENCES observation_targets(id),
                site_id INTEGER REFERENCES intel_sites(id),
                site_session_id INTEGER REFERENCES site_sessions(id),
                operation_id INTEGER REFERENCES operations(id),
                start_time TEXT NOT NULL,
                end_time TEXT,
                duration_sec REAL,
                receiver_lat REAL,
                receiver_lon REAL,
                device_key TEXT,
                freq_mhz REAL,
                tgid INTEGER,
                uid INTEGER,
                encrypted INTEGER DEFAULT 0,
                signal_dbfs REAL,
                observation_type TEXT DEFAULT 'auto',
                metadata_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_observations_target ON observations(target_id);
            CREATE INDEX IF NOT EXISTS idx_observations_time ON observations(start_time);
            CREATE INDEX IF NOT EXISTS idx_observations_site ON observations(site_id);

            CREATE TABLE IF NOT EXISTS observation_alerts (
                id INTEGER PRIMARY KEY,
                target_id INTEGER REFERENCES observation_targets(id),
                alert_type TEXT NOT NULL,
                threshold_json TEXT,
                cooldown_sec INTEGER DEFAULT 300,
                enabled INTEGER DEFAULT 1,
                last_fired TEXT,
                fire_count INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_obs_alerts_target ON observation_alerts(target_id);

            CREATE TABLE IF NOT EXISTS auto_iq_rules (
                id INTEGER PRIMARY KEY,
                trigger_type TEXT NOT NULL,
                trigger_config_json TEXT,
                enabled INTEGER DEFAULT 1,
                max_duration_sec INTEGER DEFAULT 30,
                site_id INTEGER REFERENCES intel_sites(id),
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_auto_iq_site ON auto_iq_rules(site_id);

            PRAGMA user_version = 28;"
        )?;
        tracing::info!("Database migrated to v28 (observation_targets, observations, observation_alerts, auto_iq_rules)");
    }

    if version < 29 {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_grants_ts_tgid_uid ON channel_grants(timestamp, tgid, uid);
             CREATE INDEX IF NOT EXISTS idx_grants_uid ON channel_grants(uid);
             CREATE INDEX IF NOT EXISTS idx_grants_system ON channel_grants(system);
             PRAGMA user_version = 29;"
        )?;
        tracing::info!("Database migrated to v29 (channel_grants covering indexes for site-scoped queries)");
    }

    if version < 30 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_grants_hourly (
                system TEXT NOT NULL,
                tgid INTEGER NOT NULL,
                grant_type TEXT NOT NULL DEFAULT '',
                hour TEXT NOT NULL,
                grant_count INTEGER NOT NULL DEFAULT 0,
                unique_uids INTEGER NOT NULL DEFAULT 0,
                encrypted_count INTEGER NOT NULL DEFAULT 0,
                operation_id INTEGER,
                PRIMARY KEY (system, tgid, grant_type, hour)
            );
            CREATE INDEX IF NOT EXISTS idx_grants_hourly_hour ON channel_grants_hourly(hour);

            CREATE TABLE IF NOT EXISTS dashboard_cache (
                cache_key TEXT NOT NULL,
                site_id INTEGER,
                data_json TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (cache_key, site_id)
            );

            PRAGMA user_version = 30;"
        )?;
        tracing::info!("Database migrated to v30 (channel_grants_hourly retention, dashboard_cache)");
    }

    if version < 31 {
        conn.execute_batch(
            "ALTER TABLE observation_targets ADD COLUMN coverage_target_hours REAL DEFAULT 4.0;
            CREATE TABLE IF NOT EXISTS collection_requirements (
                id INTEGER PRIMARY KEY,
                label TEXT NOT NULL,
                check_type TEXT NOT NULL,
                check_config_json TEXT,
                site_id INTEGER REFERENCES intel_sites(id),
                met INTEGER DEFAULT 0,
                last_checked TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );
            PRAGMA user_version = 31;"
        )?;
        tracing::info!("Database migrated to v31 (coverage_target_hours, collection_requirements)");
    }

    if version < 32 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS operation_operators (
                operation_id INTEGER NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
                operator_id INTEGER NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
                role TEXT NOT NULL DEFAULT 'operator',
                assigned_at TEXT DEFAULT (datetime('now')),
                PRIMARY KEY (operation_id, operator_id)
            );
            PRAGMA user_version = 32;"
        )?;
        tracing::info!("Database migrated to v32 (operation_operators junction table)");
    }

    if version < 33 {
        conn.execute_batch(
            "ALTER TABLE radio_fingerprints ADD COLUMN freq_mhz REAL;
            ALTER TABLE radio_fingerprints ADD COLUMN sample_count INTEGER;
            CREATE INDEX IF NOT EXISTS idx_fingerprints_freq ON radio_fingerprints(freq_mhz);
            PRAGMA user_version = 33;"
        )?;
        tracing::info!("Database migrated to v33 (fingerprint freq_mhz + sample_count)");
    }

    if version < 34 {
        conn.execute_batch(
            "-- Unified event log (radio-driven SIEM)
            CREATE TABLE IF NOT EXISTS event_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp_ns INTEGER NOT NULL,
                ts_bucket INTEGER NOT NULL,
                severity INTEGER NOT NULL,
                source INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                body TEXT NOT NULL,

                freq_mhz REAL,
                talkgroup INTEGER,
                source_unit INTEGER,
                nac INTEGER,
                encrypted INTEGER,
                band TEXT,
                device_key TEXT,
                classification TEXT,

                trace_id INTEGER,
                span_id INTEGER,
                operation_id INTEGER,
                site_session_id INTEGER,

                receiver_lat REAL,
                receiver_lon REAL,

                attributes TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_event_ts ON event_log(ts_bucket, timestamp_ns);
            CREATE INDEX IF NOT EXISTS idx_event_type ON event_log(event_type, timestamp_ns);
            CREATE INDEX IF NOT EXISTS idx_event_tg ON event_log(talkgroup, timestamp_ns) WHERE talkgroup IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_event_uid ON event_log(source_unit, timestamp_ns) WHERE source_unit IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_event_freq ON event_log(freq_mhz, timestamp_ns) WHERE freq_mhz IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_event_trace ON event_log(trace_id) WHERE trace_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_event_severity ON event_log(severity, timestamp_ns);
            CREATE INDEX IF NOT EXISTS idx_event_op ON event_log(operation_id, timestamp_ns) WHERE operation_id IS NOT NULL;

            -- Transmission sessions (trace correlation)
            CREATE TABLE IF NOT EXISTS transmission_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id INTEGER NOT NULL UNIQUE,
                start_ns INTEGER NOT NULL,
                end_ns INTEGER,
                talkgroup INTEGER,
                source_unit INTEGER,
                nac INTEGER,
                freq_mhz REAL,
                encrypted INTEGER,
                event_count INTEGER DEFAULT 0,
                grant_event_id INTEGER REFERENCES event_log(id),
                recording_id INTEGER REFERENCES recordings(id),
                fingerprint_id INTEGER REFERENCES radio_fingerprints(id),
                operation_id INTEGER,
                site_session_id INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_txsess_time ON transmission_sessions(start_ns);
            CREATE INDEX IF NOT EXISTS idx_txsess_tg ON transmission_sessions(talkgroup, start_ns);
            CREATE INDEX IF NOT EXISTS idx_txsess_trace ON transmission_sessions(trace_id);

            -- Downsampled spectrum snapshots (1 row per band per 30s)
            CREATE TABLE IF NOT EXISTS spectrum_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts_bucket INTEGER NOT NULL,
                band TEXT NOT NULL,
                device_key TEXT NOT NULL DEFAULT '',
                noise_floor_db REAL,
                peak_power_db REAL,
                peak_freq_mhz REAL,
                signal_count INTEGER,
                avg_occupancy REAL,
                operation_id INTEGER,
                UNIQUE(ts_bucket, band, device_key)
            );
            CREATE INDEX IF NOT EXISTS idx_spec_snap ON spectrum_snapshots(ts_bucket, band);

            -- Alert rules (monitors)
            CREATE TABLE IF NOT EXISTS alert_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                priority TEXT NOT NULL DEFAULT 'medium',
                filter_json TEXT NOT NULL,
                condition_type TEXT NOT NULL,
                condition_json TEXT NOT NULL,
                actions_json TEXT NOT NULL DEFAULT '[]',
                cooldown_sec INTEGER NOT NULL DEFAULT 60,
                last_fired_ns INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Alert firing history
            CREATE TABLE IF NOT EXISTS alert_firings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rule_id INTEGER NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
                fired_ns INTEGER NOT NULL,
                match_count INTEGER,
                sample_event_id INTEGER REFERENCES event_log(id),
                acknowledged INTEGER NOT NULL DEFAULT 0,
                ack_ns INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_alert_fire_time ON alert_firings(fired_ns);
            CREATE INDEX IF NOT EXISTS idx_alert_fire_rule ON alert_firings(rule_id, fired_ns);

            -- Custom event rules (user-defined derived events)
            CREATE TABLE IF NOT EXISTS custom_event_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                event_type TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                filter_json TEXT NOT NULL,
                condition_type TEXT NOT NULL,
                condition_json TEXT NOT NULL,
                body_template TEXT NOT NULL,
                severity INTEGER NOT NULL DEFAULT 9,
                include_source_events INTEGER NOT NULL DEFAULT 1,
                cooldown_sec INTEGER NOT NULL DEFAULT 60,
                last_fired_ns INTEGER,
                chain_depth INTEGER NOT NULL DEFAULT 0,
                max_chain_depth INTEGER NOT NULL DEFAULT 3,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Extend saved_queries with structured filter support
            ALTER TABLE saved_queries ADD COLUMN filter_json TEXT;
            ALTER TABLE saved_queries ADD COLUMN view_config_json TEXT;

            PRAGMA user_version = 34;"
        )?;
        tracing::info!("Database migrated to v34 (SIEM event log, sessions, alerts, custom events)");
    }

    if version < 35 {
        conn.execute_batch(
            "ALTER TABLE activity_baselines ADD COLUMN profile_name TEXT NOT NULL DEFAULT 'default';
            PRAGMA user_version = 35;"
        )?;
        tracing::info!("Database migrated to v35 (baseline profile names)");
    }

    // Seed P25 talkgroups and sites from RadioReference data
    seed_talkgroups_from_p25_db(conn, data_dir)?;
    seed_sites_from_p25_db(conn, data_dir)?;
    seed_p25_channels(conn, data_dir)?;

    Ok(())
}

fn seed_channels_from_freq_db(conn: &Connection, data_dir: &Path) -> Result<(), rusqlite::Error> {
    // Only seed if no freq_db channels exist yet
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM channels WHERE source = 'freq_db'", [], |r| r.get(0),
    )?;
    if count > 0 {
        return Ok(());
    }

    // Read and parse portland-frequencies.json
    let json_path = data_dir.join("portland-frequencies.json");
    let json_str = match std::fs::read_to_string(&json_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Could not read portland-frequencies.json for channel seeding: {}", e);
            return Ok(());
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not parse portland-frequencies.json: {}", e);
            return Ok(());
        }
    };

    let Some(conventional) = json.get("conventional").and_then(|c| c.as_array()) else {
        return Ok(());
    };

    let mut inserted = 0u32;
    for entry in conventional {
        let freq = entry.get("freq").and_then(|f| f.as_f64()).unwrap_or(0.0);
        let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let cls = entry.get("cls").and_then(|c| c.as_str()).unwrap_or("UNK");
        let band = entry.get("band").and_then(|b| b.as_str()).unwrap_or("");
        let mode = entry.get("mode").and_then(|m| m.as_str()).unwrap_or("NFM");
        let tag = entry.get("tag").and_then(|t| t.as_str()).unwrap_or("");
        if freq == 0.0 { continue; }

        conn.execute(
            "INSERT INTO channels (channel_type, freq_mhz, label, cls, band, mode, tag, source)
             VALUES ('analog', ?1, ?2, ?3, ?4, ?5, ?6, 'freq_db')
             ON CONFLICT (channel_type, freq_mhz, tgid, timeslot) DO NOTHING",
            rusqlite::params![freq, name, cls, band, mode, tag],
        )?;
        inserted += 1;
    }
    tracing::info!("Seeded {} channels from portland-frequencies.json", inserted);
    Ok(())
}

/// Re-seed channels from updated portland-frequencies.json.
/// Unlike initial seed, this always runs — ON CONFLICT DO NOTHING handles dedup.
fn reseed_channels_from_freq_db(conn: &Connection, data_dir: &Path) -> Result<(), rusqlite::Error> {
    let json_path = data_dir.join("portland-frequencies.json");
    let json_str = match std::fs::read_to_string(&json_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Could not read portland-frequencies.json for channel reseeding: {}", e);
            return Ok(());
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not parse portland-frequencies.json: {}", e);
            return Ok(());
        }
    };

    let Some(conventional) = json.get("conventional").and_then(|c| c.as_array()) else {
        return Ok(());
    };

    let mut inserted = 0u32;
    for entry in conventional {
        let freq = entry.get("freq").and_then(|f| f.as_f64()).unwrap_or(0.0);
        let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let cls = entry.get("cls").and_then(|c| c.as_str()).unwrap_or("UNK");
        let band = entry.get("band").and_then(|b| b.as_str()).unwrap_or("");
        let mode = entry.get("mode").and_then(|m| m.as_str()).unwrap_or("NFM");
        let tag = entry.get("tag").and_then(|t| t.as_str()).unwrap_or("");
        if freq == 0.0 { continue; }

        let changes = conn.execute(
            "INSERT INTO channels (channel_type, freq_mhz, label, cls, band, mode, tag, source)
             VALUES ('analog', ?1, ?2, ?3, ?4, ?5, ?6, 'freq_db')
             ON CONFLICT (channel_type, freq_mhz, tgid, timeslot) DO NOTHING",
            rusqlite::params![freq, name, cls, band, mode, tag],
        )?;
        if changes > 0 { inserted += 1; }
    }
    tracing::info!("Reseeded channels: {} new entries from portland-frequencies.json", inserted);
    Ok(())
}

fn migrate_custom_channels(conn: &Connection) -> Result<(), rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM custom_channels", [], |r| r.get(0),
    )?;
    if count == 0 {
        return Ok(());
    }

    conn.execute_batch(
        "INSERT INTO channels (channel_type, freq_mhz, label, cls, band, mode, notes, source, created_at)
         SELECT 'analog', freq, name, cls, band, mode, notes, 'custom', created_at
         FROM custom_channels
         ON CONFLICT (channel_type, freq_mhz, tgid, timeslot) DO NOTHING;"
    )?;
    tracing::info!("Migrated {} custom_channels to channels table", count);
    Ok(())
}

pub fn seed_default_packages(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Only seed if no packages exist yet
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM scan_packages", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }

    // Package definitions: (name, description, items: [(name, freq_mhz)])
    let packages: &[(&str, &str, &[(&str, f64)])] = &[
        ("Law Enforcement", "Portland metro police/sheriff", &[
            ("PPB Dispatch", 155.010),
            ("PPB Tactical 2", 155.370),
            ("MCSO VHF", 155.250),
            ("OSP", 148.325),
            ("MCSO UHF", 460.525),
            ("OWIN Control", 769.500),
            ("OWIN Voice 1", 770.250),
            ("PPB Encrypted", 772.500),
        ]),
        ("Fire / EMS", "Fire and emergency medical services", &[
            ("PF&R VHF", 154.430),
            ("PF&R UHF Tac", 453.450),
            ("Fire P25", 773.250),
            ("EMS P25", 774.000),
        ]),
        ("Amateur Radio", "Ham radio frequencies", &[
            ("2m Calling", 146.520),
            ("K7RPT Rptr", 147.040),
            ("70cm Calling", 446.000),
            ("W7LT Rptr", 442.500),
            ("40m SSB", 7.200),
            ("20m Emergency", 14.300),
        ]),
        ("Weather / Marine", "NOAA weather and marine channels", &[
            ("NOAA WX1", 162.400),
            ("WWV 10MHz", 10.000),
            ("WWV 15MHz", 15.000),
            ("Marine Ch16", 156.800),
        ]),
        ("GMRS / FRS", "General mobile and family radio", &[
            ("GMRS 1", 462.5625),
            ("GMRS 3", 462.6125),
            ("GMRS 5", 462.6625),
            ("GMRS 7", 462.7125),
            ("GMRS R1", 467.5625),
            ("FRS 1", 462.5625),
        ]),
        ("Transit / Commercial", "Public transit and commercial", &[
            ("TriMet", 453.900),
        ]),
    ];

    for (name, desc, items) in packages {
        conn.execute(
            "INSERT INTO scan_packages (name, description) VALUES (?1, ?2)",
            rusqlite::params![name, desc],
        )?;
        let pkg_id = conn.last_insert_rowid();
        for (i, (item_name, freq)) in items.iter().enumerate() {
            conn.execute(
                "INSERT INTO scan_package_items (package_id, target_type, target_index, target_name, freq_mhz) VALUES (?1, 'FREQ', ?2, ?3, ?4)",
                rusqlite::params![pkg_id, i as i64, item_name, freq],
            )?;
        }
    }

    tracing::info!("Seeded {} default scan packages", packages.len());
    Ok(())
}

fn seed_sites_from_p25_db(conn: &Connection, data_dir: &Path) -> Result<(), rusqlite::Error> {
    // Only seed if no named sites exist yet
    let named_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM network_sites WHERE name IS NOT NULL",
        [], |r| r.get(0),
    )?;
    if named_count > 0 {
        return Ok(());
    }

    let json_path = data_dir.join("portland-p25-talkgroups.json");
    let json_str = match std::fs::read_to_string(&json_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Could not read portland-p25-talkgroups.json for site seeding: {}", e);
            return Ok(());
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not parse portland-p25-talkgroups.json: {}", e);
            return Ok(());
        }
    };

    let system = json.get("system").and_then(|s| s.as_str()).unwrap_or("Portland");
    // WACN 0xBEE00 = 781056, system_id 0x3CC = 972
    let wacn: i64 = json.get("wacn").and_then(|w| w.as_str())
        .and_then(|s| i64::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok())
        .unwrap_or(0xBEE00);
    let system_id: i64 = json.get("system_id").and_then(|s| s.as_str())
        .and_then(|s| i64::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok())
        .unwrap_or(0x3CC);

    let Some(sites) = json.get("sites").and_then(|s| s.as_array()) else {
        return Ok(());
    };

    let mut inserted = 0u32;
    for site in sites {
        let rfss = site.get("rfss").and_then(|r| r.as_i64()).unwrap_or(0);
        let site_num = site.get("site").and_then(|s| s.as_i64()).unwrap_or(0);
        let name = site.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let ccs = site.get("control_channels").and_then(|c| c.as_array());
        if site_num == 0 { continue; }

        let control_channel = ccs.and_then(|arr| arr.first()).and_then(|v| v.as_f64());

        // Build alt_control JSON from remaining CCs (skip the first/primary)
        let alt_control = ccs.map(|arr| {
            let alts: Vec<serde_json::Value> = arr.iter().skip(1)
                .filter_map(|v| v.as_f64())
                .map(|f| serde_json::json!({ "freq_mhz": f }))
                .collect();
            serde_json::to_string(&alts).unwrap_or_default()
        });

        let alt_ref = alt_control.as_deref();

        conn.execute(
            "INSERT INTO network_sites (system, wacn, system_id, rfss_id, site_id, name, control_channel, alt_control, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'), datetime('now'))
             ON CONFLICT DO NOTHING",
            rusqlite::params![system, wacn, system_id, rfss, site_num, name, control_channel, alt_ref],
        )?;
        inserted += 1;
    }
    tracing::info!("Seeded {} P25 sites from portland-p25-talkgroups.json", inserted);
    Ok(())
}

fn seed_talkgroups_from_p25_db(conn: &Connection, data_dir: &Path) -> Result<(), rusqlite::Error> {
    // Always run — ON CONFLICT DO UPDATE backfills names on live-discovered TGs
    let json_path = data_dir.join("portland-p25-talkgroups.json");
    let json_str = match std::fs::read_to_string(&json_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Could not read portland-p25-talkgroups.json for TG seeding: {}", e);
            return Ok(());
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not parse portland-p25-talkgroups.json: {}", e);
            return Ok(());
        }
    };

    let system = json.get("system").and_then(|s| s.as_str()).unwrap_or("Portland");
    let Some(talkgroups) = json.get("talkgroups").and_then(|t| t.as_array()) else {
        return Ok(());
    };

    let mut inserted = 0u32;
    for tg in talkgroups {
        let tgid = tg.get("tgid").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
        let alpha_tag = tg.get("alpha_tag").and_then(|a| a.as_str()).unwrap_or("");
        let department = tg.get("department").and_then(|d| d.as_str()).unwrap_or("");
        let tag = tg.get("tag").and_then(|t| t.as_str()).unwrap_or("");
        let mode = tg.get("mode").and_then(|m| m.as_str()).unwrap_or("D");
        if tgid == 0 { continue; }

        let encrypted = match mode {
            "DE" | "E" => "encrypted",
            _ => "clear",
        };

        let changes = conn.execute(
            "INSERT INTO network_talkgroups (system, tgid, name, department, tag, encrypted, total_grants, unique_uids, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, datetime('now'), datetime('now'))
             ON CONFLICT(system, tgid) DO UPDATE SET
               name = COALESCE(excluded.name, name),
               department = COALESCE(excluded.department, department),
               tag = COALESCE(excluded.tag, tag)",
            rusqlite::params![system, tgid, alpha_tag, department, tag, encrypted],
        )?;
        if changes > 0 { inserted += 1; }
    }
    tracing::info!("Seeded {} P25 talkgroups from portland-p25-talkgroups.json", inserted);
    Ok(())
}

/// Seed channels table with P25 talkgroup entries from RadioReference data.
/// This enables lookup_channel_by_tgid() for signal enrichment.
fn seed_p25_channels(conn: &Connection, data_dir: &Path) -> Result<(), rusqlite::Error> {
    // Always run — ON CONFLICT updates labels on existing entries
    let json_path = data_dir.join("portland-p25-talkgroups.json");
    let json_str = match std::fs::read_to_string(&json_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Could not read portland-p25-talkgroups.json for channel seeding: {}", e);
            return Ok(());
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not parse portland-p25-talkgroups.json: {}", e);
            return Ok(());
        }
    };

    let Some(talkgroups) = json.get("talkgroups").and_then(|t| t.as_array()) else {
        return Ok(());
    };

    let mut inserted = 0u32;
    for tg in talkgroups {
        let tgid = tg.get("tgid").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
        let alpha_tag = tg.get("alpha_tag").and_then(|a| a.as_str()).unwrap_or("");
        let department = tg.get("department").and_then(|d| d.as_str()).unwrap_or("");
        let tag = tg.get("tag").and_then(|t| t.as_str()).unwrap_or("");
        let mode_str = tg.get("mode").and_then(|m| m.as_str()).unwrap_or("D");
        if tgid == 0 { continue; }

        // Map department → signal classification
        let cls = classify_p25_department(department, tag);
        let encrypted = matches!(mode_str, "DE" | "E");

        let changes = conn.execute(
            "INSERT INTO channels (channel_type, tgid, label, cls, band, mode, tag, source, encryption_seen)
             VALUES ('p25', ?1, ?2, ?3, '700/800', 'P25', ?4, 'radioreference', ?5)
             ON CONFLICT(channel_type, freq_mhz, tgid, timeslot) DO UPDATE SET
               label = excluded.label,
               cls = excluded.cls,
               tag = excluded.tag",
            rusqlite::params![tgid, alpha_tag, cls, tag, encrypted as i32],
        )?;
        if changes > 0 { inserted += 1; }
    }
    tracing::info!("Seeded {} P25 channels from portland-p25-talkgroups.json", inserted);
    Ok(())
}

/// Map P25 department/tag strings to RF-LOG signal classification codes.
fn classify_p25_department(department: &str, tag: &str) -> &'static str {
    let dept_lower = department.to_lowercase();
    let tag_lower = tag.to_lowercase();

    if tag_lower.contains("law") || tag_lower.contains("corrections") {
        "PUBS"
    } else if tag_lower.contains("fire") || tag_lower.contains("ems") || tag_lower.contains("emergency") {
        "PUBS"
    } else if tag_lower.contains("interop") {
        "PUBS"
    } else if dept_lower.contains("police") || dept_lower.contains("sheriff") {
        "PUBS"
    } else if dept_lower.contains("fire") || dept_lower.contains("ems") || dept_lower.contains("rescue") {
        "PUBS"
    } else if dept_lower.contains("trimet") || dept_lower.contains("transit") {
        "COMM"
    } else if dept_lower.contains("hospital") || dept_lower.contains("medical") {
        "COMM"
    } else if dept_lower.contains("federal") {
        "FEDL"
    } else if dept_lower.contains("port of portland") {
        "PUBS"
    } else if dept_lower.contains("water") || dept_lower.contains("transportation") || dept_lower.contains("public works") {
        "COMM"
    } else {
        "COMM"
    }
}
