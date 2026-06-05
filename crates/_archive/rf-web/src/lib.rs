#![recursion_limit = "256"]

mod state;

pub use state::{AlertHighlight, AppState, AppConfig, ChannelParams, GpsPosition, ReceiverCoords, RecorderStatusData, SdrSlotStatus, VoiceSlotState};
use std::io::BufRead;

pub async fn heartbeat_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if state.is_shutdown() { break; }
        let msg = state.status_message();
        let _ = state.broadcast_heartbeat(msg);
    }
    tracing::info!("Heartbeat loop exited (shutdown)");
}

/// Portland simulation route waypoints: (lat, lon, alt_m, description)
const SIM_ROUTE: &[(f64, f64, f64)] = &[
    (45.5172, -122.6766, 15.0),  // Downtown collection site
    (45.5195, -122.6755, 18.0),  // Pioneer Square area
    (45.5275, -122.6753, 22.0),  // North on Broadway
    (45.5355, -122.6628, 12.0),  // Broadway Bridge approach
    (45.5358, -122.6495, 10.0),  // NE Portland via I-84
    (45.5389, -122.6210, 45.0),  // Continue E on I-84
    (45.5490, -122.5730, 65.0),  // Gateway area
    (45.5389, -122.6210, 45.0),  // Return west
    (45.5358, -122.6495, 10.0),  // I-84 westbound
    (45.5275, -122.6753, 22.0),  // Back to Broadway
    (45.5195, -122.6755, 18.0),  // Pioneer area
    (45.5172, -122.6766, 15.0),  // Back to collection site
];

/// Spawn GPS simulation task — updates GPS position on AppState at 1 Hz.
/// Operator position starts at downtown Portland and slowly moves along a route.
pub async fn gps_simulation_loop(state: AppState) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut tick: u64 = 0;

        // Interpolation: spend ~30 ticks between each waypoint
        let ticks_per_segment: u64 = 30;
        let total_ticks = SIM_ROUTE.len() as u64 * ticks_per_segment;

        // Small random-ish perturbation using simple deterministic sequence
        let jitter = |t: u64, scale: f64| -> f64 {
            let x = ((t.wrapping_mul(2654435761) >> 16) & 0xFFFF) as f64 / 65535.0;
            (x - 0.5) * scale
        };

        loop {
            interval.tick().await;
            if state.is_shutdown() { break; }

            let config = state.config();
            if !config.gps_enabled || config.gps_source != "simulation" {
                tick += 1;
                continue;
            }

            // Position along route with interpolation
            let route_tick = tick % total_ticks;
            let segment = (route_tick / ticks_per_segment) as usize;
            let frac = (route_tick % ticks_per_segment) as f64 / ticks_per_segment as f64;
            let next = (segment + 1) % SIM_ROUTE.len();

            let (lat0, lon0, alt0) = SIM_ROUTE[segment];
            let (lat1, lon1, alt1) = SIM_ROUTE[next];
            let lat = lat0 + (lat1 - lat0) * frac + jitter(tick, 0.00002);
            let lon = lon0 + (lon1 - lon0) * frac + jitter(tick.wrapping_add(1000), 0.00002);
            let alt = alt0 + (alt1 - alt0) * frac;

            // Heading: approximate from segment direction
            let dlat = lat1 - lat0;
            let dlon = lon1 - lon0;
            let heading = (dlon.atan2(dlat).to_degrees() + 360.0) % 360.0;

            // Speed: ~0 when stationary (first/last waypoints), ~15 m/s otherwise
            let dist = ((dlat * dlat + dlon * dlon) as f64).sqrt() * 111_000.0; // rough meters
            let speed = dist / ticks_per_segment as f64;

            // GPS quality simulation: occasionally degrade
            let degrade = (tick % 47) == 0; // ~2% degradation
            let (accuracy, hdop, sats, fix) = if degrade {
                (25.0 + jitter(tick, 30.0).abs(), 3.5, 5u8, "2d")
            } else {
                (3.0 + jitter(tick, 4.0).abs(), 0.9 + jitter(tick, 0.4).abs(), 11 + (jitter(tick, 4.0).abs() as u8), "3d")
            };

            let pos = GpsPosition {
                latitude: lat,
                longitude: lon,
                altitude_m: Some(alt),
                heading_deg: Some(heading),
                speed_mps: Some(speed),
                accuracy_m: accuracy,
                hdop: Some(hdop),
                fix_type: fix.to_string(),
                satellite_count: sats,
                source: "simulation".to_string(),
            };

            state.set_gps_position(pos);
            tick += 1;
        }
}

/// Haversine distance in meters between two lat/lon points.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0; // Earth radius in meters
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    R * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

