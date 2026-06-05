use std::collections::VecDeque;

use eframe::egui;

use rf_events::{AlertFiring, AlertRule, CustomEventRule, LogRecord, EventQuery};

use crate::bridge::UiBridge;
use crate::state::{UiState, WatchdogTab};
use crate::theme::*;
use crate::widgets::tab_bar;

// ── WatchdogState ────────────────────────────────────────────

/// Poll interval for DB queries (seconds).
const DB_POLL_INTERVAL: f64 = 2.0;

/// Max live-tail entries to keep in memory.
const LIVE_TAIL_MAX: usize = 200;

/// Mutable per-frame state for the WATCHDOG view.
/// Cached DB results refresh every `DB_POLL_INTERVAL` seconds.
pub struct WatchdogState {
    // ── DB cache ──
    /// Alert rules (from alert_rules table)
    pub alert_rules: Vec<AlertRule>,
    /// Custom event rules (from custom_event_rules table)
    pub custom_event_rules: Vec<CustomEventRule>,
    /// Recent alert firings (from alert_firings table)
    pub alert_firings: Vec<AlertFiring>,
    /// Recent events from DB query (Events tab)
    pub query_results: Vec<LogRecord>,
    /// Total count for current query (pagination)
    pub query_total: u64,
    /// Current query page offset
    pub query_offset: usize,
    /// Facet counts: (field_name, vec of (value, count))
    pub facets: Vec<(String, Vec<(String, u64)>)>,

    // ── Live tail (streaming from EventBus) ──
    pub live_tail: VecDeque<LogRecord>,
    pub live_tail_paused: bool,
    /// Minimum severity for live tail display (filters below this)
    pub live_tail_min_severity: u8,
    /// Tactical filter: hide network-level housekeeping (grants, updates, affiliations, spectrum detect/lost)
    pub live_tail_tactical: bool,
    /// Rolling event rate (events received in last second)
    pub live_tail_rate: f32,
    /// Timestamp of last rate calculation
    live_tail_rate_ts: f64,
    /// Events counted in current rate window
    live_tail_rate_count: u32,
    /// Timestamp of last received event (for pulsing indicator)
    pub live_tail_last_event_time: f64,

    // ── Timing ──
    last_db_poll: f64,
    /// Force a refresh on next frame (e.g., after rule edit)
    pub force_refresh: bool,

    // ── Event filter state ──
    pub event_type_filter: String,
    pub event_severity_filter: String,
    pub event_band_filter: String,
    pub event_source_filter: String,
    /// Time range index: 0=1h, 1=6h, 2=24h, 3=7d
    pub event_time_range: usize,
    /// Expanded event detail (row index in query_results)
    pub event_expanded: Option<usize>,
    /// Whether query has been run at least once (auto-run on first visit)
    pub events_initialized: bool,

    // ── Alerts tab state ──
    /// Filter: 0=ALL, 1=NEW (unacked), 2=ACK (acknowledged)
    pub alerts_filter: u8,
    /// Expanded firing detail (firing id)
    pub alerts_expanded: Option<i64>,

    // ── Rules tab state ──
    /// Expanded alert rule editor (rule id)
    pub alert_rule_expanded: Option<i64>,
    /// Persistent editing copy of alert rule (survives across frames)
    pub editing_alert_rule: Option<AlertRule>,
    /// Expanded custom event rule editor (rule id)
    pub custom_rule_expanded: Option<i64>,
    /// Persistent editing copy of custom rule (survives across frames)
    pub editing_custom_rule: Option<CustomEventRule>,
    /// Pending delete confirmation (rule_id, is_alert_rule)
    pub rule_delete_confirm: Option<(i64, bool)>,

    // ── Dashboard metrics ──
    pub total_event_count: u64,
    pub active_alert_count: usize,
    pub unacked_firing_count: usize,

    // ── Threats tab cache (counter-surveillance) ──
    pub cs_new_emitters: Vec<rf_db::RadioFingerprint>,
    pub cs_anomaly_count: usize,
    pub cs_new_emitter_count: usize,
    pub cs_uid_mismatch_count: usize,
    pub cs_baseline_deviation_count: usize,
    pub cs_recent_threats: Vec<LogRecord>,
    /// Composite threat score: emitters*3 + anomalies*2 + uid*5 + baseline*1
    pub cs_threat_score: u32,

    // ── Sweep tab state ──
    /// Whether a sweep is currently active
    pub sweep_active: bool,
    /// Active sweep protocol name (e.g. "VEHICLE_QUICK")
    pub sweep_protocol: String,
    /// Sweep start time (egui time seconds)
    pub sweep_start_time: f64,
    /// Sweep total duration in seconds
    pub sweep_duration_sec: f64,

    // ── Baseline tab cache ──
    pub baselines: Vec<rf_db::ActivityBaseline>,
    pub anomaly_events: Vec<rf_db::AnomalyEvent>,
    /// Anomaly severity filter: 0=ALL, 1=warning+, 2=critical only
    pub anomaly_sev_filter: u8,
    pub baseline_count: i64,
    pub baseline_last_computed: Option<String>,
    pub baseline_computing: bool,
    pub baseline_profiles: Vec<String>,
    pub baseline_active_profile: String,
    pub baseline_new_profile_name: String,
    /// Feedback message after baseline capture (success or error)
    pub baseline_feedback: Option<(String, bool)>,  // (message, is_success)
    /// Timestamp when feedback was set (for auto-dismiss)
    pub baseline_feedback_time: f64,
}

impl Default for WatchdogState {
    fn default() -> Self {
        Self {
            alert_rules: Vec::new(),
            custom_event_rules: Vec::new(),
            alert_firings: Vec::new(),
            query_results: Vec::new(),
            query_total: 0,
            query_offset: 0,
            facets: Vec::new(),
            live_tail: VecDeque::new(),
            live_tail_paused: false,
            live_tail_min_severity: 0,
            live_tail_tactical: true,
            live_tail_rate: 0.0,
            live_tail_rate_ts: 0.0,
            live_tail_rate_count: 0,
            live_tail_last_event_time: 0.0,
            last_db_poll: 0.0,
            force_refresh: true,
            alerts_filter: 0,
            alerts_expanded: None,
            alert_rule_expanded: None,
            editing_alert_rule: None,
            custom_rule_expanded: None,
            editing_custom_rule: None,
            rule_delete_confirm: None,
            event_type_filter: String::new(),
            event_severity_filter: String::new(),
            event_band_filter: String::new(),
            event_source_filter: String::new(),
            event_time_range: 2, // default: 24h
            event_expanded: None,
            events_initialized: false,
            total_event_count: 0,
            active_alert_count: 0,
            unacked_firing_count: 0,
            cs_new_emitters: Vec::new(),
            cs_anomaly_count: 0,
            cs_new_emitter_count: 0,
            cs_uid_mismatch_count: 0,
            cs_baseline_deviation_count: 0,
            cs_recent_threats: Vec::new(),
            cs_threat_score: 0,
            sweep_active: false,
            sweep_protocol: String::new(),
            sweep_start_time: 0.0,
            sweep_duration_sec: 0.0,
            baselines: Vec::new(),
            anomaly_events: Vec::new(),
            anomaly_sev_filter: 0,
            baseline_count: 0,
            baseline_last_computed: None,
            baseline_computing: false,
            baseline_profiles: Vec::new(),
            baseline_active_profile: String::from("all"),
            baseline_new_profile_name: String::new(),
            baseline_feedback: None,
            baseline_feedback_time: 0.0,
        }
    }
}

impl WatchdogState {
    /// Add a LogRecord to the live tail (called from app update loop).
    pub fn push_live_event(&mut self, record: LogRecord, now: f64) {
        if self.live_tail_paused {
            return;
        }
        // Tactical filter: hide high-volume network housekeeping events.
        // These are still processed by the SIEM backend for intelligence analysis,
        // but don't need to clutter the operator's live display.
        if self.live_tail_tactical && is_network_noise(&record.event_type) {
            return;
        }
        self.live_tail_rate_count += 1;
        self.live_tail_last_event_time = now;
        self.live_tail.push_front(record);
        while self.live_tail.len() > LIVE_TAIL_MAX {
            self.live_tail.pop_back();
        }
    }

    /// Update event rate calculation. Call once per frame with current time.
    pub fn update_rate(&mut self, now: f64) {
        // Initialize on first call to avoid large elapsed on first frame
        if self.live_tail_rate_ts == 0.0 {
            self.live_tail_rate_ts = now;
            return;
        }
        let elapsed = now - self.live_tail_rate_ts;
        if elapsed >= 1.0 {
            self.live_tail_rate = self.live_tail_rate_count as f32 / elapsed as f32;
            self.live_tail_rate_count = 0;
            self.live_tail_rate_ts = now;
        }
    }

    /// Refresh cached data from DB if enough time has elapsed.
    #[allow(dead_code)] // Convenience wrapper; poll_db_tab used directly by show()
    pub fn poll_db(&mut self, db: &rf_db::Db, current_time: f64) {
        self.poll_db_tab(db, current_time, None);
    }

    pub fn poll_db_tab(&mut self, db: &rf_db::Db, current_time: f64, active_tab: Option<crate::state::WatchdogTab>) {
        if !self.force_refresh && (current_time - self.last_db_poll) < DB_POLL_INTERVAL {
            return;
        }
        if self.force_refresh {
            self.events_initialized = false;
        }
        self.force_refresh = false;
        self.last_db_poll = current_time;

        let tab = active_tab.unwrap_or(crate::state::WatchdogTab::Dashboard);

        // Dashboard metrics — lightweight, always load
        if let Ok(firings) = db.recent_alert_firings(100) {
            self.unacked_firing_count = firings.iter().filter(|f| !f.acknowledged).count();
            self.alert_firings = firings;
        }
        if let Ok(count) = db.event_count() {
            self.total_event_count = count;
        }

        // Tab-specific queries
        use crate::state::WatchdogTab;
        match tab {
            WatchdogTab::Dashboard => {
                if let Ok(rules) = db.list_alert_rules() {
                    self.active_alert_count = rules.iter().filter(|r| r.enabled).count();
                    self.alert_rules = rules;
                }
            }
            WatchdogTab::Threats => {
                use rf_events::{Filter, Field, FilterValue};
                if let Ok(fps) = db.list_fingerprints_typed(100) {
                    let cutoff = chrono::Utc::now()
                        .checked_sub_signed(chrono::Duration::hours(24))
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    self.cs_new_emitters = fps.iter()
                        .filter(|fp| fp.first_seen >= cutoff)
                        .cloned()
                        .collect();
                    self.cs_new_emitter_count = self.cs_new_emitters.len();
                }
                self.cs_anomaly_count = self.anomaly_events.len();
                self.cs_baseline_deviation_count = self.anomaly_events.iter()
                    .filter(|a| a.anomaly_score.unwrap_or(0.0) > 3.0)
                    .count();

                let cs_event_types = [
                    rf_events::event_types::SIGEX_EMITTER_NEW,
                    rf_events::event_types::SIGEX_EMITTER_RETURN,
                    rf_events::event_types::SIGEX_UID_MISMATCH,
                ];
                let now_ns = rf_events::event::now_ns();
                let range_start_ns = now_ns.saturating_sub(86_400 * 1_000_000_000);
                let mut cs_threats = Vec::new();
                for et in &cs_event_types {
                    let q = EventQuery {
                        filters: vec![
                            Filter::Eq(Field::EventType, FilterValue::String(et.to_string())),
                        ],
                        time_range: Some((range_start_ns, now_ns)),
                        limit: 50,
                        ..Default::default()
                    };
                    if let Ok(events) = db.query_events(&q) {
                        if *et == rf_events::event_types::SIGEX_UID_MISMATCH {
                            self.cs_uid_mismatch_count = events.len();
                        }
                        cs_threats.extend(events);
                    }
                }
                cs_threats.sort_by(|a, b| b.timestamp_ns.cmp(&a.timestamp_ns));
                cs_threats.truncate(100);
                self.cs_recent_threats = cs_threats;

                // Compute composite threat score: emitters*3 + anomalies*2 + uid*5 + baseline*1
                self.cs_threat_score =
                    (self.cs_new_emitter_count as u32).saturating_mul(3)
                    .saturating_add((self.cs_anomaly_count as u32).saturating_mul(2))
                    .saturating_add((self.cs_uid_mismatch_count as u32).saturating_mul(5))
                    .saturating_add(self.cs_baseline_deviation_count as u32);
            }
            WatchdogTab::Baseline => {
                if let Ok(baselines) = db.list_baselines(500) {
                    self.baselines = baselines;
                }
                if let Ok(anomalies) = db.list_anomaly_events(200) {
                    self.anomaly_events = anomalies;
                }
                if let Ok(count) = db.baseline_count() {
                    self.baseline_count = count;
                }
                if let Ok(last) = db.baseline_last_computed() {
                    self.baseline_last_computed = last;
                }
                if let Ok(profiles) = db.list_baseline_profiles() {
                    self.baseline_profiles = profiles;
                }
            }
            WatchdogTab::Events => {
                let now_ns = rf_events::event::now_ns();
                let range_ns = time_range_ns(self.event_time_range);
                let range_start = now_ns.saturating_sub(range_ns);
                let mut new_facets = Vec::new();
                for field in &["event_type", "severity", "band", "source"] {
                    if let Ok(facets) = db.event_facets(field, Some(range_start), Some(now_ns), 20) {
                        new_facets.push((field.to_string(), facets));
                    }
                }
                if !new_facets.is_empty() {
                    self.facets = new_facets;
                }
            }
            WatchdogTab::Alerts => {
                // Firings already loaded above
            }
            WatchdogTab::Rules => {
                if let Ok(rules) = db.list_alert_rules() {
                    self.alert_rules = rules;
                }
                if let Ok(rules) = db.list_custom_event_rules() {
                    self.custom_event_rules = rules;
                }
                self.active_alert_count = self.alert_rules.iter().filter(|r| r.enabled).count();
            }
            WatchdogTab::Sweep => {
                // Sweep tab has no periodic DB queries
            }
        }
    }

    /// Run the events tab query (called when filters change or page changes).
    pub fn run_event_query(&mut self, db: &rf_db::Db) {
        let now_ns = rf_events::event::now_ns();
        let range_ns = time_range_ns(self.event_time_range);
        let range_start = now_ns.saturating_sub(range_ns);

        let mut query = EventQuery::new()
            .time_range(range_start, now_ns)
            .limit(100)
            .offset(self.query_offset);

        // Apply type filter (supports contains for partial match)
        if !self.event_type_filter.is_empty() {
            if self.event_type_filter.contains('%') || self.event_type_filter.contains('*') {
                let pattern = self.event_type_filter.replace('*', "%");
                query = query.filter(rf_events::Filter::Like(
                    rf_events::Field::EventType,
                    pattern,
                ));
            } else {
                query = query.filter(rf_events::Filter::Eq(
                    rf_events::Field::EventType,
                    rf_events::FilterValue::String(self.event_type_filter.clone()),
                ));
            }
        }

        // Apply severity filter (exact match — consistent with facet counts)
        if !self.event_severity_filter.is_empty() {
            if let Ok(sev) = self.event_severity_filter.parse::<u8>() {
                query = query.filter(rf_events::Filter::Eq(
                    rf_events::Field::Severity,
                    rf_events::FilterValue::Int(sev as i64),
                ));
            }
        }

        // Apply band filter
        if !self.event_band_filter.is_empty() {
            query = query.filter(rf_events::Filter::Eq(
                rf_events::Field::Band,
                rf_events::FilterValue::String(self.event_band_filter.clone()),
            ));
        }

        // Apply source filter
        if !self.event_source_filter.is_empty() {
            if let Ok(src) = self.event_source_filter.parse::<u8>() {
                query = query.filter(rf_events::Filter::Eq(
                    rf_events::Field::Source,
                    rf_events::FilterValue::Int(src as i64),
                ));
            }
        }

        // Count total
        if let Ok(count) = db.count_events(&query) {
            self.query_total = count;
        }

        // Fetch page
        if let Ok(results) = db.query_events(&query) {
            self.query_results = results;
        }

        // Reset expanded detail since results changed
        self.event_expanded = None;
        self.events_initialized = true;
    }
}

// ── WATCHDOG View ────────────────────────────────────────────