/// 1 Hz geofence loop — checks operator GPS position against collection sites.
/// Opens/closes site_sessions as operator enters/leaves sites.
///
/// Uses hysteresis to prevent flickering: enter at geofence_radius_m,
/// but require moving to 2x the radius before considering the operator as having left.
/// This is critical for GPS simulation mode where position wanders continuously.
const GEOFENCE_EXIT_MULTIPLIER: f64 = 2.0;

pub async fn geofence_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if state.is_shutdown() { break; }

        let gps = match state.gps_position() {
            Some(p) if p.fix_type != "none" => p,
            _ => continue,
        };

        let config = state.config();
        let sites = match state.db().list_intel_sites(500) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // If we're currently at a site, check if we've left (with hysteresis)
        if let Some(current_id) = config.active_site_id {
            if let Some(current_site) = sites.iter().find(|s| s.id == current_id) {
                if let (Some(lat), Some(lon)) = (current_site.latitude, current_site.longitude) {
                    let dist = haversine_m(gps.latitude, gps.longitude, lat, lon);
                    let exit_radius = current_site.geofence_radius_m * GEOFENCE_EXIT_MULTIPLIER;
                    if dist <= exit_radius {
                        // Still within exit radius — stay on site
                        continue;
                    }
                    // Exceeded exit radius — leave site
                    if let Some(session_id) = config.active_site_session_id {
                        let _ = state.db().close_site_session(session_id);
                        tracing::info!("Geofence: left site {} at {:.0}m (exit radius {:.0}m, session {} closed)",
                            current_id, dist, exit_radius, session_id);
                    }
                    state.update_config(|c| {
                        c.active_site_id = None;
                        c.active_site_session_id = None;
                    });
                }
            }
            // If site was deleted while active, clear state
            if !sites.iter().any(|s| s.id == current_id) {
                if let Some(session_id) = config.active_site_session_id {
                    let _ = state.db().close_site_session(session_id);
                }
                state.update_config(|c| {
                    c.active_site_id = None;
                    c.active_site_session_id = None;
                });
            }
            continue;
        }

        // Not at any site — check if we've entered one (at normal radius)
        let mut best: Option<(i64, f64)> = None;
        for site in &sites {
            let (lat, lon) = match (site.latitude, site.longitude) {
                (Some(lat), Some(lon)) => (lat, lon),
                _ => continue,
            };
            let dist = haversine_m(gps.latitude, gps.longitude, lat, lon);
            if dist <= site.geofence_radius_m {
                if best.is_none() || dist < best.unwrap().1 {
                    best = Some((site.id, dist));
                }
            }
        }

        if let Some((site_id, dist)) = best {
            let new_session_id = match state.db().open_site_session(site_id, Some(gps.latitude), Some(gps.longitude)) {
                Ok(id) => {
                    tracing::info!("Geofence: entered site {} at {:.0}m (session {})", site_id, dist, id);
                    Some(id)
                }
                Err(e) => {
                    tracing::warn!("Geofence: failed to open session: {}", e);
                    None
                }
            };
            state.update_config(|c| {
                c.active_site_id = Some(site_id);
                c.active_site_session_id = new_session_id;
            });
        }
    }
    tracing::info!("Geofence loop exited (shutdown)");
}

/// Spawn GPS fixed-position task — emits a static position at 1 Hz when source="fixed".
pub async fn gps_fixed_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if state.is_shutdown() { break; }

        let config = state.config();
        if !config.gps_enabled || config.gps_source != "fixed" {
            continue;
        }

        let (lat, lon) = match (config.fixed_lat, config.fixed_lon) {
            (Some(lat), Some(lon)) => (lat, lon),
            _ => continue,
        };

        let pos = GpsPosition {
            latitude: lat,
            longitude: lon,
            altitude_m: config.fixed_alt_m,
            heading_deg: None,
            speed_mps: Some(0.0),
            accuracy_m: 0.0,
            hdop: None,
            fix_type: "3d".to_string(),
            satellite_count: 0,
            source: "fixed".to_string(),
        };
        state.set_gps_position(pos);
    }
}

// --- NMEA parsing for external GPS ---

/// Convert NMEA latitude (DDMM.MMMM) + hemisphere to decimal degrees.
fn parse_nmea_lat(field: &str, hem: &str) -> Option<f64> {
    if field.len() < 4 { return None; }
    let deg: f64 = field[..2].parse().ok()?;
    let min: f64 = field[2..].parse().ok()?;
    let val = deg + min / 60.0;
    Some(if hem == "S" { -val } else { val })
}

/// Convert NMEA longitude (DDDMM.MMMM) + hemisphere to decimal degrees.
fn parse_nmea_lon(field: &str, hem: &str) -> Option<f64> {
    if field.len() < 5 { return None; }
    let deg: f64 = field[..3].parse().ok()?;
    let min: f64 = field[3..].parse().ok()?;
    let val = deg + min / 60.0;
    Some(if hem == "W" { -val } else { val })
}

/// Verify NMEA XOR checksum: everything between $ and * must XOR to the hex value after *.
fn verify_nmea_checksum(line: &str) -> bool {
    let Some(start) = line.find('$') else { return false };
    let Some(star) = line.find('*') else { return false };
    if star <= start + 1 || star + 3 > line.len() { return false; }
    let body = &line[start + 1..star];
    let expected = u8::from_str_radix(&line[star + 1..star + 3], 16).unwrap_or(0);
    let computed = body.bytes().fold(0u8, |acc, b| acc ^ b);
    computed == expected
}

/// Accumulates GGA + RMC fields to produce a complete GpsPosition.
#[derive(Default)]
struct NmeaState {
    lat: Option<f64>,
    lon: Option<f64>,
    alt_m: Option<f64>,
    fix_quality: u8,  // GGA fix: 0=invalid, 1=GPS, 2=DGPS
    sats: u8,
    hdop: Option<f64>,
    speed_mps: Option<f64>,
    heading_deg: Option<f64>,
}

impl NmeaState {
    /// Parse $GPGGA / $GNGGA sentence.
    fn parse_gga(&mut self, fields: &[&str]) {
        if fields.len() < 10 { return; }
        self.lat = parse_nmea_lat(fields[2], fields[3]);
        self.lon = parse_nmea_lon(fields[4], fields[5]);
        self.fix_quality = fields[6].parse().unwrap_or(0);
        self.sats = fields[7].parse().unwrap_or(0);
        self.hdop = fields[8].parse().ok();
        self.alt_m = fields[9].parse().ok();
    }

    /// Parse $GPRMC / $GNRMC sentence.
    fn parse_rmc(&mut self, fields: &[&str]) {
        if fields.len() < 8 { return; }
        // RMC status: A=active, V=void
        if fields[2] != "A" { return; }
        self.lat = parse_nmea_lat(fields[3], fields[4]);
        self.lon = parse_nmea_lon(fields[5], fields[6]);
        // Speed in knots → m/s
        if let Ok(knots) = fields[7].parse::<f64>() {
            self.speed_mps = Some(knots * 0.514444);
        }
        // Heading (track angle)
        if fields.len() > 8 && !fields[8].is_empty() {
            self.heading_deg = fields[8].parse().ok();
        }
    }

    /// Convert accumulated state to GpsPosition if we have a valid fix.
    fn to_gps_position(&self) -> Option<GpsPosition> {
        let lat = self.lat?;
        let lon = self.lon?;
        if self.fix_quality == 0 { return None; }
        let fix_type = match self.fix_quality {
            2 => "dgps",
            1 => if self.alt_m.is_some() { "3d" } else { "2d" },
            _ => "none",
        };
        let accuracy = self.hdop.unwrap_or(99.0) * 2.5; // CEP estimate
        Some(GpsPosition {
            latitude: lat,
            longitude: lon,
            altitude_m: self.alt_m,
            heading_deg: self.heading_deg,
            speed_mps: self.speed_mps,
            accuracy_m: accuracy,
            hdop: self.hdop,
            fix_type: fix_type.to_string(),
            satellite_count: self.sats,
            source: "external".to_string(),
        })
    }
}