/// WATCHDOG view — SIEM + counter-surveillance home
pub fn show(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    _bridge: &UiBridge,
    db: &rf_db::Db,
    ws: &mut WatchdogState,
) {
    // Poll DB for cached data — only query the active tab's data
    let t = ui.ctx().input(|i| i.time);
    ws.poll_db_tab(db, t, Some(ui_state.watchdog_tab));

    // Tab bar at top
    ui.vertical(|ui| {
        tab_bar::show(
            ui,
            WatchdogTab::ALL,
            &mut ui_state.watchdog_tab,
            |t| t.label(),
            RED_WATCHDOG,
        );
        ui.separator();

        // Calculate remaining rect for content + live tail
        let full_rect = ui.available_rect_before_wrap();
        let live_h = ui_state.watchdog_live_tail_h.clamp(60.0, (full_rect.height() - 100.0).max(60.0));

        // --- Horizontal drag handle to resize live tail ---
        let handle_y = full_rect.bottom() - live_h;
        let handle_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left(), handle_y - 2.0),
            egui::vec2(full_rect.width(), 5.0),
        );
        let handle_resp = ui.interact(handle_rect, egui::Id::new("watchdog_h_split"), egui::Sense::drag());
        if handle_resp.dragged() {
            ui_state.watchdog_live_tail_h = (full_rect.bottom() - handle_resp.interact_pointer_pos().unwrap().y)
                .clamp(60.0, (full_rect.height() - 100.0).max(60.0));
        }
        let handle_color = if handle_resp.hovered() || handle_resp.dragged() { BORDER_BRIGHT } else { BORDER };
        ui.painter().line_segment(
            [egui::pos2(full_rect.left(), handle_y), egui::pos2(full_rect.right(), handle_y)],
            egui::Stroke::new(if handle_resp.hovered() || handle_resp.dragged() { 2.0 } else { 1.0 }, handle_color),
        );
        if handle_resp.hovered() || handle_resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }

        // --- Main content area ---
        let content_area = egui::Rect::from_min_max(
            full_rect.left_top(),
            egui::pos2(full_rect.right(), handle_y - 2.0),
        );
        let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_area));
        content_ui.set_clip_rect(content_area);

        match ui_state.watchdog_tab {
            WatchdogTab::Dashboard => show_dashboard(&mut content_ui, ws),
            WatchdogTab::Threats => show_threats(&mut content_ui, ws),
            WatchdogTab::Events => show_events(&mut content_ui, ws, db, ui_state),
            WatchdogTab::Alerts => show_alerts(&mut content_ui, ws, db),
            WatchdogTab::Rules => show_rules(&mut content_ui, ws, db),
            WatchdogTab::Sweep => show_sweep(&mut content_ui, db, ws),
            WatchdogTab::Baseline => show_baseline(&mut content_ui, db, ws),
        }

        // --- Live tail (resizable bottom strip) ---
        let tail_rect = egui::Rect::from_min_max(
            egui::pos2(full_rect.left(), handle_y + 3.0),
            full_rect.right_bottom(),
        );
        let mut tail_ui = ui.new_child(egui::UiBuilder::new().max_rect(tail_rect));
        show_live_tail(&mut tail_ui, ws, ui_state);

        ui.allocate_rect(full_rect, egui::Sense::hover());
    });
}

// ── Dashboard Tab ────────────────────────────────────────────

fn show_dashboard(ui: &mut egui::Ui, ws: &WatchdogState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);

        // Metrics row
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            metric_card(ui, "TOTAL EVENTS", &ws.total_event_count.to_string(), TEXT_PRIMARY);
            metric_card(ui, "ACTIVE RULES", &ws.active_alert_count.to_string(), GREEN_COLLECT);
            metric_card(ui, "UNACKED ALERTS", &ws.unacked_firing_count.to_string(),
                if ws.unacked_firing_count > 0 { RED_WATCHDOG } else { TEXT_SECONDARY });
            metric_card(ui, "ALERT RULES", &ws.alert_rules.len().to_string(), TEXT_SECONDARY);
            metric_card(ui, "CUSTOM RULES", &ws.custom_event_rules.len().to_string(), TEXT_SECONDARY);
        });

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // Event type breakdown (from facets)
        let range_label = TIME_RANGE_LABELS.get(ws.event_time_range)
            .map(|&(l, _)| l).unwrap_or("24H");
        ui.label(
            egui::RichText::new(format!("EVENT TYPES ({})", range_label))
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );
        ui.add_space(4.0);

        if let Some((_, type_facets)) = ws.facets.iter().find(|(k, _)| k == "event_type") {
            if type_facets.is_empty() {
                ui.label(
                    egui::RichText::new("No events in last hour")
                        .color(TEXT_SECONDARY)
                        .size(FONT_SIZE_DATA),
                );
            } else {
                egui::Grid::new("facet_grid")
                    .num_columns(3)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for (val, count) in type_facets.iter().take(15) {
                            let display = rf_events::event_types::display_name_or_raw(val);
                            ui.label(
                                egui::RichText::new(display)
                                    .size(FONT_SIZE_DATA)
                                    .color(CYAN_P25)
                                    .family(egui::FontFamily::Monospace),
                            );
                            ui.label(
                                egui::RichText::new(val)
                                    .size(FONT_SIZE_HUD)
                                    .color(TEXT_SECONDARY)
                                    .family(egui::FontFamily::Monospace),
                            );
                            ui.label(
                                egui::RichText::new(count.to_string())
                                    .size(FONT_SIZE_DATA)
                                    .color(TEXT_PRIMARY)
                                    .family(egui::FontFamily::Monospace),
                            );
                            ui.end_row();
                        }
                    });
            }
        } else {
            ui.label(
                egui::RichText::new("Loading...")
                    .color(TEXT_SECONDARY)
                    .size(FONT_SIZE_DATA),
            );
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // Recent alert firings
        ui.label(
            egui::RichText::new("RECENT ALERTS")
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );
        ui.add_space(4.0);

        if ws.alert_firings.is_empty() {
            ui.label(
                egui::RichText::new("No alert firings")
                    .color(TEXT_SECONDARY)
                    .size(FONT_SIZE_DATA),
            );
        } else {
            for firing in ws.alert_firings.iter().take(10) {
                let age = format_age_ns(firing.fired_ns);
                let ack_mark = if firing.acknowledged { " [ACK]" } else { "" };
                let color = if firing.acknowledged { TEXT_SECONDARY } else { AMBER_WARNING };
                ui.label(
                    egui::RichText::new(format!(
                        "{} — {} (x{}){}",
                        age, firing.rule_name, firing.match_count, ack_mark,
                    ))
                    .size(FONT_SIZE_DATA)
                    .color(color)
                    .family(egui::FontFamily::Monospace),
                );
            }
        }
    });
}

// ── Threats Tab ──────────────────────────────────────────────

fn show_threats(ui: &mut egui::Ui, ws: &WatchdogState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);

        // Composite threat score — single-glance posture assessment
        let score = ws.cs_threat_score;
        let (score_color, score_label) = if score > 20 {
            (RED_WATCHDOG, "HIGH THREAT")
        } else if score >= 5 {
            (AMBER_WARNING, "ELEVATED")
        } else {
            (GREEN_COLLECT, "LOW THREAT")
        };

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("COUNTER-SURVEILLANCE THREATS")
                    .size(FONT_SIZE_HEADER)
                    .color(RED_WATCHDOG)
                    .family(egui::FontFamily::Monospace),
            );
            ui.add_space(16.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(12, 4))
                .fill(BG_ELEVATED)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(score.to_string())
                                .size(FONT_SIZE_FREQ)
                                .color(score_color)
                                .family(egui::FontFamily::Monospace)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(score_label)
                                .size(FONT_SIZE_DATA)
                                .color(score_color)
                                .family(egui::FontFamily::Monospace),
                        );
                    });
                });
        });
        ui.add_space(8.0);

        // CS threat indicator badges
        let threat_indicators: &[(&str, usize, egui::Color32)] = &[
            ("NEW EMITTERS", ws.cs_new_emitter_count, RED_WATCHDOG),
            ("ANOMALIES", ws.cs_anomaly_count, AMBER_WARNING),
            ("BASELINE DEV", ws.cs_baseline_deviation_count, AMBER_WARNING),
            ("UID MISMATCH", ws.cs_uid_mismatch_count, RED_WATCHDOG),
        ];

        ui.horizontal_wrapped(|ui| {
            for (label, count, color) in threat_indicators {
                let display_color = if *count > 0 { *color } else { TEXT_SECONDARY };
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .fill(BG_SURFACE)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(count.to_string())
                                    .size(FONT_SIZE_LARGE)
                                    .color(display_color)
                                    .family(egui::FontFamily::Monospace)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(*label)
                                    .size(FONT_SIZE_HUD)
                                    .color(display_color),
                            );
                        });
                    });
            }
        });

        // New emitters (last 24h)
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(format!("NEW EMITTERS — LAST 24H ({})", ws.cs_new_emitters.len()))
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );

        if ws.cs_new_emitters.is_empty() {
            ui.label(
                egui::RichText::new("No new emitters detected in the last 24 hours.")
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY),
            );
        } else {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(4))
                .fill(BG_SURFACE)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    egui::Grid::new("cs_new_emitters")
                        .num_columns(7)
                        .spacing([10.0, 2.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in &["FP ID", "UID", "CFO Hz", "IQ Imbal", "Freq MHz", "Conf", "First Seen"] {
                                ui.label(egui::RichText::new(*h).color(RED_WATCHDOG).size(FONT_SIZE_HUD));
                            }
                            ui.end_row();

                            for fp in &ws.cs_new_emitters {
                                ui.label(egui::RichText::new(&fp.fingerprint_id)
                                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                    .family(egui::FontFamily::Monospace));
                                ui.label(egui::RichText::new(fp.uid.map_or("-".to_string(), |v| v.to_string()))
                                    .color(CYAN_P25).size(FONT_SIZE_DATA));
                                ui.label(egui::RichText::new(fp.freq_offset_hz.map_or("-".to_string(), |v| format!("{:.1}", v)))
                                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                                ui.label(egui::RichText::new(fp.iq_imbalance.map_or("-".to_string(), |v| format!("{:.3}", v)))
                                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                                ui.label(egui::RichText::new(fp.freq_mhz.map_or("-".to_string(), |v| format!("{:.4}", v)))
                                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                                ui.label(egui::RichText::new(fp.confidence.map_or("-".to_string(), |v| format!("{:.0}%", v * 100.0)))
                                    .color(AMBER_WARNING).size(FONT_SIZE_DATA));
                                let time = fp.first_seen.get(..16).unwrap_or(&fp.first_seen);
                                ui.label(egui::RichText::new(time)
                                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                                ui.end_row();
                            }
                        });
                });
        }

        // CS event feed
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(format!("CS EVENT FEED ({})", ws.cs_recent_threats.len()))
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );

        if ws.cs_recent_threats.is_empty() {
            ui.label(
                egui::RichText::new("No counter-surveillance events in the last 24 hours.")
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY),
            );
        } else {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(4))
                .fill(BG_SURFACE)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    egui::Grid::new("cs_event_feed")
                        .num_columns(5)
                        .spacing([10.0, 2.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in &["Type", "Severity", "Summary", "Source", "Time"] {
                                ui.label(egui::RichText::new(*h).color(RED_WATCHDOG).size(FONT_SIZE_HUD));
                            }
                            ui.end_row();

                            for evt in &ws.cs_recent_threats {
                                let type_label = rf_events::event_types::display_name_or_raw(&evt.event_type);
                                let sev_color = severity_color(evt.severity.as_u8());
                                ui.label(egui::RichText::new(type_label)
                                    .color(sev_color).size(FONT_SIZE_DATA));
                                ui.label(egui::RichText::new(format!("{}", evt.severity.as_u8()))
                                    .color(sev_color).size(FONT_SIZE_DATA));

                                let body = &evt.body;
                                let display_body = if body.chars().nth(50).is_some() {
                                    let s: String = body.chars().take(50).collect();
                                    format!("{}...", s)
                                } else {
                                    body.clone()
                                };
                                ui.label(egui::RichText::new(&display_body)
                                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                                ui.label(egui::RichText::new(evt.source.label())
                                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                                let time_str = format_timestamp_ns(evt.timestamp_ns);
                                ui.label(egui::RichText::new(&time_str)
                                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                                ui.end_row();
                            }
                        });
                });
        }
    });
}

// ── Events Tab ───────────────────────────────────────────────

fn show_events(ui: &mut egui::Ui, ws: &mut WatchdogState, db: &rf_db::Db, ui_state: &mut UiState) {
    // Auto-run query on first visit or after force_refresh
    if !ws.events_initialized {
        ws.run_event_query(db);
    }

    let available = ui.available_rect_before_wrap();
    let facet_w = ui_state.watchdog_facet_w.clamp(100.0, (available.width() - 200.0).max(100.0));
    let divider_x = available.left() + facet_w;

    // --- Vertical drag handle to resize facet panel ---
    let vhandle_rect = egui::Rect::from_min_size(
        egui::pos2(divider_x - 2.0, available.top()),
        egui::vec2(5.0, available.height()),
    );
    let vhandle_resp = ui.interact(vhandle_rect, egui::Id::new("events_v_split"), egui::Sense::drag());
    if vhandle_resp.dragged() {
        ui_state.watchdog_facet_w = (vhandle_resp.interact_pointer_pos().unwrap().x - available.left())
            .clamp(100.0, (available.width() - 200.0).max(100.0));
    }
    let vhandle_color = if vhandle_resp.hovered() || vhandle_resp.dragged() { BORDER_BRIGHT } else { BORDER };
    ui.painter().line_segment(
        [egui::pos2(divider_x, available.top()), egui::pos2(divider_x, available.bottom())],
        egui::Stroke::new(if vhandle_resp.hovered() || vhandle_resp.dragged() { 2.0 } else { 1.0 }, vhandle_color),
    );
    if vhandle_resp.hovered() || vhandle_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // --- Left: Facet Panel ---
    let facet_rect = egui::Rect::from_min_max(
        available.left_top(),
        egui::pos2(divider_x - 3.0, available.bottom()),
    );
    let mut facet_ui = ui.new_child(egui::UiBuilder::new().max_rect(facet_rect));
    facet_ui.set_clip_rect(facet_rect);
    let mut facet_click: Option<(String, String)> = None;
    show_facet_panel(&mut facet_ui, ws, &mut facet_click);

    // Apply facet click (sets filter + reruns query)
    if let Some((field, value)) = facet_click {
        match field.as_str() {
            "event_type" => ws.event_type_filter = value,
            "band" => ws.event_band_filter = value,
            "source" => ws.event_source_filter = value,
            "severity" => ws.event_severity_filter = value,
            _ => {}
        }
        ws.query_offset = 0;
        ws.run_event_query(db);
    }

    // --- Right: FilterBar + EventTable ---
    let table_rect = egui::Rect::from_min_max(
        egui::pos2(divider_x + 3.0, available.top()),
        available.right_bottom(),
    );
    let mut table_ui = ui.new_child(egui::UiBuilder::new().max_rect(table_rect));
    table_ui.set_clip_rect(table_rect);

    let mut needs_query = false;

    table_ui.vertical(|ui| {
        // --- FilterBar ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // Time range pills
            for (i, &(label, _)) in TIME_RANGE_LABELS.iter().enumerate() {
                let selected = ws.event_time_range == i;
                let color = if selected { GREEN_COLLECT } else { TEXT_SECONDARY };
                if ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(FONT_SIZE_HUD)
                            .color(color)
                            .family(egui::FontFamily::Monospace),
                    ).sense(egui::Sense::click()),
                ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    ws.event_time_range = i;
                    ws.query_offset = 0;
                    needs_query = true;
                }
            }

            ui.separator();

            // Type filter (combo box with search + suggestions)
            {
                // Build display label for current value
                let current_display = if ws.event_type_filter.is_empty() {
                    "event type...".to_string()
                } else {
                    let d = rf_events::event_types::display_name_or_raw(&ws.event_type_filter);
                    d.to_string()
                };
                egui::ComboBox::from_id_salt("evt_type_filter")
                    .selected_text(egui::RichText::new(&current_display)
                        .size(FONT_SIZE_DATA)
                        .color(if ws.event_type_filter.is_empty() { TEXT_SECONDARY } else { CYAN_P25 })
                        .family(egui::FontFamily::Monospace))
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        // "All" option
                        if ui.selectable_label(ws.event_type_filter.is_empty(), "ALL").clicked() {
                            ws.event_type_filter.clear();
                            ws.query_offset = 0;
                            needs_query = true;
                        }
                        ui.separator();
                        // Facet values first (with counts)
                        let mut shown = std::collections::HashSet::new();
                        if let Some((_, facets)) = ws.facets.iter().find(|(k, _)| k == "event_type") {
                            for (val, count) in facets {
                                shown.insert(val.clone());
                                let display = rf_events::event_types::display_name_or_raw(val);
                                let label = format!("{display} ({count})");
                                if ui.selectable_label(ws.event_type_filter == *val, &label).clicked() {
                                    ws.event_type_filter = val.clone();
                                    ws.query_offset = 0;
                                    needs_query = true;
                                }
                            }
                        }
                        // Known types not in facets
                        if !shown.is_empty() { ui.separator(); }
                        for &(raw, display) in rf_events::event_types::ALL_DISPLAY {
                            if shown.contains(raw) { continue; }
                            if ui.selectable_label(ws.event_type_filter == raw, display).clicked() {
                                ws.event_type_filter = raw.to_string();
                                ws.query_offset = 0;
                                needs_query = true;
                            }
                        }
                    });
            }

            // Band filter (combo box)
            {
                let band_display = if ws.event_band_filter.is_empty() { "band..." } else { &ws.event_band_filter };
                egui::ComboBox::from_id_salt("evt_band_filter")
                    .selected_text(egui::RichText::new(band_display)
                        .size(FONT_SIZE_DATA)
                        .color(if ws.event_band_filter.is_empty() { TEXT_SECONDARY } else { CYAN_P25 })
                        .family(egui::FontFamily::Monospace))
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(ws.event_band_filter.is_empty(), "ALL").clicked() {
                            ws.event_band_filter.clear();
                            ws.query_offset = 0;
                            needs_query = true;
                        }
                        for &band in KNOWN_BANDS {
                            if ui.selectable_label(ws.event_band_filter == band, band).clicked() {
                                ws.event_band_filter = band.to_string();
                                ws.query_offset = 0;
                                needs_query = true;
                            }
                        }
                    });
            }

            // Source filter (combo box)
            {
                let src_display = if ws.event_source_filter.is_empty() {
                    "source...".to_string()
                } else if let Ok(s) = ws.event_source_filter.parse::<u8>() {
                    source_label(s).to_string()
                } else {
                    ws.event_source_filter.clone()
                };
                egui::ComboBox::from_id_salt("evt_src_filter")
                    .selected_text(egui::RichText::new(&src_display)
                        .size(FONT_SIZE_DATA)
                        .color(if ws.event_source_filter.is_empty() { TEXT_SECONDARY } else { CYAN_P25 })
                        .family(egui::FontFamily::Monospace))
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(ws.event_source_filter.is_empty(), "ALL").clicked() {
                            ws.event_source_filter.clear();
                            ws.query_offset = 0;
                            needs_query = true;
                        }
                        for &(val, label) in SRC_CHOICES {
                            let is_sel = ws.event_source_filter == val.to_string();
                            if ui.selectable_label(is_sel, label).clicked() {
                                ws.event_source_filter = val.to_string();
                                ws.query_offset = 0;
                                needs_query = true;
                            }
                        }
                    });
            }

            // Severity filter (combo box)
            {
                let sev_display = if ws.event_severity_filter.is_empty() {
                    "severity...".to_string()
                } else if let Ok(s) = ws.event_severity_filter.parse::<u8>() {
                    rf_events::event::Severity::from_u8(s).label().to_string()
                } else {
                    ws.event_severity_filter.clone()
                };
                egui::ComboBox::from_id_salt("evt_sev_filter")
                    .selected_text(egui::RichText::new(&sev_display)
                        .size(FONT_SIZE_DATA)
                        .color(if ws.event_severity_filter.is_empty() { TEXT_SECONDARY } else {
                            ws.event_severity_filter.parse::<u8>().map(severity_color).unwrap_or(TEXT_PRIMARY)
                        })
                        .family(egui::FontFamily::Monospace))
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(ws.event_severity_filter.is_empty(), "ALL").clicked() {
                            ws.event_severity_filter.clear();
                            ws.query_offset = 0;
                            needs_query = true;
                        }
                        for &(val, label) in SEV_CHOICES {
                            let is_sel = ws.event_severity_filter == val.to_string();
                            if ui.selectable_label(is_sel, label).clicked() {
                                ws.event_severity_filter = val.to_string();
                                ws.query_offset = 0;
                                needs_query = true;
                            }
                        }
                    });
            }

            if ui.small_button("Clear").clicked() {
                ws.event_type_filter.clear();
                ws.event_severity_filter.clear();
                ws.event_band_filter.clear();
                ws.event_source_filter.clear();
                ws.query_offset = 0;
                needs_query = true;
            }
        });

        // --- Pagination bar ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let page = ws.query_offset / 100 + 1;
            let total_pages = (ws.query_total as usize + 99) / 100;
            ui.label(
                egui::RichText::new(format!(
                    "{} results — page {}/{}",
                    ws.query_total, page, total_pages.max(1),
                ))
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
            );

            if ws.query_offset > 0 && ui.small_button("\u{25C0}").clicked() {
                ws.query_offset = ws.query_offset.saturating_sub(100);
                needs_query = true;
            }
            if (ws.query_offset + 100) < ws.query_total as usize && ui.small_button("\u{25B6}").clicked() {
                ws.query_offset += 100;
                needs_query = true;
            }

            // Active filters summary
            let mut active: Vec<String> = Vec::new();
            if !ws.event_type_filter.is_empty() {
                active.push(format!("type={}", ws.event_type_filter));
            }
            if !ws.event_band_filter.is_empty() {
                active.push(format!("band={}", ws.event_band_filter));
            }
            if !ws.event_severity_filter.is_empty() {
                active.push(format!("sev={}", ws.event_severity_filter));
            }
            if !ws.event_source_filter.is_empty() {
                active.push(format!("src={}", ws.event_source_filter));
            }
            if !active.is_empty() {
                ui.label(
                    egui::RichText::new(format!("[{}]", active.join(", ")))
                        .size(FONT_SIZE_HUD)
                        .color(AMBER_WARNING)
                        .family(egui::FontFamily::Monospace),
                );
            }
        });

        ui.separator();

        // --- EventTable ---
        let mut click_expand: Option<usize> = None;

        egui::ScrollArea::vertical()
            .id_salt("event_table_scroll")
            .show(ui, |ui| {
                if ws.query_results.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(if ws.events_initialized {
                            "No matching events"
                        } else {
                            "Loading..."
                        })
                        .color(TEXT_SECONDARY)
                        .size(FONT_SIZE_DATA),
                    );
                } else {
                    // Render table rows + inline detail panels
                    // Grid for each row, detail rendered outside Grid for full width
                    for (idx, evt) in ws.query_results.iter().enumerate() {
                        let sev_color = severity_color(evt.severity.as_u8());
                        let is_expanded = ws.event_expanded == Some(idx);

                        // Header (only for first row)
                        if idx == 0 {
                            egui::Grid::new("event_table_header")
                                .num_columns(7)
                                .spacing([6.0, 1.0])
                                .show(ui, |ui| {
                                    for h in &["", "TIME", "SEV", "SOURCE", "TYPE", "BODY", "FREQ"] {
                                        ui.label(
                                            egui::RichText::new(*h)
                                                .size(FONT_SIZE_HUD)
                                                .color(TEXT_SECONDARY)
                                                .family(egui::FontFamily::Monospace),
                                        );
                                    }
                                    ui.end_row();
                                });
                        }

                        // Event row
                        let grid_resp = egui::Grid::new(egui::Id::new(("event_row", idx)))
                            .num_columns(7)
                            .spacing([6.0, 1.0])
                            .show(ui, |ui| {
                                // Severity dot
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, FONT_SIZE_DATA),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(
                                    dot_rect.center(), 2.5, sev_color,
                                );

                                // Timestamp (clickable to expand)
                                let ts = format_timestamp_ns(evt.timestamp_ns);
                                if ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&ts)
                                            .size(FONT_SIZE_DATA)
                                            .color(if is_expanded { CYAN_P25 } else { TEXT_SECONDARY })
                                            .family(egui::FontFamily::Monospace),
                                    ).sense(egui::Sense::click()),
                                ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    click_expand = Some(idx);
                                }

                                // Severity (3-char)
                                let sev_short = sev_label_short(evt.severity.as_u8());
                                ui.label(
                                    egui::RichText::new(sev_short)
                                        .size(FONT_SIZE_DATA)
                                        .color(sev_color)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // Source
                                ui.label(
                                    egui::RichText::new(source_label(evt.source.as_u8()))
                                        .size(FONT_SIZE_DATA)
                                        .color(TEXT_SECONDARY)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // Event type (human-readable)
                                let type_display = rf_events::event_types::display_name_or_raw(&evt.event_type);
                                ui.label(
                                    egui::RichText::new(type_display)
                                        .size(FONT_SIZE_DATA)
                                        .color(CYAN_P25)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // Body (truncated)
                                let body_short: String = evt.body.chars().take(50).collect();
                                ui.label(
                                    egui::RichText::new(&body_short)
                                        .size(FONT_SIZE_DATA)
                                        .color(TEXT_PRIMARY)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // Freq
                                let freq_str = evt.freq_mhz
                                    .map(|f| format!("{:.4}", f))
                                    .unwrap_or_default();
                                ui.label(
                                    egui::RichText::new(&freq_str)
                                        .size(FONT_SIZE_DATA)
                                        .color(GREEN_COLLECT)
                                        .family(egui::FontFamily::Monospace),
                                );

                                ui.end_row();
                            });

                        // Paint alternating row background over entire grid rect
                        if idx % 2 == 1 {
                            ui.painter().rect_filled(
                                grid_resp.response.rect,
                                0.0,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 4),
                            );
                        }

                        // Detail panel — rendered outside Grid for full width
                        if is_expanded {
                            show_event_detail(ui, evt);
                        }
                    }
                }
            });

        // Apply expand toggle outside scroll borrow
        if let Some(idx) = click_expand {
            ws.event_expanded = if ws.event_expanded == Some(idx) { None } else { Some(idx) };
        }
    });

    ui.allocate_rect(available, egui::Sense::hover());

    // Run query if needed (after UI to avoid borrow issues)
    if needs_query {
        ws.run_event_query(db);
    }
}

// ── Facet Panel ──────────────────────────────────────────────

fn show_facet_panel(
    ui: &mut egui::Ui,
    ws: &WatchdogState,
    click: &mut Option<(String, String)>,
) {
    // Resolve current filter value per field for highlighting
    let active_filter = |field: &str| -> &str {
        match field {
            "event_type" => &ws.event_type_filter,
            "severity" => &ws.event_severity_filter,
            "band" => &ws.event_band_filter,
            "source" => &ws.event_source_filter,
            _ => "",
        }
    };

    egui::ScrollArea::vertical()
        .id_salt("facet_scroll")
        .show(ui, |ui| {
            ui.add_space(2.0);

            for (field, facets) in &ws.facets {
                let title = match field.as_str() {
                    "event_type" => "EVENT TYPE",
                    "severity" => "SEVERITY",
                    "band" => "BAND",
                    "source" => "SOURCE",
                    other => other,
                };

                let current_filter = active_filter(field);

                ui.label(
                    egui::RichText::new(title)
                        .size(FONT_SIZE_HUD)
                        .color(RED_WATCHDOG)
                        .family(egui::FontFamily::Monospace),
                );

                if facets.is_empty() {
                    ui.label(
                        egui::RichText::new("  —")
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY),
                    );
                } else {
                    // Find max count for proportion bar
                    let max_count = facets.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);

                    for (value, count) in facets.iter().take(12) {
                        let is_active = !current_filter.is_empty() && current_filter == value;

                        // Display label: human-readable names for all field types
                        let display = match field.as_str() {
                            "event_type" => {
                                rf_events::event_types::display_name(value)
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| value.clone())
                            }
                            "severity" => {
                                if let Ok(s) = value.parse::<u8>() {
                                    rf_events::event::Severity::from_u8(s).label().to_string()
                                } else {
                                    value.clone()
                                }
                            }
                            "source" => {
                                if let Ok(s) = value.parse::<u8>() {
                                    source_label(s).to_string()
                                } else {
                                    value.clone()
                                }
                            }
                            _ => value.clone(),
                        };

                        // Color: active filter = green, normal = cyan
                        let text_color = if is_active { GREEN_COLLECT } else { CYAN_P25 };
                        let prefix = if is_active { "\u{25B8} " } else { "  " };
                        let text = format!("{}{} ({})", prefix, display, count);

                        // Pre-compute bar color
                        let bar_frac = *count as f32 / max_count as f32;
                        let bar_color = if is_active {
                            egui::Color32::from_rgba_unmultiplied(
                                GREEN_COLLECT.r(), GREEN_COLLECT.g(), GREEN_COLLECT.b(), 15,
                            )
                        } else {
                            egui::Color32::from_rgba_unmultiplied(
                                CYAN_P25.r(), CYAN_P25.g(), CYAN_P25.b(), 10,
                            )
                        };

                        // Reserve space, paint bar, then draw label on top
                        let available_width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(available_width, FONT_SIZE_DATA + 2.0),
                            egui::Sense::click(),
                        );

                        // Proportion bar (behind text)
                        let bar_rect = egui::Rect::from_min_size(
                            rect.left_top(),
                            egui::vec2(rect.width() * bar_frac, rect.height()),
                        );
                        ui.painter().rect_filled(bar_rect, 0.0, bar_color);

                        // Severity color dot
                        if field == "severity" {
                            if let Ok(s) = value.parse::<u8>() {
                                let dot_pos = egui::pos2(rect.left() + 4.0, rect.center().y);
                                ui.painter().circle_filled(dot_pos, 2.5, severity_color(s));
                            }
                        }

                        // Label text on top
                        ui.painter().text(
                            rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            &text,
                            egui::FontId::monospace(FONT_SIZE_DATA),
                            text_color,
                        );

                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

                        if resp.clicked() {
                            if is_active {
                                // Click active filter to clear it
                                *click = Some((field.clone(), String::new()));
                            } else {
                                *click = Some((field.clone(), value.clone()));
                            }
                        }

                        // Hover effect
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                resp.rect,
                                egui::CornerRadius::ZERO,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                            );
                        }
                    }
                }

                ui.add_space(6.0);
            }
        });
}

// ── Event Detail (expanded row) ──────────────────────────────

fn show_event_detail(ui: &mut egui::Ui, evt: &LogRecord) {
    let detail_frame = egui::Frame::new()
        .fill(BG_ELEVATED)
        .inner_margin(egui::Margin::same(6));

    detail_frame.show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;

            detail_field(ui, "ID", &evt.id.to_string());
            detail_field(ui, "TIME", &format_timestamp_ns(evt.timestamp_ns));
            detail_field(ui, "AGE", &format_age_ns(evt.timestamp_ns));
            detail_field(ui, "SEV", evt.severity.label());
            detail_field(ui, "SRC", evt.source.label());
            detail_field(ui, "TYPE", &evt.event_type);

            if let Some(freq) = evt.freq_mhz {
                detail_field(ui, "FREQ", &format!("{:.4} MHz", freq));
            }
            if let Some(tg) = evt.talkgroup {
                detail_field(ui, "TG", &tg.to_string());
            }
            if let Some(su) = evt.source_unit {
                detail_field(ui, "UID", &su.to_string());
            }
            if let Some(nac) = evt.nac {
                detail_field(ui, "NAC", &format!("0x{:03X}", nac));
            }
            if let Some(enc) = evt.encrypted {
                detail_field(ui, "ENC", if enc { "YES" } else { "NO" });
            }
            if let Some(ref band) = evt.band {
                detail_field(ui, "BAND", band);
            }
            if let Some(ref dk) = evt.device_key {
                detail_field(ui, "DEV", dk);
            }
            if let Some(ref cls) = evt.classification {
                detail_field(ui, "CLS", cls);
            }
            if let Some(tid) = evt.trace_id {
                detail_field(ui, "TRACE", &tid.to_string());
            }
            if let Some(sid) = evt.span_id {
                detail_field(ui, "SPAN", &sid.to_string());
            }
            if let Some(oid) = evt.operation_id {
                detail_field(ui, "OP", &oid.to_string());
            }
            if let Some(ssid) = evt.site_session_id {
                detail_field(ui, "SITE_S", &ssid.to_string());
            }
            if let Some(lat) = evt.receiver_lat {
                if let Some(lon) = evt.receiver_lon {
                    detail_field(ui, "POS", &format!("{:.5},{:.5}", lat, lon));
                }
            }
        });

        // Full body
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(&evt.body)
                .size(FONT_SIZE_DATA)
                .color(TEXT_PRIMARY)
                .family(egui::FontFamily::Monospace),
        );

        // Attributes
        if !evt.attributes.is_empty() {
            ui.add_space(2.0);
            let attr_str = serde_json::to_string(&evt.attributes).unwrap_or_default();
            ui.label(
                egui::RichText::new(attr_str)
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
        }
    });
}

fn detail_field(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.label(
            egui::RichText::new(format!("{}:", label))
                .size(FONT_SIZE_DATA)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
        ui.label(
            egui::RichText::new(value)
                .size(FONT_SIZE_DATA)
                .color(TEXT_PRIMARY)
                .family(egui::FontFamily::Monospace),
        );
    });
}

// ── Alerts Tab ───────────────────────────────────────────────