/// One-shot GPS auto-detection at startup.
/// Scans all available serial ports for NMEA sentences. If a GNSS device is found,
/// sets gps_source to "external" and gps_port to the detected port. Otherwise stays "none".
pub async fn gps_auto_detect(state: AppState) {
    let config = state.config();
    if !config.gps_enabled {
        return;
    }
    // Only auto-detect when source is "external" but no port is configured yet
    if config.gps_source != "external" || !config.gps_port.is_empty() {
        return;
    }

    tracing::info!("GPS: source is 'external' with no port — probing for GNSS device...");
    let baud = config.gps_baud;

    let result = tokio::task::spawn_blocking(move || {
        let ports = serialport::available_ports().unwrap_or_default();
        if ports.is_empty() {
            tracing::info!("GPS: no serial ports found");
            return None;
        }

        tracing::info!("GPS: probing {} serial port(s) for GNSS...", ports.len());

        for port_info in &ports {
            let port_name = &port_info.port_name;
            tracing::info!("GPS: probing {}...", port_name);

            let port = match serialport::new(port_name, baud)
                .timeout(std::time::Duration::from_secs(2))
                .open()
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("GPS: {} open failed: {}", port_name, e);
                    continue;
                }
            };

            let reader = std::io::BufReader::new(port);
            let mut found = false;
            for line_result in reader.lines().take(15) {
                match line_result {
                    Ok(line) if line.contains("$G") => { found = true; break; }
                    Ok(_) => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(_) => break,
                }
            }
            // Port is dropped here, releasing the handle

            if found {
                tracing::info!("GPS: GNSS device detected on {}", port_name);
                return Some(port_name.clone());
            }
        }

        tracing::info!("GPS: no GNSS device found on any port");
        None
    }).await;

    match result {
        Ok(Some(port_name)) => {
            tracing::info!("GPS: auto-detected GNSS on {} — keeping source 'external'", port_name);
            state.update_config(|c| {
                c.gps_port = port_name;
            });
        }
        Ok(None) => {
            tracing::info!("GPS: no GNSS device found — downgrading source to 'none'");
            state.update_config(|c| {
                c.gps_source = "none".into();
            });
        }
        Err(e) => {
            tracing::warn!("GPS: auto-detect task panicked: {}", e);
        }
    }
}

/// Spawn GPS serial reader — connects to external GNSS receiver (e.g. VK-162 u-blox 7).
/// Reads NMEA sentences over serial, parses GGA/RMC, updates AppState GPS position.
/// Only active when `config.gps_source == "external"`. Retries on port errors.
pub async fn gps_serial_loop(state: AppState) {
    loop {
        if state.is_shutdown() { break; }
        let config = state.config();
        if !config.gps_enabled || config.gps_source != "external" || config.gps_port.is_empty() {
            tracing::trace!("GPS serial: waiting (enabled={}, source={}, port='{}')",
                config.gps_enabled, config.gps_source, config.gps_port);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }

        tracing::info!("GPS serial: opening {} at {} baud", config.gps_port, config.gps_baud);
        let port_name = config.gps_port.clone();
        let baud = config.gps_baud;
        let state2 = state.clone();

        // Run blocking serial I/O on a dedicated thread
        let result = tokio::task::spawn_blocking(move || {
            run_gps_serial_loop(&state2, &port_name, baud)
        }).await;

        match result {
            Ok(Ok(())) => tracing::info!("GPS: serial reader exited cleanly"),
            Ok(Err(e)) => tracing::warn!("GPS: serial error on {}: {} — retrying in 3s", config.gps_port, e),
            Err(e) => tracing::error!("GPS: serial task panicked: {} — retrying in 3s", e),
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

/// Blocking serial read loop. Returns when port disconnects or config changes.
fn run_gps_serial_loop(state: &AppState, port_name: &str, baud: u32) -> Result<(), String> {
    let port = serialport::new(port_name, baud)
        .timeout(std::time::Duration::from_secs(2))
        .open()
        .map_err(|e| format!("{}", e))?;

    tracing::info!("GPS: connected to {} at {} baud", port_name, baud);

    let reader = std::io::BufReader::new(port);
    let mut nmea = NmeaState::default();

    for line_result in reader.lines() {
        // Check if config changed away from external
        let config = state.config();
        if !config.gps_enabled || config.gps_source != "external" {
            tracing::info!("GPS: source changed away from external, closing port");
            return Ok(());
        }

        let line = match line_result {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(format!("read error: {}", e)),
        };

        if !verify_nmea_checksum(&line) {
            tracing::trace!("GPS: bad checksum: {}", line);
            continue;
        }

        let fields: Vec<&str> = line.trim_end().split(',').collect();
        if fields.is_empty() { continue; }

        // Strip talker ID prefix — match sentence type
        let sentence = fields[0].get(3..).unwrap_or("");
        match sentence {
            "GGA" => {
                nmea.parse_gga(&fields);
                tracing::trace!("GPS: GGA fix_quality={} sats={} lat={:?} lon={:?}",
                    nmea.fix_quality, nmea.sats, nmea.lat, nmea.lon);
            }
            "RMC" => {
                nmea.parse_rmc(&fields);
                // RMC completes a position cycle — emit position
                match nmea.to_gps_position() {
                    Some(pos) => {
                        tracing::trace!("GPS: position {:.6},{:.6} fix={} sats={}",
                            pos.latitude, pos.longitude, pos.fix_type, pos.satellite_count);
                        state.set_gps_position(pos);
                    }
                    None => {
                        tracing::trace!("GPS: RMC — no valid position (fix_quality={})", nmea.fix_quality);
                    }
                }
            }
            _ => {}
        }
    }

    Err("serial port closed".to_string())
}