fn show_alerts(ui: &mut egui::Ui, ws: &mut WatchdogState, db: &rf_db::Db) {
    let mut ack_id: Option<i64> = None;
    let mut ack_all = false;
    let mut click_expand: Option<i64> = None;

    ui.vertical(|ui| {
        // --- Header + toolbar ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            ui.label(
                egui::RichText::new("ALERT FIRINGS")
                    .size(FONT_SIZE_HEADER)
                    .color(RED_WATCHDOG)
                    .family(egui::FontFamily::Monospace),
            );

            if ws.unacked_firing_count > 0 {
                ui.label(
                    egui::RichText::new(format!("({} unacked)", ws.unacked_firing_count))
                        .size(FONT_SIZE_DATA)
                        .color(AMBER_WARNING)
                        .family(egui::FontFamily::Monospace),
                );
            }

            ui.separator();

            // Filter pills: ALL / NEW / ACK
            for &(label, filter_val) in &[("ALL", 0u8), ("NEW", 1), ("ACK", 2)] {
                let selected = ws.alerts_filter == filter_val;
                let color = if selected { GREEN_COLLECT } else { TEXT_SECONDARY };
                if ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(FONT_SIZE_HUD)
                            .color(color)
                            .family(egui::FontFamily::Monospace),
                    ).sense(egui::Sense::click()),
                ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    ws.alerts_filter = filter_val;
                }
            }

            ui.separator();

            // ACK ALL button
            if ws.unacked_firing_count > 0 && ui.small_button("ACK ALL").clicked() {
                ack_all = true;
            }
        });

        ui.add_space(2.0);
        ui.separator();

        // --- Firing list ---
        egui::ScrollArea::vertical()
            .id_salt("alerts_scroll")
            .show(ui, |ui| {
                if ws.alert_firings.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("No alert firings yet")
                            .color(TEXT_SECONDARY)
                            .size(FONT_SIZE_DATA),
                    );
                    return;
                }

                // Filter firings
                let filtered: Vec<&AlertFiring> = ws.alert_firings.iter().filter(|f| {
                    match ws.alerts_filter {
                        1 => !f.acknowledged,
                        2 => f.acknowledged,
                        _ => true,
                    }
                }).collect();

                if filtered.is_empty() {
                    ui.add_space(8.0);
                    let msg = match ws.alerts_filter {
                        1 => "No unacknowledged firings",
                        2 => "No acknowledged firings",
                        _ => "No firings",
                    };
                    ui.label(
                        egui::RichText::new(msg)
                            .color(TEXT_SECONDARY)
                            .size(FONT_SIZE_DATA),
                    );
                    return;
                }

                // Resolve rule priority for each firing
                for firing in &filtered {
                    let is_expanded = ws.alerts_expanded == Some(firing.id);
                    let rule_priority = ws.alert_rules.iter()
                        .find(|r| r.id == firing.rule_id)
                        .map(|r| r.priority);
                    let p_color = rule_priority
                        .as_ref()
                        .map(priority_color)
                        .unwrap_or(TEXT_SECONDARY);

                    // Row frame — subtle highlight for unacked
                    let row_bg = if !firing.acknowledged {
                        egui::Color32::from_rgba_unmultiplied(255, 80, 50, 12)
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    egui::Frame::new()
                        .fill(row_bg)
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;

                                // Priority dot
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(8.0, FONT_SIZE_DATA),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(dot_rect.center(), 3.5, p_color);

                                // Age (clickable to expand)
                                let age = format_age_ns(firing.fired_ns);
                                if ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&age)
                                            .size(FONT_SIZE_DATA)
                                            .color(if is_expanded { CYAN_P25 } else { TEXT_SECONDARY })
                                            .family(egui::FontFamily::Monospace),
                                    ).sense(egui::Sense::click()),
                                ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    click_expand = Some(firing.id);
                                }

                                // Priority label
                                if let Some(p) = &rule_priority {
                                    ui.label(
                                        egui::RichText::new(p.label())
                                            .size(FONT_SIZE_HUD)
                                            .color(p_color)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                }

                                // Rule name
                                ui.label(
                                    egui::RichText::new(&firing.rule_name)
                                        .size(FONT_SIZE_DATA)
                                        .color(RED_WATCHDOG)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // Match count
                                ui.label(
                                    egui::RichText::new(format!("x{}", firing.match_count))
                                        .size(FONT_SIZE_DATA)
                                        .color(TEXT_PRIMARY)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // Status
                                let (status_text, status_color) = if firing.acknowledged {
                                    ("ACK", TEXT_SECONDARY)
                                } else {
                                    ("NEW", AMBER_WARNING)
                                };
                                ui.label(
                                    egui::RichText::new(status_text)
                                        .size(FONT_SIZE_DATA)
                                        .color(status_color)
                                        .family(egui::FontFamily::Monospace),
                                );

                                // ACK button (only for unacked)
                                if !firing.acknowledged && ui.small_button("ACK").clicked() {
                                    ack_id = Some(firing.id);
                                }
                            });
                        });

                    // Expanded detail panel
                    if is_expanded {
                        show_firing_detail(ui, firing, &ws.alert_rules);
                    }
                }
            });
    });

    // Apply ack actions outside UI borrow
    if ack_all {
        for firing in &ws.alert_firings {
            if !firing.acknowledged {
                let _ = db.acknowledge_alert(firing.id);
            }
        }
        ws.force_refresh = true;
    } else if let Some(id) = ack_id {
        let _ = db.acknowledge_alert(id);
        ws.force_refresh = true;
    }

    // Apply expand toggle
    if let Some(id) = click_expand {
        ws.alerts_expanded = if ws.alerts_expanded == Some(id) { None } else { Some(id) };
    }
}

/// Detail panel for an expanded alert firing.
fn show_firing_detail(ui: &mut egui::Ui, firing: &AlertFiring, rules: &[AlertRule]) {
    let detail_frame = egui::Frame::new()
        .fill(BG_ELEVATED)
        .inner_margin(egui::Margin::same(6));

    detail_frame.show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;

            detail_field(ui, "ID", &firing.id.to_string());
            detail_field(ui, "RULE_ID", &firing.rule_id.to_string());
            detail_field(ui, "FIRED", &format_timestamp_ns(firing.fired_ns));
            detail_field(ui, "AGE", &format_age_ns(firing.fired_ns));
            detail_field(ui, "MATCHES", &firing.match_count.to_string());

            if let Some(eid) = firing.sample_event_id {
                detail_field(ui, "SAMPLE_EVT", &eid.to_string());
            }

            if firing.acknowledged {
                detail_field(ui, "STATUS", "ACKNOWLEDGED");
                if let Some(ack_ns) = firing.ack_ns {
                    detail_field(ui, "ACK_AT", &format_timestamp_ns(ack_ns));
                }
            } else {
                detail_field(ui, "STATUS", "NEW");
            }
        });

        // Show linked rule details
        if let Some(rule) = rules.iter().find(|r| r.id == firing.rule_id) {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                detail_field(ui, "PRIORITY", rule.priority.label());
                detail_field(ui, "CONDITION", &condition_summary(&rule.condition));
                detail_field(ui, "COOLDOWN", &format!("{}s", rule.cooldown_sec));
                detail_field(ui, "ENABLED", if rule.enabled { "YES" } else { "NO" });

                if !rule.filter.is_empty() {
                    let filter_str = rule.filter.iter()
                        .map(|f| format!("{:?}", f))
                        .collect::<Vec<_>>()
                        .join(", ");
                    detail_field(ui, "FILTER", &filter_str);
                }
            });
        }
    });
}

// ── Rules Tab ────────────────────────────────────────────────

fn show_rules(ui: &mut egui::Ui, ws: &mut WatchdogState, db: &rf_db::Db) {
    // Deferred actions
    let mut alert_toggle: Option<(i64, bool)> = None;
    let mut alert_click: Option<i64> = None;
    let mut alert_delete: Option<i64> = None;
    let mut alert_save: Option<AlertRule> = None;
    let mut custom_toggle: Option<(i64, bool)> = None;
    let mut custom_click: Option<i64> = None;
    let mut custom_delete: Option<i64> = None;
    let mut custom_save: Option<CustomEventRule> = None;
    let mut new_alert_preset: Option<AlertRule> = None;
    let mut new_custom_preset: Option<CustomEventRule> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(4.0);

        // ── Alert Rules section ──
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("ALERT RULES")
                    .size(FONT_SIZE_HEADER)
                    .color(RED_WATCHDOG)
                    .family(egui::FontFamily::Monospace),
            );
            ui.label(
                egui::RichText::new(format!("({})", ws.alert_rules.len()))
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
            // Preset menu
            ui.menu_button("+ New", |ui| {
                ui.label(
                    egui::RichText::new("FROM PRESET")
                        .size(FONT_SIZE_HUD)
                        .color(RED_WATCHDOG)
                        .family(egui::FontFamily::Monospace),
                );
                for (name, preset) in alert_presets() {
                    if ui.button(name).clicked() {
                        new_alert_preset = Some(preset);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Blank Rule").clicked() {
                    new_alert_preset = Some(AlertRule {
                        name: "New Alert Rule".to_string(),
                        ..AlertRule::default()
                    });
                    ui.close();
                }
            });
        });
        ui.add_space(4.0);

        if ws.alert_rules.is_empty() {
            ui.label(
                egui::RichText::new("No alert rules — click + New to create from a preset")
                    .color(TEXT_SECONDARY)
                    .size(FONT_SIZE_DATA),
            );
        } else {
            for rule in &ws.alert_rules {
                let is_expanded = ws.alert_rule_expanded == Some(rule.id);
                let is_deleting = ws.rule_delete_confirm == Some((rule.id, true));

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    // Enable checkbox
                    let mut enabled = rule.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        alert_toggle = Some((rule.id, enabled));
                    }

                    // Priority dot
                    let p_color = priority_color(&rule.priority);
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(8.0, FONT_SIZE_DATA), egui::Sense::hover(),
                    );
                    ui.painter().circle_filled(dot_rect.center(), 3.5, p_color);

                    // Name (clickable)
                    if ui.add(
                        egui::Label::new(
                            egui::RichText::new(&rule.name)
                                .size(FONT_SIZE_DATA)
                                .color(if is_expanded { CYAN_P25 } else if rule.enabled { TEXT_PRIMARY } else { TEXT_SECONDARY })
                                .family(egui::FontFamily::Monospace),
                        ).sense(egui::Sense::click()),
                    ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        alert_click = Some(rule.id);
                    }

                    // Priority
                    ui.label(
                        egui::RichText::new(rule.priority.label())
                            .size(FONT_SIZE_HUD)
                            .color(p_color)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Condition summary
                    ui.label(
                        egui::RichText::new(condition_summary(&rule.condition))
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Cooldown
                    ui.label(
                        egui::RichText::new(format!("{}s", rule.cooldown_sec))
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Delete button
                    if is_deleting {
                        if ui.small_button("Confirm?").clicked() {
                            alert_delete = Some(rule.id);
                        }
                        if ui.small_button("Cancel").clicked() {
                            // Cancel handled in deferred section
                            ws.rule_delete_confirm = None;
                        }
                    } else if ui.small_button("\u{2716}").clicked() {
                        ws.rule_delete_confirm = Some((rule.id, true));
                    }
                });

            }
        }

        // Expanded alert rule editor (rendered outside the &ws.alert_rules borrow)
        // Clone facets for suggestions (avoids double-borrow of ws)
        let facets_snap = ws.facets.clone();
        if let Some(edit_rule) = &mut ws.editing_alert_rule {
            let changed = show_alert_rule_editor(ui, edit_rule, &facets_snap);
            if changed {
                alert_save = Some(edit_rule.clone());
            }
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // ── Custom Event Rules section ──
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("CUSTOM EVENT RULES")
                    .size(FONT_SIZE_HEADER)
                    .color(MAGENTA_EXPLOIT)
                    .family(egui::FontFamily::Monospace),
            );
            ui.label(
                egui::RichText::new(format!("({})", ws.custom_event_rules.len()))
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
            ui.menu_button("+ New", |ui| {
                ui.label(
                    egui::RichText::new("FROM PRESET")
                        .size(FONT_SIZE_HUD)
                        .color(MAGENTA_EXPLOIT)
                        .family(egui::FontFamily::Monospace),
                );
                for (name, preset) in custom_rule_presets() {
                    if ui.button(name).clicked() {
                        new_custom_preset = Some(preset);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Blank Rule").clicked() {
                    new_custom_preset = Some(CustomEventRule::default());
                    ui.close();
                }
            });
        });
        ui.add_space(4.0);

        if ws.custom_event_rules.is_empty() {
            ui.label(
                egui::RichText::new("No custom event rules — click + New to create from a preset")
                    .color(TEXT_SECONDARY)
                    .size(FONT_SIZE_DATA),
            );
        } else {
            for rule in &ws.custom_event_rules {
                let is_expanded = ws.custom_rule_expanded == Some(rule.id);
                let is_deleting = ws.rule_delete_confirm == Some((rule.id, false));

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    let mut enabled = rule.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        custom_toggle = Some((rule.id, enabled));
                    }

                    // Name (clickable)
                    if ui.add(
                        egui::Label::new(
                            egui::RichText::new(&rule.name)
                                .size(FONT_SIZE_DATA)
                                .color(if is_expanded { CYAN_P25 } else if rule.enabled { TEXT_PRIMARY } else { TEXT_SECONDARY })
                                .family(egui::FontFamily::Monospace),
                        ).sense(egui::Sense::click()),
                    ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        custom_click = Some(rule.id);
                    }

                    // Event type
                    ui.label(
                        egui::RichText::new(&rule.event_type)
                            .size(FONT_SIZE_DATA)
                            .color(CYAN_P25)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Condition
                    ui.label(
                        egui::RichText::new(custom_condition_summary(&rule.condition))
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Cooldown
                    ui.label(
                        egui::RichText::new(format!("{}s", rule.cooldown_sec))
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Delete
                    if is_deleting {
                        if ui.small_button("Confirm?").clicked() {
                            custom_delete = Some(rule.id);
                        }
                        if ui.small_button("Cancel").clicked() {
                            ws.rule_delete_confirm = None;
                        }
                    } else if ui.small_button("\u{2716}").clicked() {
                        ws.rule_delete_confirm = Some((rule.id, false));
                    }
                });

            }
        }

        // Expanded custom rule editor (rendered outside the &ws.custom_event_rules borrow)
        if let Some(edit_rule) = &mut ws.editing_custom_rule {
            let changed = show_custom_rule_editor(ui, edit_rule, &facets_snap);
            if changed {
                custom_save = Some(edit_rule.clone());
            }
        }
    });

    // ── Apply deferred actions ──

    // Alert rule toggle
    if let Some((id, enabled)) = alert_toggle {
        if let Some(mut rule) = ws.alert_rules.iter().find(|r| r.id == id).cloned() {
            rule.enabled = enabled;
            let _ = db.update_alert_rule(&rule);
            ws.force_refresh = true;
        }
    }

    // Alert rule expand toggle — initialize/clear editing copy
    if let Some(id) = alert_click {
        if ws.alert_rule_expanded == Some(id) {
            // Collapse
            ws.alert_rule_expanded = None;
            ws.editing_alert_rule = None;
        } else {
            // Expand — clone rule into persistent editing copy
            ws.alert_rule_expanded = Some(id);
            ws.editing_alert_rule = ws.alert_rules.iter().find(|r| r.id == id).cloned();
        }
    }

    // Alert rule save
    if let Some(rule) = alert_save {
        let _ = db.update_alert_rule(&rule);
        ws.editing_alert_rule = None;
        ws.alert_rule_expanded = None;
        ws.force_refresh = true;
    }

    // Alert rule delete
    if let Some(id) = alert_delete {
        let _ = db.delete_alert_rule(id);
        ws.rule_delete_confirm = None;
        ws.alert_rule_expanded = None;
        ws.editing_alert_rule = None;
        ws.force_refresh = true;
    }

    // New alert rule (from preset or blank)
    if let Some(preset_rule) = new_alert_preset {
        let mut rule = preset_rule;
        if let Ok(id) = db.insert_alert_rule(&rule) {
            rule.id = id;
            ws.alert_rule_expanded = Some(id);
            ws.editing_alert_rule = Some(rule);
            ws.force_refresh = true;
        }
    }

    // Custom rule toggle
    if let Some((id, enabled)) = custom_toggle {
        let _ = db.toggle_custom_event_rule(id, enabled);
        ws.force_refresh = true;
    }

    // Custom rule expand toggle — initialize/clear editing copy
    if let Some(id) = custom_click {
        if ws.custom_rule_expanded == Some(id) {
            ws.custom_rule_expanded = None;
            ws.editing_custom_rule = None;
        } else {
            ws.custom_rule_expanded = Some(id);
            ws.editing_custom_rule = ws.custom_event_rules.iter().find(|r| r.id == id).cloned();
        }
    }

    // Custom rule save
    if let Some(rule) = custom_save {
        let _ = db.update_custom_event_rule(&rule);
        ws.editing_custom_rule = None;
        ws.custom_rule_expanded = None;
        ws.force_refresh = true;
    }

    // Custom rule delete
    if let Some(id) = custom_delete {
        let _ = db.delete_custom_event_rule(id);
        ws.rule_delete_confirm = None;
        ws.custom_rule_expanded = None;
        ws.editing_custom_rule = None;
        ws.force_refresh = true;
    }

    // New custom event rule (from preset or blank)
    if let Some(preset_rule) = new_custom_preset {
        let rule = preset_rule;
        if let Ok(id) = db.insert_custom_event_rule(&rule) {
            let mut r = rule;
            r.id = id;
            ws.custom_rule_expanded = Some(id);
            ws.editing_custom_rule = Some(r);
            ws.force_refresh = true;
        }
    }
}

// ── Shared Editor Helpers ─────────────────────────────────

type Facets = Vec<(String, Vec<(String, u64)>)>;

/// Section header label for editor panels.
fn editor_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(FONT_SIZE_HUD)
            .color(TEXT_SECONDARY)
            .family(egui::FontFamily::Monospace),
    );
}

/// Human-readable field definitions for filter/condition combos.
/// (display_name, internal_field, hint_text)
const FILTER_FIELDS: &[(&str, rf_events::Field, &str)] = &[
    ("Event Type",    rf_events::Field::EventType,      "e.g. protocol.p25.grant"),
    ("Severity",      rf_events::Field::Severity,       "1=TRC 5=DBG 9=INF 13=WRN 17=ERR"),
    ("Source",        rf_events::Field::Source,          "0=SPEC 1=PROTO 2=SIGEX 3=SYS"),
    ("Frequency",     rf_events::Field::FreqMhz,        "e.g. 155.73"),
    ("Talkgroup",     rf_events::Field::Talkgroup,       "e.g. 1001"),
    ("Radio Unit ID", rf_events::Field::SourceUnit,      "e.g. 42001"),
    ("NAC",           rf_events::Field::Nac,             "e.g. 0x293"),
    ("Encrypted",     rf_events::Field::Encrypted,       "true or false"),
    ("Band",          rf_events::Field::Band,            "e.g. VHF, UHF, HF"),
    ("Device",        rf_events::Field::DeviceKey,       "SDR device key"),
    ("Classification",rf_events::Field::Classification,  "e.g. FEDL, PUBS, COMM"),
    ("Body Text",     rf_events::Field::Body,            "message body substring"),
];

/// Human-readable comparison operators.
const FILTER_OPS: &[(&str, &str, u8)] = &[
    ("equals",          "=",        0),
    ("not equals",      "!=",       1),
    ("greater than",    ">",        2),
    ("at least",        ">=",       3),
    ("less than",       "<",        4),
    ("at most",         "<=",       5),
    ("contains",        "contains", 6),
    ("matches pattern", "like",     7),
];

/// Known event types for autocomplete suggestions.
const KNOWN_EVENT_TYPES: &[&str] = &[
    "spectrum.detect", "spectrum.lost", "spectrum.anomaly",
    "protocol.p25.voice", "protocol.p25.grant", "protocol.p25.update",
    "protocol.p25.register", "protocol.p25.deregister", "protocol.p25.affiliation",
    "protocol.p25.deny", "protocol.p25.adjacent",
    "protocol.rds.update", "protocol.same.alert",
    "sigex.emitter.new", "sigex.emitter.return", "sigex.uid.mismatch",
    "sigex.crypto.rotation", "sigex.traffic.session", "sigex.anomaly.baseline",
    "system.sdr.connect", "system.sdr.disconnect", "system.sdr.error",
    "system.gps.fix", "system.mode.change",
    "system.recording.start", "system.recording.stop",
    "system.alert.fired",
];

/// Known band names.
const KNOWN_BANDS: &[&str] = &["VHF", "UHF", "HF", "700", "800", "900", "MARINE", "AIR", "FRS", "GMRS", "MURS"];

/// Known classifications.
const KNOWN_CLASSIFICATIONS: &[&str] = &["PUBS", "AMAT", "MARN", "WX", "GMRS", "COMM", "FEDL", "BCST", "UNK"];

/// Severity choices for combo (value, label).
const SEV_CHOICES: &[(u8, &str)] = &[
    (1, "TRACE"), (5, "DEBUG"), (9, "INFO"), (11, "NOTICE"),
    (13, "WARN"), (17, "ERROR"), (21, "FATAL"),
];

/// Source choices for combo (value, label).
const SRC_CHOICES: &[(u8, &str)] = &[
    (0, "SPECTRUM"), (1, "PROTOCOL"), (2, "SIGEX"), (3, "SYSTEM"), (4, "AUDIO"), (5, "CUSTOM"),
];

/// Resolve a Field to its human-readable label.
fn field_display(f: &rf_events::Field) -> &'static str {
    for &(label, ref known, _) in FILTER_FIELDS {
        if std::mem::discriminant(f) == std::mem::discriminant(known) {
            return label;
        }
    }
    "Attribute"
}

/// Map a filter variant to (op_index, field, value_string).
fn decompose_filter(f: &rf_events::Filter) -> (u8, rf_events::Field, String) {
    use rf_events::{Filter, Field};
    match f {
        Filter::Eq(field, val) => (0, field.clone(), filter_val_str(val)),
        Filter::Ne(field, val) => (1, field.clone(), filter_val_str(val)),
        Filter::Gt(field, val) => (2, field.clone(), filter_val_str(val)),
        Filter::Gte(field, val) => (3, field.clone(), filter_val_str(val)),
        Filter::Lt(field, val) => (4, field.clone(), filter_val_str(val)),
        Filter::Lte(field, val) => (5, field.clone(), filter_val_str(val)),
        Filter::Contains(field, s) => (6, field.clone(), s.clone()),
        Filter::Like(field, s) => (7, field.clone(), s.clone()),
        _ => (0, Field::EventType, String::new()),
    }
}

fn recompose_filter(op: u8, field: rf_events::Field, value: &str) -> rf_events::Filter {
    use rf_events::Filter;
    let fv = parse_filter_value(value);
    match op {
        0 => Filter::Eq(field, fv),
        1 => Filter::Ne(field, fv),
        2 => Filter::Gt(field, fv),
        3 => Filter::Gte(field, fv),
        4 => Filter::Lt(field, fv),
        5 => Filter::Lte(field, fv),
        6 => Filter::Contains(field, value.to_string()),
        7 => Filter::Like(field, value.to_string()),
        _ => Filter::Eq(field, fv),
    }
}

fn filter_val_str(fv: &rf_events::FilterValue) -> String {
    match fv {
        rf_events::FilterValue::String(s) => s.clone(),
        rf_events::FilterValue::Int(n) => n.to_string(),
        rf_events::FilterValue::Float(f) => f.to_string(),
        rf_events::FilterValue::Bool(b) => b.to_string(),
    }
}

fn parse_filter_value(s: &str) -> rf_events::FilterValue {
    if let Ok(n) = s.parse::<i64>() {
        rf_events::FilterValue::Int(n)
    } else if let Ok(f) = s.parse::<f64>() {
        rf_events::FilterValue::Float(f)
    } else if s == "true" || s == "false" {
        rf_events::FilterValue::Bool(s == "true")
    } else {
        rf_events::FilterValue::String(s.to_string())
    }
}

fn cmp_op_combo(ui: &mut egui::Ui, id_salt: &str, op: &mut rf_events::CmpOp) {
    use rf_events::CmpOp;
    let display = match op {
        CmpOp::Gt => "greater than (>)",
        CmpOp::Gte => "at least (>=)",
        CmpOp::Lt => "less than (<)",
        CmpOp::Lte => "at most (<=)",
        CmpOp::Eq => "equals (=)",
        CmpOp::Ne => "not equals (!=)",
    };
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(display)
        .width(130.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(op, CmpOp::Gte, "at least (>=)");
            ui.selectable_value(op, CmpOp::Gt, "greater than (>)");
            ui.selectable_value(op, CmpOp::Lte, "at most (<=)");
            ui.selectable_value(op, CmpOp::Lt, "less than (<)");
            ui.selectable_value(op, CmpOp::Eq, "equals (=)");
            ui.selectable_value(op, CmpOp::Ne, "not equals (!=)");
        });
}

/// Get value suggestions for a given field, merging known values with DB facets.
fn value_suggestions(field_idx: usize, facets: &Facets) -> Vec<String> {
    let field_entry = FILTER_FIELDS.get(field_idx);
    let field_name = field_entry.map(|e| e.0).unwrap_or("");

    // Start with known values for this field type
    let mut suggestions: Vec<String> = match field_name {
        "Event Type" => KNOWN_EVENT_TYPES.iter().map(|s| s.to_string()).collect(),
        "Band" => KNOWN_BANDS.iter().map(|s| s.to_string()).collect(),
        "Classification" => KNOWN_CLASSIFICATIONS.iter().map(|s| s.to_string()).collect(),
        "Severity" => SEV_CHOICES.iter().map(|(v, l)| format!("{v} ({l})")).collect(),
        "Source" => SRC_CHOICES.iter().map(|(v, l)| format!("{v} ({l})")).collect(),
        "Encrypted" => vec!["true".into(), "false".into()],
        _ => Vec::new(),
    };

    // Merge DB facets for matching fields
    let facet_key = match field_name {
        "Event Type" => Some("event_type"),
        "Severity" => Some("severity"),
        "Band" => Some("band"),
        "Source" => Some("source"),
        _ => None,
    };
    if let Some(key) = facet_key {
        if let Some((_, fvals)) = facets.iter().find(|(k, _)| k == key) {
            for (val, _count) in fvals {
                if !suggestions.iter().any(|s| s == val || s.starts_with(val.as_str())) {
                    suggestions.push(val.clone());
                }
            }
        }
    }

    suggestions
}

/// Show a value input with suggestion dropdown for the given field.
fn value_input_with_suggestions(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut String,
    field_idx: usize,
    facets: &Facets,
) -> bool {
    let hint = FILTER_FIELDS.get(field_idx).map(|e| e.2).unwrap_or("value");
    let suggestions = value_suggestions(field_idx, facets);
    let mut changed = false;

    if suggestions.is_empty() {
        // Plain text input
        changed = ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(170.0)
                .hint_text(hint)
                .font(egui::FontId::monospace(FONT_SIZE_DATA)),
        ).changed();
    } else {
        // Text input with suggestion combo
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;

            changed = ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(130.0)
                    .hint_text(hint)
                    .font(egui::FontId::monospace(FONT_SIZE_DATA)),
            ).changed();

            // Filtered suggestions matching current input
            let filtered: Vec<&String> = if value.is_empty() {
                suggestions.iter().take(15).collect()
            } else {
                let lower = value.to_lowercase();
                suggestions.iter()
                    .filter(|s| s.to_lowercase().contains(&lower))
                    .take(15)
                    .collect()
            };

            if !filtered.is_empty() {
                egui::ComboBox::from_id_salt(id_salt)
                    .selected_text("")
                    .width(30.0)
                    .show_ui(ui, |ui| {
                        for s in &filtered {
                            // Strip "(LABEL)" suffix for severity/source display values
                            let pick_val = if s.contains(" (") {
                                s.split(" (").next().unwrap_or(s).to_string()
                            } else {
                                s.to_string()
                            };
                            if ui.selectable_label(false, s.as_str()).clicked() {
                                *value = pick_val;
                                changed = true;
                            }
                        }
                    });
            }
        });
    }

    changed
}

/// Editable filter list with human-readable labels and value suggestions.
fn show_filter_editor(
    ui: &mut egui::Ui,
    filters: &mut Vec<rf_events::Filter>,
    id_prefix: &str,
    facets: &Facets,
) {
    let mut parts: Vec<(u8, usize, String)> = filters.iter().map(|f| {
        let (op, field, val) = decompose_filter(f);
        let field_idx = FILTER_FIELDS.iter().position(|(_, ff, _)| {
            std::mem::discriminant(ff) == std::mem::discriminant(&field)
        }).unwrap_or(0);
        (op, field_idx, val)
    }).collect();

    let mut delete_idx: Option<usize> = None;
    let mut changed = false;

    if parts.is_empty() {
        ui.label(
            egui::RichText::new("  No filters — matches all events. Click + to narrow.")
                .size(FONT_SIZE_DATA)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
    }

    for (i, (op, field_idx, val)) in parts.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // "where" label for first, "and" for rest
            let prefix = if i == 0 { "where" } else { "  and" };
            ui.label(
                egui::RichText::new(prefix)
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );

            // Field selector (human names)
            let prev_field = *field_idx;
            egui::ComboBox::from_id_salt(format!("{id_prefix}_field_{i}"))
                .selected_text(FILTER_FIELDS.get(*field_idx).map(|f| f.0).unwrap_or("?"))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (idx, &(label, _, _)) in FILTER_FIELDS.iter().enumerate() {
                        ui.selectable_value(field_idx, idx, label);
                    }
                });
            if *field_idx != prev_field { changed = true; }

            // Op selector (human names)
            let prev_op = *op;
            let op_display = FILTER_OPS.get(*op as usize).map(|o| o.0).unwrap_or("equals");
            egui::ComboBox::from_id_salt(format!("{id_prefix}_op_{i}"))
                .selected_text(op_display)
                .width(120.0)
                .show_ui(ui, |ui| {
                    for &(label, _short, idx) in FILTER_OPS {
                        ui.selectable_value(op, idx, label);
                    }
                });
            if *op != prev_op { changed = true; }

            // Value with suggestions
            if value_input_with_suggestions(
                ui,
                &format!("{id_prefix}_sug_{i}"),
                val,
                *field_idx,
                facets,
            ) {
                changed = true;
            }

            // Delete
            if ui.small_button("\u{2716}").on_hover_text("Remove filter").clicked() {
                delete_idx = Some(i);
            }
        });
    }

    if ui.small_button("+ Add Filter").clicked() {
        parts.push((0, 0, String::new()));
        changed = true;
    }

    if let Some(idx) = delete_idx {
        parts.remove(idx);
        changed = true;
    }

    if changed {
        *filters = parts.iter().map(|(op, field_idx, val)| {
            let field = FILTER_FIELDS.get(*field_idx)
                .map(|(_, f, _)| f.clone())
                .unwrap_or(rf_events::Field::EventType);
            recompose_filter(*op, field, val)
        }).collect();
    }
}

/// Generate a natural-language summary of what a rule does.
fn alert_rule_summary(rule: &AlertRule) -> String {
    let cond_text = match &rule.condition {
        rf_events::AlertCondition::Threshold { op, value, window_sec } => {
            let op_text = match op {
                rf_events::CmpOp::Gt => "more than",
                rf_events::CmpOp::Gte => "at least",
                rf_events::CmpOp::Lt => "fewer than",
                rf_events::CmpOp::Lte => "at most",
                rf_events::CmpOp::Eq => "exactly",
                rf_events::CmpOp::Ne => "not exactly",
            };
            format!("{op_text} {value:.0} matching events within {}", format_duration(*window_sec))
        }
        rf_events::AlertCondition::Absence { window_sec } =>
            format!("no matching events for {}", format_duration(*window_sec)),
        rf_events::AlertCondition::RateChange { percent, window_sec, baseline_sec } =>
            format!("event rate changes by more than {percent:.0}% (comparing {} window to {} baseline)",
                format_duration(*window_sec), format_duration(*baseline_sec)),
        rf_events::AlertCondition::FirstOccurrence =>
            "a matching event is seen for the first time".to_string(),
    };

    let filter_text = if rule.filter.is_empty() {
        "any event".to_string()
    } else {
        let parts: Vec<String> = rule.filter.iter().map(|f| {
            let (_, field, val) = decompose_filter(f);
            format!("{} is \"{}\"", field_display(&field), val)
        }).collect();
        parts.join(" AND ")
    };

    format!("Alert when {} — watching {}", cond_text, filter_text)
}

fn custom_rule_summary(rule: &CustomEventRule) -> String {
    let cond_text = match &rule.condition {
        rf_events::CustomEventCondition::Every => "every matching event".to_string(),
        rf_events::CustomEventCondition::Threshold { op, value, window_sec } => {
            let op_text = match op {
                rf_events::CmpOp::Gte => "at least",
                rf_events::CmpOp::Gt => "more than",
                _ => "threshold of",
            };
            format!("{op_text} {value:.0} matches in {}", format_duration(*window_sec))
        }
        rf_events::CustomEventCondition::Absence { window_sec } =>
            format!("no matches for {}", format_duration(*window_sec)),
        rf_events::CustomEventCondition::NewValue { field, lookback_sec } =>
            format!("a new {} value (not seen in {})", field_display(field), format_duration(*lookback_sec)),
        rf_events::CustomEventCondition::Cardinality { field, op, value, window_sec } => {
            let op_text = match op {
                rf_events::CmpOp::Gte => "at least",
                _ => "reaching",
            };
            format!("{op_text} {} distinct {} values in {}", value, field_display(field), format_duration(*window_sec))
        }
        rf_events::CustomEventCondition::RateChange { percent, .. } =>
            format!("rate changes by >{percent:.0}%"),
        rf_events::CustomEventCondition::Correlation { .. } =>
            "correlated events detected".to_string(),
    };

    format!("Create \"{}\" ({}) when {}", rule.event_type, rule.severity.label(), cond_text)
}

fn format_duration(secs: u64) -> String {
    if secs < 60 { format!("{secs}s") }
    else if secs < 3600 { format!("{}m", secs / 60) }
    else if secs < 86400 { format!("{}h", secs / 3600) }
    else { format!("{}d", secs / 86400) }
}

// ── Alert Presets ────────────────────────────────────────

fn alert_presets() -> Vec<(&'static str, AlertRule)> {
    use rf_events::*;
    vec![
        ("Encrypted Activity Burst", AlertRule {
            name: "Encrypted Activity Burst".into(),
            priority: AlertPriority::High,
            condition: AlertCondition::Threshold {
                op: CmpOp::Gte, value: 5.0, window_sec: 60,
            },
            filter: vec![Filter::Eq(Field::Encrypted, FilterValue::Bool(true))],
            cooldown_sec: 300,
            ..AlertRule::default()
        }),
        ("New Emitter Detected", AlertRule {
            name: "New Emitter Detected".into(),
            priority: AlertPriority::Medium,
            condition: AlertCondition::FirstOccurrence,
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("sigex.emitter.new".into()))],
            cooldown_sec: 60,
            ..AlertRule::default()
        }),
        ("SDR Disconnected", AlertRule {
            name: "SDR Disconnected".into(),
            priority: AlertPriority::Critical,
            condition: AlertCondition::FirstOccurrence,
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("system.sdr.disconnect".into()))],
            cooldown_sec: 10,
            ..AlertRule::default()
        }),
        ("Signal Lost (Dead Man)", AlertRule {
            name: "Signal Lost (No Spectrum)".into(),
            priority: AlertPriority::High,
            condition: AlertCondition::Absence { window_sec: 30 },
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("spectrum.detect".into()))],
            cooldown_sec: 60,
            ..AlertRule::default()
        }),
        ("Federal Activity Detected", AlertRule {
            name: "Federal Activity Detected".into(),
            priority: AlertPriority::High,
            condition: AlertCondition::FirstOccurrence,
            filter: vec![Filter::Eq(Field::Classification, FilterValue::String("FEDL".into()))],
            cooldown_sec: 120,
            ..AlertRule::default()
        }),
        ("P25 Channel Grant Spike", AlertRule {
            name: "P25 Grant Spike".into(),
            priority: AlertPriority::Medium,
            condition: AlertCondition::Threshold {
                op: CmpOp::Gte, value: 20.0, window_sec: 60,
            },
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("protocol.p25.grant".into()))],
            cooldown_sec: 300,
            ..AlertRule::default()
        }),
        ("Crypto Key Rotation", AlertRule {
            name: "Crypto Key Rotation".into(),
            priority: AlertPriority::High,
            condition: AlertCondition::FirstOccurrence,
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("sigex.crypto.rotation".into()))],
            cooldown_sec: 60,
            ..AlertRule::default()
        }),
        ("High Event Rate", AlertRule {
            name: "High Event Rate".into(),
            priority: AlertPriority::Medium,
            condition: AlertCondition::RateChange {
                percent: 300.0, window_sec: 60, baseline_sec: 3600,
            },
            filter: Vec::new(),
            cooldown_sec: 600,
            ..AlertRule::default()
        }),
    ]
}

fn custom_rule_presets() -> Vec<(&'static str, CustomEventRule)> {
    use rf_events::*;
    vec![
        ("Federal Burst Detector", CustomEventRule {
            name: "Federal Burst Detector".into(),
            event_type: "custom.fedl_burst".into(),
            description: "Detects bursts of federal-classified activity".into(),
            severity: event::Severity::Warn,
            condition: CustomEventCondition::Threshold {
                op: CmpOp::Gte, value: 3.0, window_sec: 120,
            },
            filter: vec![Filter::Eq(Field::Classification, FilterValue::String("FEDL".into()))],
            body_template: "{{count}} federal signals on {{freqs}} in {{bands}}".into(),
            cooldown_sec: 300,
            ..CustomEventRule::default()
        }),
        ("New Talkgroup Spotter", CustomEventRule {
            name: "New Talkgroup Spotter".into(),
            event_type: "custom.new_talkgroup".into(),
            description: "Fires when a talkgroup not seen in 24h appears".into(),
            severity: event::Severity::Notice,
            condition: CustomEventCondition::NewValue {
                field: Field::Talkgroup, lookback_sec: 86400,
            },
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("protocol.p25.grant".into()))],
            body_template: "New talkgroup activity: TG {{talkgroups}}".into(),
            cooldown_sec: 60,
            ..CustomEventRule::default()
        }),
        ("Multi-Band Radio Unit", CustomEventRule {
            name: "Multi-Band Radio Unit".into(),
            event_type: "custom.multiband_uid".into(),
            description: "Same UID seen across 3+ bands in 10 minutes".into(),
            severity: event::Severity::Warn,
            condition: CustomEventCondition::Cardinality {
                field: Field::Band, op: CmpOp::Gte, value: 3, window_sec: 600,
            },
            filter: vec![Filter::Exists(Field::SourceUnit)],
            body_template: "UID {{uids}} active on {{count}} bands: {{bands}}".into(),
            cooldown_sec: 600,
            ..CustomEventRule::default()
        }),
        ("Encrypted Traffic Tagger", CustomEventRule {
            name: "Encrypted Traffic Tagger".into(),
            event_type: "custom.encrypted_voice".into(),
            description: "Tags every encrypted P25 voice event".into(),
            severity: event::Severity::Info,
            condition: CustomEventCondition::Every,
            filter: vec![
                Filter::Eq(Field::EventType, FilterValue::String("protocol.p25.voice".into())),
                Filter::Eq(Field::Encrypted, FilterValue::Bool(true)),
            ],
            body_template: "Encrypted voice on TG {{talkgroups}} at {{freqs}}".into(),
            cooldown_sec: 0,
            ..CustomEventRule::default()
        }),
        ("Spectrum Silence Alert", CustomEventRule {
            name: "Spectrum Silence Alert".into(),
            event_type: "custom.spectrum_silence".into(),
            description: "No spectrum detections for 60 seconds".into(),
            severity: event::Severity::Error,
            condition: CustomEventCondition::Absence { window_sec: 60 },
            filter: vec![Filter::Eq(Field::EventType, FilterValue::String("spectrum.detect".into()))],
            body_template: "No spectrum activity detected for 60s".into(),
            cooldown_sec: 120,
            ..CustomEventRule::default()
        }),
    ]
}

// ── Alert Rule Editor ────────────────────────────────────

const ALERT_COND_TYPES: &[(&str, &str)] = &[
    ("Threshold",        "Alert when event count crosses a limit"),
    ("Absence",          "Alert when events stop arriving (dead man)"),
    ("Rate Change",      "Alert on sudden rate increase/decrease"),
    ("First Occurrence", "Alert on first matching event ever"),
];

fn alert_cond_type_index(c: &rf_events::AlertCondition) -> usize {
    match c {
        rf_events::AlertCondition::Threshold { .. } => 0,
        rf_events::AlertCondition::Absence { .. } => 1,
        rf_events::AlertCondition::RateChange { .. } => 2,
        rf_events::AlertCondition::FirstOccurrence => 3,
    }
}

fn show_alert_rule_editor(ui: &mut egui::Ui, rule: &mut AlertRule, facets: &Facets) -> bool {
    let mut save = false;

    egui::Frame::new()
        .fill(BG_ELEVATED)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            // ── Natural language summary ──
            let summary = alert_rule_summary(rule);
            ui.label(
                egui::RichText::new(&summary)
                    .size(FONT_SIZE_DATA)
                    .color(AMBER_WARNING)
                    .family(egui::FontFamily::Monospace)
                    .italics(),
            );
            ui.add_space(6.0);

            // ── Row 1: Name, Priority, Cooldown ──
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                editor_label(ui, "Name");
                ui.add(
                    egui::TextEdit::singleline(&mut rule.name)
                        .desired_width(200.0)
                        .font(egui::FontId::monospace(FONT_SIZE_DATA)),
                );
                editor_label(ui, "Priority");
                egui::ComboBox::from_id_salt(format!("alert_pri_{}", rule.id))
                    .selected_text(rule.priority.label())
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        use rf_events::AlertPriority;
                        for p in &[AlertPriority::Low, AlertPriority::Medium, AlertPriority::High, AlertPriority::Critical] {
                            ui.selectable_value(&mut rule.priority, *p, p.label());
                        }
                    });
                editor_label(ui, "Cooldown");
                let mut cd = rule.cooldown_sec as i64;
                if ui.add(egui::DragValue::new(&mut cd).range(0..=86400).suffix("s").speed(1.0)).changed() {
                    rule.cooldown_sec = cd.max(0) as u64;
                }
            });

            ui.add_space(6.0);

            // ── Trigger Condition ──
            ui.label(
                egui::RichText::new("WHEN (trigger condition)")
                    .size(FONT_SIZE_HUD)
                    .color(RED_WATCHDOG)
                    .family(egui::FontFamily::Monospace),
            );

            // Condition type with description
            let mut cond_idx = alert_cond_type_index(&rule.condition);
            let prev_idx = cond_idx;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                egui::ComboBox::from_id_salt(format!("alert_ctype_{}", rule.id))
                    .selected_text(ALERT_COND_TYPES[cond_idx].0)
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for (i, &(name, desc)) in ALERT_COND_TYPES.iter().enumerate() {
                            let r = ui.selectable_value(&mut cond_idx, i, name);
                            if r.hovered() {
                                r.on_hover_text(desc);
                            }
                        }
                    });

                // Show description of current type
                ui.label(
                    egui::RichText::new(ALERT_COND_TYPES[cond_idx].1)
                        .size(FONT_SIZE_HUD)
                        .color(TEXT_SECONDARY)
                        .family(egui::FontFamily::Monospace),
                );
            });

            if cond_idx != prev_idx {
                rule.condition = match cond_idx {
                    0 => rf_events::AlertCondition::Threshold {
                        op: rf_events::CmpOp::Gte, value: 5.0, window_sec: 60,
                    },
                    1 => rf_events::AlertCondition::Absence { window_sec: 30 },
                    2 => rf_events::AlertCondition::RateChange {
                        percent: 200.0, window_sec: 60, baseline_sec: 3600,
                    },
                    _ => rf_events::AlertCondition::FirstOccurrence,
                };
            }

            // Condition parameters
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(16.0); // indent
                match &mut rule.condition {
                    rf_events::AlertCondition::Threshold { op, value, window_sec } => {
                        editor_label(ui, "Event count is");
                        cmp_op_combo(ui, &format!("alert_cop_{}", rule.id), op);
                        let mut v = *value;
                        ui.add(egui::DragValue::new(&mut v).range(0.0..=1e9).speed(0.1));
                        *value = v;
                        editor_label(ui, "within");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                    }
                    rf_events::AlertCondition::Absence { window_sec } => {
                        editor_label(ui, "No events arrive for");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                    }
                    rf_events::AlertCondition::RateChange { percent, window_sec, baseline_sec } => {
                        editor_label(ui, "Rate changes by more than");
                        let mut p = *percent;
                        ui.add(egui::DragValue::new(&mut p).range(1.0..=10000.0).suffix("%").speed(1.0));
                        *percent = p;
                        editor_label(ui, "comparing");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                        editor_label(ui, "to baseline of");
                        let mut b = *baseline_sec as i64;
                        ui.add(egui::DragValue::new(&mut b).range(1..=604800).suffix("s").speed(1.0));
                        *baseline_sec = b.max(1) as u64;
                    }
                    rf_events::AlertCondition::FirstOccurrence => {
                        editor_label(ui, "Fires once when a matching event first appears");
                    }
                }
            });

            ui.add_space(6.0);

            // ── Filter Section ──
            ui.label(
                egui::RichText::new("MATCH (which events to watch)")
                    .size(FONT_SIZE_HUD)
                    .color(RED_WATCHDOG)
                    .family(egui::FontFamily::Monospace),
            );

            show_filter_editor(ui, &mut rule.filter, &format!("af_{}", rule.id), facets);

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save Rule").clicked() {
                    save = true;
                }
            });
        });

    save
}

// ── Custom Event Rule Editor ─────────────────────────────

const CUSTOM_COND_TYPES: &[(&str, &str)] = &[
    ("Every Match",   "Create an event for every matching input"),
    ("Threshold",     "Fire when match count crosses a limit"),
    ("Absence",       "Fire when events stop arriving"),
    ("New Value",     "Fire when an unseen field value appears"),
    ("Cardinality",   "Fire on N distinct values of a field"),
    ("Rate Change",   "Fire on sudden rate change"),
];

fn custom_cond_type_index(c: &rf_events::CustomEventCondition) -> usize {
    match c {
        rf_events::CustomEventCondition::Every => 0,
        rf_events::CustomEventCondition::Threshold { .. } => 1,
        rf_events::CustomEventCondition::Absence { .. } => 2,
        rf_events::CustomEventCondition::NewValue { .. } => 3,
        rf_events::CustomEventCondition::Cardinality { .. } => 4,
        rf_events::CustomEventCondition::RateChange { .. } => 5,
        rf_events::CustomEventCondition::Correlation { .. } => 5,
    }
}

fn show_custom_rule_editor(ui: &mut egui::Ui, rule: &mut CustomEventRule, facets: &Facets) -> bool {
    let mut save = false;

    egui::Frame::new()
        .fill(BG_ELEVATED)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            // ── Natural language summary ──
            let summary = custom_rule_summary(rule);
            ui.label(
                egui::RichText::new(&summary)
                    .size(FONT_SIZE_DATA)
                    .color(AMBER_WARNING)
                    .family(egui::FontFamily::Monospace)
                    .italics(),
            );
            ui.add_space(6.0);

            // ── Row 1: Name, Output Event Type, Cooldown ──
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                editor_label(ui, "Name");
                ui.add(
                    egui::TextEdit::singleline(&mut rule.name)
                        .desired_width(180.0)
                        .font(egui::FontId::monospace(FONT_SIZE_DATA)),
                );
                editor_label(ui, "Produces event type");
                ui.add(
                    egui::TextEdit::singleline(&mut rule.event_type)
                        .desired_width(160.0)
                        .hint_text("custom.my_event")
                        .font(egui::FontId::monospace(FONT_SIZE_DATA)),
                );
                editor_label(ui, "Cooldown");
                let mut cd = rule.cooldown_sec as i64;
                if ui.add(egui::DragValue::new(&mut cd).range(0..=86400).suffix("s").speed(1.0)).changed() {
                    rule.cooldown_sec = cd.max(0) as u64;
                }
            });

            // ── Row 2: Severity, Description ──
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                editor_label(ui, "Output severity");
                egui::ComboBox::from_id_salt(format!("csev_{}", rule.id))
                    .selected_text(rule.severity.label())
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        use rf_events::event::Severity;
                        for s in &[Severity::Trace, Severity::Debug, Severity::Info, Severity::Notice, Severity::Warn, Severity::Error, Severity::Fatal] {
                            ui.selectable_value(&mut rule.severity, *s, s.label());
                        }
                    });
                editor_label(ui, "Description");
                ui.add(
                    egui::TextEdit::singleline(&mut rule.description)
                        .desired_width(350.0)
                        .hint_text("What does this rule detect?")
                        .font(egui::FontId::monospace(FONT_SIZE_DATA)),
                );
            });

            ui.add_space(6.0);

            // ── Trigger Condition ──
            ui.label(
                egui::RichText::new("WHEN (trigger condition)")
                    .size(FONT_SIZE_HUD)
                    .color(MAGENTA_EXPLOIT)
                    .family(egui::FontFamily::Monospace),
            );

            let mut cond_idx = custom_cond_type_index(&rule.condition);
            let prev_idx = cond_idx;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                egui::ComboBox::from_id_salt(format!("cctype_{}", rule.id))
                    .selected_text(CUSTOM_COND_TYPES.get(cond_idx).map(|c| c.0).unwrap_or("?"))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for (i, &(name, _)) in CUSTOM_COND_TYPES.iter().enumerate() {
                            ui.selectable_value(&mut cond_idx, i, name);
                        }
                    });
                if let Some(&(_, desc)) = CUSTOM_COND_TYPES.get(cond_idx) {
                    ui.label(
                        egui::RichText::new(desc)
                            .size(FONT_SIZE_HUD)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );
                }
            });

            if cond_idx != prev_idx {
                rule.condition = match cond_idx {
                    0 => rf_events::CustomEventCondition::Every,
                    1 => rf_events::CustomEventCondition::Threshold {
                        op: rf_events::CmpOp::Gte, value: 5.0, window_sec: 60,
                    },
                    2 => rf_events::CustomEventCondition::Absence { window_sec: 30 },
                    3 => rf_events::CustomEventCondition::NewValue {
                        field: rf_events::Field::Talkgroup, lookback_sec: 86400,
                    },
                    4 => rf_events::CustomEventCondition::Cardinality {
                        field: rf_events::Field::Band, op: rf_events::CmpOp::Gte,
                        value: 3, window_sec: 600,
                    },
                    _ => rf_events::CustomEventCondition::RateChange {
                        percent: 200.0, window_sec: 60, baseline_sec: 3600,
                    },
                };
            }

            // Condition parameters
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(16.0);
                match &mut rule.condition {
                    rf_events::CustomEventCondition::Every => {
                        editor_label(ui, "Every matching input event produces an output event");
                    }
                    rf_events::CustomEventCondition::Threshold { op, value, window_sec } => {
                        editor_label(ui, "Match count is");
                        cmp_op_combo(ui, &format!("ccop_{}", rule.id), op);
                        let mut v = *value;
                        ui.add(egui::DragValue::new(&mut v).range(0.0..=1e9).speed(0.1));
                        *value = v;
                        editor_label(ui, "within");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                    }
                    rf_events::CustomEventCondition::Absence { window_sec } => {
                        editor_label(ui, "No matching events for");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                    }
                    rf_events::CustomEventCondition::NewValue { field, lookback_sec } => {
                        editor_label(ui, "A new value of");
                        let mut fidx = FILTER_FIELDS.iter().position(|(_, ff, _)| {
                            std::mem::discriminant(ff) == std::mem::discriminant(field)
                        }).unwrap_or(0);
                        egui::ComboBox::from_id_salt(format!("cnvf_{}", rule.id))
                            .selected_text(field_display(field))
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for (i, &(label, _, _)) in FILTER_FIELDS.iter().enumerate() {
                                    ui.selectable_value(&mut fidx, i, label);
                                }
                            });
                        if let Some(&(_, ref f, _)) = FILTER_FIELDS.get(fidx) {
                            *field = f.clone();
                        }
                        editor_label(ui, "not seen in the last");
                        let mut w = *lookback_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=604800).suffix("s").speed(1.0));
                        *lookback_sec = w.max(1) as u64;
                    }
                    rf_events::CustomEventCondition::Cardinality { field, op, value, window_sec } => {
                        let mut fidx = FILTER_FIELDS.iter().position(|(_, ff, _)| {
                            std::mem::discriminant(ff) == std::mem::discriminant(field)
                        }).unwrap_or(0);
                        editor_label(ui, "Distinct values of");
                        egui::ComboBox::from_id_salt(format!("ccdf_{}", rule.id))
                            .selected_text(field_display(field))
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for (i, &(label, _, _)) in FILTER_FIELDS.iter().enumerate() {
                                    ui.selectable_value(&mut fidx, i, label);
                                }
                            });
                        if let Some(&(_, ref f, _)) = FILTER_FIELDS.get(fidx) {
                            *field = f.clone();
                        }
                        editor_label(ui, "is");
                        cmp_op_combo(ui, &format!("ccdop_{}", rule.id), op);
                        let mut v = *value as i64;
                        ui.add(egui::DragValue::new(&mut v).range(1..=10000));
                        *value = v.max(1) as u64;
                        editor_label(ui, "in");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                    }
                    rf_events::CustomEventCondition::RateChange { percent, window_sec, baseline_sec } => {
                        editor_label(ui, "Rate changes by more than");
                        let mut p = *percent;
                        ui.add(egui::DragValue::new(&mut p).range(1.0..=10000.0).suffix("%").speed(1.0));
                        *percent = p;
                        editor_label(ui, "comparing");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                        editor_label(ui, "to baseline");
                        let mut b = *baseline_sec as i64;
                        ui.add(egui::DragValue::new(&mut b).range(1..=604800).suffix("s").speed(1.0));
                        *baseline_sec = b.max(1) as u64;
                    }
                    rf_events::CustomEventCondition::Correlation { window_sec, .. } => {
                        editor_label(ui, "Correlation window");
                        let mut w = *window_sec as i64;
                        ui.add(egui::DragValue::new(&mut w).range(1..=86400).suffix("s").speed(1.0));
                        *window_sec = w.max(1) as u64;
                    }
                }
            });

            ui.add_space(6.0);

            // ── Filters ──
            ui.label(
                egui::RichText::new("MATCH (which events to watch)")
                    .size(FONT_SIZE_HUD)
                    .color(MAGENTA_EXPLOIT)
                    .family(egui::FontFamily::Monospace),
            );

            show_filter_editor(ui, &mut rule.filter, &format!("cf_{}", rule.id), facets);

            ui.add_space(6.0);

            // ── Output Template ──
            ui.label(
                egui::RichText::new("OUTPUT (produced event)")
                    .size(FONT_SIZE_HUD)
                    .color(MAGENTA_EXPLOIT)
                    .family(egui::FontFamily::Monospace),
            );
            ui.horizontal(|ui| {
                editor_label(ui, "Body template");
                ui.add(
                    egui::TextEdit::singleline(&mut rule.body_template)
                        .desired_width(400.0)
                        .hint_text("{{count}} events on {{freqs}} in {{bands}}")
                        .font(egui::FontId::monospace(FONT_SIZE_DATA)),
                );
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                editor_label(ui, "Placeholders:");
                for ph in &["{{count}}", "{{freqs}}", "{{talkgroups}}", "{{uids}}", "{{bands}}"] {
                    if ui.small_button(*ph).on_hover_text("Click to insert").clicked() {
                        rule.body_template.push_str(ph);
                    }
                }
            });

            // Options
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.checkbox(&mut rule.include_source_events, "Include source event IDs");
                editor_label(ui, "Chain depth");
                let mut cd = rule.chain_depth as i64;
                if ui.add(egui::DragValue::new(&mut cd).range(0..=10)).changed() {
                    rule.chain_depth = cd.max(0) as u32;
                }
                editor_label(ui, "Max");
                let mut mcd = rule.max_chain_depth as i64;
                if ui.add(egui::DragValue::new(&mut mcd).range(0..=10)).changed() {
                    rule.max_chain_depth = mcd.max(0) as u32;
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save Rule").clicked() {
                    save = true;
                }
            });
        });

    save
}

// ── Sweep Tab ────────────────────────────────────────────────

fn show_sweep(ui: &mut egui::Ui, _db: &rf_db::Db, ws: &mut WatchdogState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("TSCM SWEEP")
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );
        ui.add_space(4.0);

        // Active sweep progress bar
        if ws.sweep_active {
            let now = ui.ctx().input(|i| i.time);
            let elapsed = (now - ws.sweep_start_time).max(0.0);
            let progress = (elapsed / ws.sweep_duration_sec).clamp(0.0, 1.0) as f32;
            let elapsed_m = (elapsed as u32) / 60;
            let elapsed_s = (elapsed as u32) % 60;
            let total_m = (ws.sweep_duration_sec as u32) / 60;
            let total_s = (ws.sweep_duration_sec as u32) % 60;

            egui::Frame::NONE
                .inner_margin(egui::Margin::same(8))
                .fill(BG_ELEVATED)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("ACTIVE:")
                            .color(RED_WATCHDOG).size(FONT_SIZE_DATA).strong());
                        ui.label(egui::RichText::new(&ws.sweep_protocol)
                            .color(AMBER_WARNING).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));
                        ui.label(egui::RichText::new(format!("{}:{:02}/{}:{:02}", elapsed_m, elapsed_s, total_m, total_s))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        if ui.add(egui::Button::new(
                            egui::RichText::new("CANCEL").color(RED_WATCHDOG).size(FONT_SIZE_DATA)
                        )).clicked() {
                            ws.sweep_active = false;
                            ws.sweep_protocol.clear();
                        }
                    });

                    // Progress bar
                    let (bar_rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 8.0),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(bar_rect);
                    painter.rect_filled(bar_rect, 2.0, BG_SURFACE);
                    let fill_rect = egui::Rect::from_min_size(
                        bar_rect.left_top(),
                        egui::vec2(bar_rect.width() * progress, bar_rect.height()),
                    );
                    let bar_color = if progress > 0.9 { GREEN_COLLECT } else { AMBER_WARNING };
                    painter.rect_filled(fill_rect, 2.0, bar_color);
                });

            // Auto-complete when time is up
            if elapsed >= ws.sweep_duration_sec {
                ws.sweep_active = false;
                ws.sweep_protocol.clear();
            }

            ui.ctx().request_repaint();
            ui.add_space(8.0);
        }

        ui.label(
            egui::RichText::new("Counter-surveillance sweep protocols")
                .color(TEXT_SECONDARY)
                .size(FONT_SIZE_DATA),
        );
        ui.add_space(12.0);

        // Sweep protocol cards with enabled Start buttons
        let protocols: &[(&str, &str, f64, &str)] = &[
            ("VEHICLE_QUICK", "5 min", 300.0, "Fast vehicle sweep: common tracker bands, FM/LTE/GPS"),
            ("VEHICLE_THOROUGH", "15 min", 900.0, "Full vehicle sweep: 70-1700 MHz, all bands, nearfield"),
            ("ROOM_SWEEP", "20 min", 1200.0, "Indoor sweep: close-range, extended dwell, all modulations"),
        ];
        for (name, duration, dur_sec, desc) in protocols {
            egui::Frame::new()
                .fill(BG_ELEVATED)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        ui.label(
                            egui::RichText::new(*name)
                                .size(FONT_SIZE_DATA)
                                .color(RED_WATCHDOG)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(*duration)
                                .size(FONT_SIZE_HUD)
                                .color(AMBER_WARNING)
                                .family(egui::FontFamily::Monospace),
                        );
                        let can_start = !ws.sweep_active;
                        if ui.add_enabled(can_start, egui::Button::new(
                            egui::RichText::new("Start").color(if can_start { GREEN_COLLECT } else { TEXT_SECONDARY })
                        )).clicked() {
                            ws.sweep_active = true;
                            ws.sweep_protocol = name.to_string();
                            ws.sweep_start_time = ui.ctx().input(|i| i.time);
                            ws.sweep_duration_sec = *dur_sec;
                        }
                    });
                    ui.label(
                        egui::RichText::new(*desc)
                            .size(FONT_SIZE_HUD)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );
                });
            ui.add_space(4.0);
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Note: sweep timer only — no SDR control until Phase 2 TSCM hardware integration")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // Nearfield mode controls (disabled preview)
        ui.label(
            egui::RichText::new("NEARFIELD SWEEP")
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label(
                egui::RichText::new("Range: 70 — 1700 MHz")
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
            ui.label(
                egui::RichText::new("Dwell: 500ms/segment")
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
            ui.label(
                egui::RichText::new("Est: ~185s full sweep")
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Requires: dedicated SDR in NEARFIELD_SWEEP role (Phase 2)")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // Dual-antenna differential (preview)
        ui.label(
            egui::RichText::new("DIFFERENTIAL ANALYSIS")
                .size(FONT_SIZE_HEADER)
                .color(RED_WATCHDOG)
                .family(egui::FontFamily::Monospace),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("External vs internal antenna power comparison")
                .size(FONT_SIZE_DATA)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
        ui.label(
            egui::RichText::new("delta > -6dB = nearfield candidate | delta > 0dB = very nearfield")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
        ui.label(
            egui::RichText::new("Requires: 2 SDRs + antenna calibration (Phase 2)")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
    });
}

// ── Baseline Tab ─────────────────────────────────────────────

fn show_baseline(ui: &mut egui::Ui, db: &rf_db::Db, ws: &mut WatchdogState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        // 1. Baseline status + controls
        show_baseline_status(ui, db, ws);

        ui.add_space(8.0);
        ui.separator();

        // 2. Baseline profiles (hour×day grid)
        show_baseline_profiles(ui, ws);

        ui.add_space(8.0);
        ui.separator();

        // 3. Anomaly events
        show_anomaly_events(ui, ws);
    });
}

fn baseline_section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(text)
            .size(FONT_SIZE_HEADER)
            .color(RED_WATCHDOG)
            .family(egui::FontFamily::Monospace),
    );
    ui.add_space(4.0);
}

// ── Baseline Status & Controls ───────────────────────────

fn show_baseline_status(ui: &mut egui::Ui, db: &rf_db::Db, ws: &mut WatchdogState) {
    baseline_section_header(ui, "RF ENVIRONMENT BASELINE");

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(8))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            // Status summary
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Baseline entries:")
                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(format!("{}", ws.baseline_count))
                    .color(if ws.baseline_count > 0 { GREEN_COLLECT } else { TEXT_SECONDARY })
                    .size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));

                ui.add_space(16.0);

                ui.label(egui::RichText::new("Last computed:")
                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                let last = ws.baseline_last_computed.as_deref()
                    .map(|s| s.get(..16).unwrap_or(s))
                    .unwrap_or("never");
                ui.label(egui::RichText::new(last)
                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
            });

            ui.add_space(8.0);

            // Profile name input + capture
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Profile:")
                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ws.baseline_new_profile_name)
                    .desired_width(120.0)
                    .hint_text("default"));

                let capture_enabled = !ws.baseline_computing;
                if ui.add_enabled(capture_enabled, egui::Button::new(
                    egui::RichText::new("CAPTURE SNAPSHOT")
                        .color(if capture_enabled { GREEN_COLLECT } else { TEXT_SECONDARY }).size(FONT_SIZE_DATA)
                )).clicked() {
                    let name = if ws.baseline_new_profile_name.trim().is_empty() {
                        "default"
                    } else {
                        ws.baseline_new_profile_name.trim()
                    };
                    let profile_display = name.to_string();
                    match db.compute_baseline_snapshot_named(name) {
                        Ok(n) => {
                            ws.baseline_computing = false;
                            ws.force_refresh = true;
                            let msg = format!("Captured {} baseline entries for profile '{}'", n, profile_display);
                            tracing::info!("{}", msg);
                            ws.baseline_feedback = Some((msg, true));
                            ws.baseline_feedback_time = ui.ctx().input(|i| i.time);
                        }
                        Err(e) => {
                            ws.baseline_computing = false;
                            let msg = format!("Baseline compute failed: {}", e);
                            tracing::error!("{}", msg);
                            ws.baseline_feedback = Some((msg, false));
                            ws.baseline_feedback_time = ui.ctx().input(|i| i.time);
                        }
                    }
                }

                ui.add_space(8.0);

                if ws.baseline_count > 0 {
                    if ui.add(egui::Button::new(
                        egui::RichText::new("CLEAR BASELINES")
                            .color(RED_WATCHDOG).size(FONT_SIZE_DATA)
                    )).clicked() {
                        let _ = db.clear_baselines();
                        ws.force_refresh = true;
                        ws.baseline_feedback = Some(("Baselines cleared.".to_string(), true));
                        ws.baseline_feedback_time = ui.ctx().input(|i| i.time);
                    }
                }

                if ws.baseline_computing {
                    // Spinning indicator
                    let t = ui.ctx().input(|i| i.time);
                    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                    let idx = (t * 10.0) as usize % spinner.len();
                    ui.label(egui::RichText::new(format!("{} Computing...", spinner[idx]))
                        .color(AMBER_WARNING).size(FONT_SIZE_DATA));
                    ui.ctx().request_repaint();
                }
            });

            // Profile filter
            if !ws.baseline_profiles.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("View profile:")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    let current = ws.baseline_active_profile.clone();
                    egui::ComboBox::from_id_salt("baseline_profile_select")
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut ws.baseline_active_profile, "all".to_string(), "ALL");
                            for p in &ws.baseline_profiles {
                                ui.selectable_value(&mut ws.baseline_active_profile, p.clone(), p);
                            }
                        });
                });
            }

            // Feedback message (auto-dismiss after 8 seconds)
            if let Some((ref msg, is_success)) = ws.baseline_feedback {
                let now = ui.ctx().input(|i| i.time);
                if now - ws.baseline_feedback_time < 8.0 {
                    ui.add_space(4.0);
                    let color = if is_success { GREEN_COLLECT } else { RED_WATCHDOG };
                    let prefix = if is_success { "OK" } else { "ERR" };
                    ui.label(egui::RichText::new(format!("[{}] {}", prefix, msg))
                        .color(color).size(FONT_SIZE_DATA));
                }
            }

            ui.add_space(4.0);
            ui.label(egui::RichText::new(
                "Snapshot computes hour-of-week activity profiles from traffic sessions (last 30 days). Name profiles for comparison (e.g. weekday, weekend, event)."
            ).color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
        });
}

// ── Baseline Profiles ────────────────────────────────────

fn show_baseline_profiles(ui: &mut egui::Ui, ws: &WatchdogState) {
    baseline_section_header(ui, &format!("BASELINE PROFILES ({} entries)", ws.baselines.len()));

    if ws.baselines.is_empty() {
        ui.label(egui::RichText::new("No baseline data. Capture a snapshot to populate.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Filter by active profile
    let filtered: Vec<&rf_db::ActivityBaseline> = ws.baselines.iter()
        .filter(|b| ws.baseline_active_profile == "all" || b.profile_name == ws.baseline_active_profile)
        .collect();

    if filtered.is_empty() {
        ui.label(egui::RichText::new("No baseline data for this profile. Capture a snapshot to populate.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Group by freq/TG to show unique channels
    let mut channels: std::collections::BTreeMap<String, Vec<&rf_db::ActivityBaseline>> = std::collections::BTreeMap::new();
    for b in &filtered {
        let key = if let Some(tgid) = b.tgid {
            format!("TG:{}", tgid)
        } else if let Some(freq) = b.freq_mhz {
            format!("{:.4} MHz", freq)
        } else {
            format!("ch:{}", b.channel_id.unwrap_or(0))
        };
        channels.entry(key).or_default().push(b);
    }

    ui.label(egui::RichText::new(format!("{} unique channels/TGs profiled", channels.len()))
        .color(GREEN_COLLECT).size(FONT_SIZE_DATA));

    ui.add_space(4.0);

    // Hour-of-day activity heatmap (aggregated across all channels)
    baseline_section_header(ui, "HOURLY ACTIVITY PATTERN");

    let mut hour_totals = [0.0f64; 24];
    for b in &filtered {
        let h = b.hour_of_day as usize;
        if h < 24 {
            hour_totals[h] += b.avg_sessions;
        }
    }
    let max_total = hour_totals.iter().cloned().fold(0.0f64, f64::max).max(1.0);

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            // Bar chart using painter
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().min(720.0), 80.0),
                egui::Sense::hover(),
            );
            let bar_w = rect.width() / 24.0;
            let painter = ui.painter_at(rect);

            for h in 0..24 {
                let frac = (hour_totals[h] / max_total) as f32;
                let bar_h = frac * rect.height() * 0.85;
                let x = rect.left() + h as f32 * bar_w;
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x + 1.0, rect.bottom() - bar_h - 12.0),
                    egui::vec2(bar_w - 2.0, bar_h),
                );
                let color = if frac > 0.7 {
                    RED_WATCHDOG
                } else if frac > 0.3 {
                    AMBER_WARNING
                } else {
                    GREEN_COLLECT
                };
                painter.rect_filled(bar_rect, 1.0, color.linear_multiply(0.7));

                // Hour label
                painter.text(
                    egui::pos2(x + bar_w / 2.0, rect.bottom() - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{:02}", h),
                    egui::FontId::monospace(8.0),
                    TEXT_SECONDARY,
                );
            }
        });

    ui.add_space(8.0);

    // Top channels table (limited to 30)
    baseline_section_header(ui, "TOP PROFILED CHANNELS");

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("baseline_channels_table")
                .num_columns(5)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["CHANNEL", "ENTRIES", "AVG SESS/HR", "AVG DUR (s)", "SAMPLE DAYS"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    for (key, entries) in channels.iter().take(30) {
                        let total_sessions: f64 = entries.iter().map(|e| e.avg_sessions).sum();
                        let avg_duration: f64 = if entries.is_empty() { 0.0 } else {
                            entries.iter().map(|e| e.avg_duration).sum::<f64>() / entries.len() as f64
                        };
                        let max_sample = entries.iter().map(|e| e.sample_days).max().unwrap_or(0);

                        ui.label(egui::RichText::new(key)
                            .color(CYAN_P25).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.label(egui::RichText::new(format!("{}", entries.len()))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.label(egui::RichText::new(format!("{:.1}", total_sessions / 24.0))
                            .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.label(egui::RichText::new(format!("{:.0}", avg_duration))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.label(egui::RichText::new(format!("{}", max_sample))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.end_row();
                    }
                });
        });
}

// ── Anomaly Events ───────────────────────────────────────

fn show_anomaly_events(ui: &mut egui::Ui, ws: &mut WatchdogState) {
    baseline_section_header(ui, &format!("ANOMALY EVENTS ({})", ws.anomaly_events.len()));

    // Severity filter toolbar
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Min severity:")
            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
        let labels = [("ALL", 0u8), ("WARNING+", 1), ("CRITICAL", 2)];
        for (label, val) in &labels {
            let selected = ws.anomaly_sev_filter == *val;
            let color = if selected { RED_WATCHDOG } else { TEXT_SECONDARY };
            if ui.add(egui::Button::new(
                egui::RichText::new(*label).size(FONT_SIZE_HUD).color(color)
            ).fill(if selected { BG_ELEVATED } else { BG_SURFACE })).clicked() {
                ws.anomaly_sev_filter = *val;
            }
        }
    });
    ui.add_space(4.0);

    if ws.anomaly_events.is_empty() {
        ui.label(egui::RichText::new("No anomaly events detected.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Filter events by severity
    let min_sev = ws.anomaly_sev_filter;
    let filtered: Vec<&rf_db::AnomalyEvent> = ws.anomaly_events.iter()
        .filter(|evt| match min_sev {
            0 => true,
            1 => matches!(evt.severity.as_str(), "warning" | "critical"),
            2 => evt.severity.as_str() == "critical",
            _ => true,
        })
        .collect();

    if filtered.is_empty() {
        ui.label(egui::RichText::new("No events match the selected severity filter.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("anomaly_events_table")
                .num_columns(7)
                .spacing([8.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["SEV", "TYPE", "FREQ", "TG", "SCORE", "DESCRIPTION", "TIME"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    for evt in filtered.iter().take(100) {
                        // Severity
                        let sev_color = match evt.severity.as_str() {
                            "critical" => RED_WATCHDOG,
                            "warning" => AMBER_WARNING,
                            _ => TEXT_SECONDARY,
                        };
                        ui.label(egui::RichText::new(evt.severity.to_uppercase())
                            .color(sev_color).size(FONT_SIZE_DATA));

                        // Type
                        ui.label(egui::RichText::new(&evt.event_type)
                            .color(CYAN_P25).size(FONT_SIZE_DATA));

                        // Freq
                        let freq = evt.freq_mhz
                            .map(|f| format!("{:.4}", f))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&freq)
                            .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // TG
                        let tg = evt.tgid
                            .map(|t| format!("{}", t))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&tg)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Score
                        let score = evt.anomaly_score
                            .map(|s| format!("{:.2}", s))
                            .unwrap_or_else(|| "—".to_string());
                        let score_color = evt.anomaly_score
                            .map(|s| if s > 3.0 { RED_WATCHDOG } else if s > 1.5 { AMBER_WARNING } else { TEXT_SECONDARY })
                            .unwrap_or(TEXT_SECONDARY);
                        ui.label(egui::RichText::new(&score)
                            .color(score_color).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Description (truncated)
                        let truncated = evt.description.chars().nth(50).is_some();
                        let desc: String = evt.description.chars().take(50).collect();
                        let display = if truncated { format!("{}...", desc) } else { desc };
                        ui.label(egui::RichText::new(display)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                        // Time
                        let time = evt.timestamp.get(..16).unwrap_or(&evt.timestamp);
                        ui.label(egui::RichText::new(time)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        ui.end_row();
                    }
                });
        });
}

// ── Live Tail ────────────────────────────────────────────────

/// Severity filter labels for the live tail dropdown.
const SEV_FILTER_LABELS: &[(&str, u8)] = &[
    ("ALL", 0),
    ("INFO+", 9),
    ("WARN+", 13),
    ("ERROR+", 17),
];

fn show_live_tail(ui: &mut egui::Ui, ws: &mut WatchdogState, ui_state: &mut UiState) {
    let now_time = ui.ctx().input(|i| i.time);

    // --- Toolbar ---
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        // Pulsing LIVE dot — green when receiving, dims when idle
        let since_last = now_time - ws.live_tail_last_event_time;
        let dot_alpha: f32 = if ws.live_tail_paused {
            0.3
        } else if since_last < 0.5 {
            1.0
        } else {
            let fade = ((since_last - 0.5).min(0.3)) as f32;
            (0.6 - fade).max(0.3)
        };
        let dot_color = if ws.live_tail_paused {
            TEXT_SECONDARY
        } else {
            egui::Color32::from_rgba_unmultiplied(
                GREEN_COLLECT.r(),
                GREEN_COLLECT.g(),
                GREEN_COLLECT.b(),
                (dot_alpha * 255.0) as u8,
            )
        };
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 3.5, dot_color);

        ui.label(
            egui::RichText::new("LIVE")
                .size(FONT_SIZE_HUD)
                .color(if ws.live_tail_paused { TEXT_SECONDARY } else { GREEN_COLLECT })
                .family(egui::FontFamily::Monospace),
        );

        // Pause/Resume
        if ui.small_button(if ws.live_tail_paused { "\u{25B6}" } else { "\u{23F8}" }).clicked() {
            ws.live_tail_paused = !ws.live_tail_paused;
        }

        // Event rate
        let rate_str = if ws.live_tail_rate >= 1.0 {
            format!("{:.0}/s", ws.live_tail_rate)
        } else if ws.live_tail_rate > 0.0 {
            format!("{:.1}/s", ws.live_tail_rate)
        } else {
            "0/s".to_string()
        };
        ui.label(
            egui::RichText::new(rate_str)
                .size(FONT_SIZE_HUD)
                .color(if ws.live_tail_rate > 10.0 { AMBER_WARNING } else { TEXT_SECONDARY })
                .family(egui::FontFamily::Monospace),
        );

        ui.separator();

        // Tactical/All toggle
        let tac_label = if ws.live_tail_tactical { "TACTICAL" } else { "ALL" };
        let tac_color = if ws.live_tail_tactical { GREEN_COLLECT } else { AMBER_WARNING };
        if ui.add(
            egui::Button::new(
                egui::RichText::new(tac_label).size(FONT_SIZE_HUD).color(tac_color)
                    .family(egui::FontFamily::Monospace),
            ).fill(BG_ELEVATED).stroke(egui::Stroke::new(1.0, tac_color)),
        ).on_hover_text("TACTICAL: physical events only\nALL: include network housekeeping").clicked() {
            ws.live_tail_tactical = !ws.live_tail_tactical;
        }

        ui.separator();

        // Severity filter
        for &(label, min_sev) in SEV_FILTER_LABELS {
            let selected = ws.live_tail_min_severity == min_sev;
            let color = if selected { GREEN_COLLECT } else { TEXT_SECONDARY };
            if ui.add(
                egui::Label::new(
                    egui::RichText::new(label)
                        .size(FONT_SIZE_HUD)
                        .color(color)
                        .family(egui::FontFamily::Monospace),
                ).sense(egui::Sense::click()),
            ).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ws.live_tail_min_severity = min_sev;
            }
        }

        ui.separator();

        // Buffer count + full indicator
        let buf_text = if ws.live_tail.len() >= LIVE_TAIL_MAX {
            format!("{} (FULL)", ws.live_tail.len())
        } else {
            format!("{}", ws.live_tail.len())
        };
        let buf_color = if ws.live_tail.len() >= LIVE_TAIL_MAX { AMBER_WARNING } else { TEXT_SECONDARY };
        ui.label(
            egui::RichText::new(buf_text)
                .size(FONT_SIZE_HUD)
                .color(buf_color)
                .family(egui::FontFamily::Monospace),
        );

        // Clear
        if ui.small_button("Clear").clicked() {
            ws.live_tail.clear();
        }

        ui.separator();

        // Alert volume slider (independent of monitor volume)
        ui.label(
            egui::RichText::new("\u{1F514}")
                .size(FONT_SIZE_HUD),
        );
        let mut alert_pct = (ui_state.alert_volume * 100.0).round() as i32;
        let slider = egui::Slider::new(&mut alert_pct, 0..=100)
            .suffix("%")
            .show_value(true)
            .clamping(egui::SliderClamping::Always);
        let resp = ui.add_sized([100.0, 16.0], slider);
        if resp.changed() {
            ui_state.alert_volume = alert_pct as f32 / 100.0;
        }
    });

    // --- Event stream ---
    egui::ScrollArea::vertical()
        .id_salt("live_tail_scroll")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if ws.live_tail.is_empty() {
                ui.label(
                    egui::RichText::new("Waiting for events...")
                        .color(TEXT_SECONDARY)
                        .size(FONT_SIZE_DATA)
                        .family(egui::FontFamily::Monospace),
                );
            } else {
                let min_sev = ws.live_tail_min_severity;
                // Show newest at bottom (iterate in reverse since live_tail is newest-first)
                for evt in ws.live_tail.iter().rev() {
                    // Severity filter
                    if evt.severity.as_u8() < min_sev {
                        continue;
                    }

                    let sev_color = severity_color(evt.severity.as_u8());
                    let ts = format_timestamp_ns(evt.timestamp_ns);
                    let body_short: String = evt.body.chars().take(90).collect();

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        // Severity dot
                        let (dot_rect, _) = ui.allocate_exact_size(
                            egui::vec2(6.0, FONT_SIZE_DATA),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(
                            egui::pos2(dot_rect.center().x, dot_rect.center().y),
                            2.5,
                            sev_color,
                        );

                        // Timestamp (HH:MM:SS)
                        ui.label(
                            egui::RichText::new(&ts)
                                .size(FONT_SIZE_DATA)
                                .color(TEXT_SECONDARY)
                                .family(egui::FontFamily::Monospace),
                        );

                        // Severity label (3-char)
                        let sev_short = sev_label_short(evt.severity.as_u8());
                        ui.label(
                            egui::RichText::new(sev_short)
                                .size(FONT_SIZE_DATA)
                                .color(sev_color)
                                .family(egui::FontFamily::Monospace),
                        );

                        // Event type (human-readable)
                        let type_display = rf_events::event_types::display_name_or_raw(&evt.event_type);
                        ui.label(
                            egui::RichText::new(type_display)
                                .size(FONT_SIZE_DATA)
                                .color(CYAN_P25)
                                .family(egui::FontFamily::Monospace),
                        );

                        // Frequency (if present)
                        if let Some(freq) = evt.freq_mhz {
                            ui.label(
                                egui::RichText::new(format!("{:.4}", freq))
                                    .size(FONT_SIZE_DATA)
                                    .color(GREEN_COLLECT)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }

                        // Body (truncated)
                        ui.label(
                            egui::RichText::new(&body_short)
                                .size(FONT_SIZE_DATA)
                                .color(TEXT_PRIMARY)
                                .family(egui::FontFamily::Monospace),
                        );
                    });
                }
            }
        });

    // Request repaint for pulse animation
    if !ws.live_tail_paused && now_time - ws.live_tail_last_event_time < 1.0 {
        ui.ctx().request_repaint();
    }
}

// ── Helpers ──────────────────────────────────────────────────

/// Tactical filter: only show events that represent physical RF activity
/// or operator-relevant alerts. Everything else is network housekeeping
/// that the SIEM processes for intelligence but operators don't need live.
fn is_network_noise(event_type: &str) -> bool {
    // Whitelist: these ARE tactical (return false = show them)
    if matches!(event_type,
        // Physical transmissions
        "protocol.p25.voice"
        // Intelligence events
        | "sigex.traffic.session"
        | "sigex.emitter.new"
        | "sigex.emitter.return"
        | "sigex.uid.mismatch"
        | "sigex.crypto.rotation"
        | "sigex.network.emergency"
        | "sigex.network.deny"
        | "sigex.anomaly.baseline"
        | "sigex.protocol.new_uid"
        // Weather
        | "protocol.same.alert"
        // System alerts
        | "system.alert.fired"
        | "system.sdr.error"
        | "system.sdr.disconnect"
        | "system.mode.change"
        | "system.site.enter"
        | "system.site.exit"
    ) {
        return false; // Not noise — show it
    }
    // Also show any custom event rules
    if event_type.starts_with("custom.") {
        return false;
    }
    true // Everything else is network noise — hide it
}

fn metric_card(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(value)
                .size(FONT_SIZE_LARGE)
                .color(color)
                .family(egui::FontFamily::Monospace),
        );
        ui.label(
            egui::RichText::new(label)
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
    });
}

/// Time range presets (index → nanoseconds).
const TIME_RANGE_LABELS: &[(&str, u64)] = &[
    ("1H",  3_600_000_000_000),
    ("6H",  21_600_000_000_000),
    ("24H", 86_400_000_000_000),
    ("7D",  604_800_000_000_000),
];

fn time_range_ns(index: usize) -> u64 {
    TIME_RANGE_LABELS.get(index).map(|&(_, ns)| ns).unwrap_or(86_400_000_000_000)
}

/// Map EventSource u8 to human label.
fn source_label(src_u8: u8) -> &'static str {
    rf_events::EventSource::from_u8(src_u8).label()
}

fn format_timestamp_ns(ns: u64) -> String {
    let secs = (ns / 1_000_000_000) as i64;
    let millis = ((ns % 1_000_000_000) / 1_000_000) as u32;
    let dt = chrono::DateTime::from_timestamp(secs, millis * 1_000_000);
    match dt {
        Some(t) => t.format("%H:%M:%S").to_string(),
        None => "??:??:??".to_string(),
    }
}

fn format_age_ns(ns: u64) -> String {
    let now = rf_events::event::now_ns();
    let delta_sec = now.saturating_sub(ns) / 1_000_000_000;
    if delta_sec < 60 {
        format!("{}s ago", delta_sec)
    } else if delta_sec < 3600 {
        format!("{}m ago", delta_sec / 60)
    } else if delta_sec < 86400 {
        format!("{}h ago", delta_sec / 3600)
    } else {
        format!("{}d ago", delta_sec / 86400)
    }
}

/// 3-char severity label, ranges matching Severity::from_u8.
fn sev_label_short(sev: u8) -> &'static str {
    match sev {
        0..=2 => "TRC",
        3..=6 => "DBG",
        7..=10 => "INF",
        11..=12 => "NTC",
        13..=16 => "WRN",
        17..=20 => "ERR",
        _ => "FTL",
    }
}

fn severity_color(sev: u8) -> egui::Color32 {
    // Ranges match Severity::from_u8
    match sev {
        0..=2 => TEXT_SECONDARY,                        // Trace (1)
        3..=6 => TEXT_SECONDARY,                        // Debug (5)
        7..=10 => TEXT_PRIMARY,                         // Info (9)
        11..=12 => TEXT_PRIMARY,                        // Notice (11)
        13..=16 => AMBER_WARNING,                       // Warn (13)
        17..=20 => RED_WATCHDOG,                        // Error (17)
        21.. => egui::Color32::from_rgb(255, 50, 50),   // Fatal (21)
    }
}

fn priority_color(p: &rf_events::AlertPriority) -> egui::Color32 {
    match p {
        rf_events::AlertPriority::Low => TEXT_SECONDARY,
        rf_events::AlertPriority::Medium => AMBER_WARNING,
        rf_events::AlertPriority::High => egui::Color32::from_rgb(255, 120, 50),
        rf_events::AlertPriority::Critical => RED_WATCHDOG,
    }
}

fn condition_summary(c: &rf_events::AlertCondition) -> String {
    match c {
        rf_events::AlertCondition::Threshold { op, value, window_sec } =>
            format!("count {} {} in {}s", op.label(), value, window_sec),
        rf_events::AlertCondition::Absence { window_sec } =>
            format!("absent for {}s", window_sec),
        rf_events::AlertCondition::RateChange { percent, window_sec, .. } =>
            format!("rate ±{}% in {}s", percent, window_sec),
        rf_events::AlertCondition::FirstOccurrence =>
            "first occurrence".to_string(),
    }
}

fn custom_condition_summary(c: &rf_events::CustomEventCondition) -> String {
    match c {
        rf_events::CustomEventCondition::Every => "every match".to_string(),
        rf_events::CustomEventCondition::Threshold { op, value, window_sec } =>
            format!("count {} {} in {}s", op.label(), value, window_sec),
        rf_events::CustomEventCondition::Absence { window_sec } =>
            format!("absent for {}s", window_sec),
        rf_events::CustomEventCondition::NewValue { field, lookback_sec } =>
            format!("new {:?} in {}s", field, lookback_sec),
        rf_events::CustomEventCondition::Cardinality { field, op, value, window_sec } =>
            format!("card {:?} {} {} in {}s", field, op.label(), value, window_sec),
        rf_events::CustomEventCondition::RateChange { percent, window_sec, .. } =>
            format!("rate ±{}% in {}s", percent, window_sec),
        rf_events::CustomEventCondition::Correlation { window_sec, .. } =>
            format!("correlation in {}s", window_sec),
    }
}
