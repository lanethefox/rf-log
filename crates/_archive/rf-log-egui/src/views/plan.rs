use eframe::egui;

use crate::bridge::UiBridge;
use crate::state::{PlanTab, UiState};
use crate::theme::*;
use crate::widgets::tab_bar;

// ── PlanState ────────────────────────────────────────────

/// Poll interval for DB queries (seconds).
const DB_POLL_INTERVAL: f64 = 3.0;

/// Mutable per-frame state for the PLAN view.
pub struct PlanState {
    // ── Operations cache ──
    pub operations: Vec<rf_db::Operation>,
    pub operators: Vec<rf_db::Operator>,
    pub op_stats: Option<serde_json::Value>,
    pub op_stats_id: Option<i64>,
    pub op_operators: Vec<rf_db::Operator>,
    pub op_operators_id: Option<i64>,

    // ── Create operation form ──
    pub new_op_name: String,
    pub new_op_desc: String,
    pub new_op_profile: String, // "test" or "live"
    pub show_create_form: bool,

    // ── Create operator form ──
    pub new_opr_callsign: String,
    pub new_opr_display_name: String,
    pub new_opr_notes: String,
    pub show_create_operator: bool,

    // ── Selected operation for detail view ──
    pub selected_op_id: Option<i64>,

    // ── Sites cache ──
    pub sites: Vec<rf_db::IntelSite>,
    pub selected_site_id: Option<i64>,
    pub site_dashboard: Option<rf_db::SiteDashboard>,
    pub site_dashboard_id: Option<i64>,
    pub site_sessions: Vec<rf_db::SiteSession>,
    pub site_sessions_id: Option<i64>,
    pub site_talkgroups: Vec<serde_json::Value>,
    pub site_talkgroups_id: Option<i64>,
    pub site_radio_ids: Vec<serde_json::Value>,
    pub site_radio_ids_id: Option<i64>,

    // ── Config tab ──
    pub antennas: Vec<rf_db::Antenna>,
    pub show_antenna_form: bool,
    pub editing_antenna_id: Option<i64>,
    pub ant_form_name: String,
    pub ant_form_type: String,
    pub ant_form_connector: String,
    pub ant_form_freq_min: String,
    pub ant_form_freq_max: String,
    pub ant_form_gain: String,
    pub ant_form_notes: String,
    pub device_names: std::collections::HashMap<String, String>,
    pub device_name_edits: std::collections::HashMap<String, String>,

    // ── Create/edit site form ──
    pub show_site_form: bool,
    pub editing_site_id: Option<i64>,
    pub site_form_name: String,
    pub site_form_type: String,
    pub site_form_lat: String,
    pub site_form_lon: String,
    pub site_form_radius: String,
    pub site_form_address: String,
    pub site_form_notes: String,

    // ── Targets tab cache ──
    pub targets: Vec<rf_db::ObservationTarget>,
    pub collection_targets: Vec<serde_json::Value>,
    pub requirements: Vec<rf_db::CollectionRequirement>,
    pub auto_iq_rules: Vec<rf_db::AutoIqRule>,
    pub packages: Vec<rf_db::ScanPackage>,
    pub selected_package_id: Option<i64>,
    pub package_items: Vec<rf_db::ScanPackageItem>,
    pub observations: Vec<rf_db::Observation>,
    pub obs_alerts: Vec<rf_db::ObservationAlert>,
    pub selected_target_id: Option<i64>,

    // ── Create/edit target form ──
    pub show_target_form: bool,
    pub editing_target_id: Option<i64>,
    pub target_form_type: String,
    pub target_form_key: String,
    pub target_form_label: String,
    pub target_form_priority: String,
    pub target_form_notes: String,
    pub confirm_delete_target: Option<i64>,

    // ── Create requirement form ──
    pub show_req_form: bool,
    pub req_form_label: String,
    pub req_form_type: String,
    pub confirm_delete_req: Option<i64>,

    // ── Create auto-IQ rule form ──
    pub show_iq_rule_form: bool,
    pub iq_rule_form_type: String,
    pub iq_rule_form_duration: String,

    // ── Data tab ──
    pub schema: Vec<rf_db::TableSchema>,
    pub saved_queries: Vec<rf_db::SavedQuery>,
    pub sql_input: String,
    pub query_result: Option<rf_db::QueryResult>,
    pub query_error: Option<String>,
    pub save_query_name: String,
    #[allow(dead_code)] // CollapsingHeader manages open/close state
    pub show_schema_browser: bool,
    pub selected_schema_table: Option<usize>,
    pub selected_prebuilt: Option<usize>,

    // ── Auto-suggestion caches (Targets tab) ──
    pub suggest_frequencies: Vec<String>,   // from channels table freq_mhz
    pub suggest_talkgroups: Vec<String>,    // from network_talkgroups tgid
    pub suggest_radio_ids: Vec<String>,     // from radio_id_sightings uid
    pub suggest_channels: Vec<String>,      // from channels table label
    pub suggest_networks: Vec<String>,      // from network_sites system (deduplicated)
    pub suggest_sites: Vec<String>,         // from intel_sites name

    // ── Timing ──
    last_poll: f64,
}

impl Default for PlanState {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            operators: Vec::new(),
            op_stats: None,
            op_stats_id: None,
            op_operators: Vec::new(),
            op_operators_id: None,
            new_op_name: String::new(),
            new_op_desc: String::new(),
            new_op_profile: "test".to_string(),
            show_create_form: false,
            new_opr_callsign: String::new(),
            new_opr_display_name: String::new(),
            new_opr_notes: String::new(),
            show_create_operator: false,
            selected_op_id: None,
            sites: Vec::new(),
            selected_site_id: None,
            site_dashboard: None,
            site_dashboard_id: None,
            site_sessions: Vec::new(),
            site_sessions_id: None,
            site_talkgroups: Vec::new(),
            site_talkgroups_id: None,
            site_radio_ids: Vec::new(),
            site_radio_ids_id: None,
            antennas: Vec::new(),
            show_antenna_form: false,
            editing_antenna_id: None,
            ant_form_name: String::new(),
            ant_form_type: "whip".to_string(),
            ant_form_connector: "SMA".to_string(),
            ant_form_freq_min: String::new(),
            ant_form_freq_max: String::new(),
            ant_form_gain: String::new(),
            ant_form_notes: String::new(),
            device_names: std::collections::HashMap::new(),
            device_name_edits: std::collections::HashMap::new(),
            show_site_form: false,
            editing_site_id: None,
            site_form_name: String::new(),
            site_form_type: String::new(),
            site_form_lat: String::new(),
            site_form_lon: String::new(),
            site_form_radius: "500".to_string(),
            site_form_address: String::new(),
            site_form_notes: String::new(),
            targets: Vec::new(),
            collection_targets: Vec::new(),
            requirements: Vec::new(),
            auto_iq_rules: Vec::new(),
            packages: Vec::new(),
            selected_package_id: None,
            package_items: Vec::new(),
            observations: Vec::new(),
            obs_alerts: Vec::new(),
            selected_target_id: None,
            show_target_form: false,
            editing_target_id: None,
            target_form_type: "frequency".to_string(),
            target_form_key: String::new(),
            target_form_label: String::new(),
            target_form_priority: "0".to_string(),
            target_form_notes: String::new(),
            confirm_delete_target: None,
            show_req_form: false,
            req_form_label: String::new(),
            req_form_type: "manual".to_string(),
            confirm_delete_req: None,
            show_iq_rule_form: false,
            iq_rule_form_type: "frequency".to_string(),
            iq_rule_form_duration: "30".to_string(),
            schema: Vec::new(),
            saved_queries: Vec::new(),
            sql_input: String::new(),
            query_result: None,
            query_error: None,
            save_query_name: String::new(),
            show_schema_browser: false,
            selected_schema_table: None,
            selected_prebuilt: None,
            suggest_frequencies: Vec::new(),
            suggest_talkgroups: Vec::new(),
            suggest_radio_ids: Vec::new(),
            suggest_channels: Vec::new(),
            suggest_networks: Vec::new(),
            suggest_sites: Vec::new(),
            last_poll: 0.0,
        }
    }
}

// ── Main show ────────────────────────────────────────────

/// PLAN view — 5 planning/config sub-tabs
pub fn show(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    _bridge: &UiBridge,
    state: &rf_web::AppState,
    ps: &mut PlanState,
) {
    // Periodic DB refresh — only query data for the active tab
    let now = ui.input(|i| i.time);
    if now - ps.last_poll > DB_POLL_INTERVAL {
        ps.last_poll = now;
        refresh_db_cache(state, ps, ui_state.plan_tab);
    }

    ui.vertical(|ui| {
        tab_bar::show(
            ui,
            PlanTab::ALL,
            &mut ui_state.plan_tab,
            |t| t.label(),
            BLUE_PLAN,
        );
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            match ui_state.plan_tab {
                PlanTab::Ops => show_ops_tab(ui, state, ps),
                PlanTab::Sites => show_sites_tab(ui, state, ps),
                PlanTab::Targets => {
                    show_targets_tab(ui, state, ps);
                }
                PlanTab::Data => {
                    show_data_tab(ui, state, ps);
                }
                PlanTab::Config => {
                    show_config_tab(ui, state, ps);
                }
            }
        });
    });
}

// ── DB refresh ───────────────────────────────────────────

fn refresh_db_cache(state: &rf_web::AppState, ps: &mut PlanState, active_tab: PlanTab) {
    match active_tab {
        PlanTab::Ops => {
            if let Ok(ops) = state.db().list_operations() {
                ps.operations = ops;
            }
            if let Ok(oprs) = state.db().list_operators() {
                ps.operators = oprs;
            }
            if let Some(id) = ps.selected_op_id {
                if let Ok(stats) = state.db().get_operation_stats(id) {
                    ps.op_stats = Some(stats);
                    ps.op_stats_id = Some(id);
                }
                if ps.op_operators_id != Some(id) {
                    if let Ok(oprs) = state.db().list_operation_operators(id) {
                        ps.op_operators = oprs;
                        ps.op_operators_id = Some(id);
                    }
                }
            }
        }
        PlanTab::Sites => {
            if let Ok(sites) = state.db().list_intel_sites(500) {
                ps.sites = sites;
            }
            if let Some(id) = ps.selected_site_id {
                if ps.site_dashboard_id != Some(id) {
                    if let Ok(dash) = state.db().site_dashboard(id) {
                        ps.site_dashboard = Some(dash);
                        ps.site_dashboard_id = Some(id);
                    }
                }
                if ps.site_sessions_id != Some(id) {
                    if let Ok(sessions) = state.db().list_site_sessions(id, 50) {
                        ps.site_sessions = sessions;
                        ps.site_sessions_id = Some(id);
                    }
                }
                if ps.site_talkgroups_id != Some(id) {
                    if let Ok(tgs) = state.db().site_talkgroups(id, 100) {
                        ps.site_talkgroups = tgs;
                        ps.site_talkgroups_id = Some(id);
                    }
                }
                if ps.site_radio_ids_id != Some(id) {
                    if let Ok(uids) = state.db().site_radio_ids(id, 100) {
                        ps.site_radio_ids = uids;
                        ps.site_radio_ids_id = Some(id);
                    }
                }
            }
        }
        PlanTab::Targets => {
            if let Ok(targets) = state.db().list_observation_targets(None, None) {
                ps.targets = targets;
            }
            if let Ok(ct) = state.db().list_collection_targets(None) {
                ps.collection_targets = ct;
            }
            if let Ok(reqs) = state.db().list_collection_requirements(None) {
                ps.requirements = reqs;
            }
            if let Ok(rules) = state.db().list_auto_iq_rules(None) {
                ps.auto_iq_rules = rules;
            }
            if let Ok(pkgs) = state.db().get_packages() {
                ps.packages = pkgs;
            }
            if let Some(pkg_id) = ps.selected_package_id {
                if let Ok(items) = state.db().get_package_items(pkg_id) {
                    ps.package_items = items;
                }
            }
            if let Ok(obs) = state.db().list_observations(ps.selected_target_id, None, 100, None) {
                ps.observations = obs;
            }
            if let Ok(alerts) = state.db().list_observation_alerts(ps.selected_target_id) {
                ps.obs_alerts = alerts;
            }

            // Populate auto-suggestion caches
            if let Ok(channels) = state.db().list_channels(&rf_db::ChannelFilter { limit: Some(500), ..Default::default() }) {
                let mut freqs: Vec<String> = channels.iter()
                    .filter_map(|c| c.freq_mhz.map(|f| format!("{:.4}", f)))
                    .collect();
                freqs.sort();
                freqs.dedup();
                ps.suggest_frequencies = freqs;

                let mut names: Vec<String> = channels.iter()
                    .filter(|c| !c.label.is_empty())
                    .map(|c| c.label.clone())
                    .collect();
                names.sort();
                names.dedup();
                ps.suggest_channels = names;
            }
            if let Ok(tgs) = state.db().list_network_talkgroups(None, 500) {
                let mut ids: Vec<String> = tgs.iter()
                    .map(|t| {
                        if let Some(ref name) = t.name {
                            format!("{} — {}", t.tgid, name)
                        } else {
                            format!("{}", t.tgid)
                        }
                    })
                    .collect();
                ids.sort();
                ids.dedup();
                ps.suggest_talkgroups = ids;
            }
            if let Ok(uids) = state.db().list_radio_id_sightings(200) {
                let mut ids: Vec<String> = uids.iter()
                    .map(|u| format!("{}", u.uid))
                    .collect();
                ids.sort();
                ids.dedup();
                ps.suggest_radio_ids = ids;
            }
            if let Ok(sites) = state.db().list_network_sites(None, 100) {
                let mut systems: Vec<String> = sites.iter()
                    .map(|s| s.system.clone())
                    .collect();
                systems.sort();
                systems.dedup();
                ps.suggest_networks = systems;
            }
            if let Ok(intel_sites) = state.db().list_intel_sites(200) {
                let mut names: Vec<String> = intel_sites.iter()
                    .map(|s| s.name.clone())
                    .collect();
                names.sort();
                names.dedup();
                ps.suggest_sites = names;
            }
        }
        PlanTab::Data => {
            if let Ok(sq) = state.db().list_saved_queries() {
                ps.saved_queries = sq;
            }
            if ps.schema.is_empty() {
                if let Ok(schema) = state.db().query_schema() {
                    ps.schema = schema;
                }
            }
        }
        PlanTab::Config => {
            if let Ok(ants) = state.db().list_antennas() {
                ps.antennas = ants;
            }
            if let Ok(names) = state.db().get_device_names() {
                ps.device_names = names;
            }
        }
    }
}

// ── Ops tab ──────────────────────────────────────────────

fn show_ops_tab(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    let config = state.config();
    let active_op_id = config.active_operation_id;

    // Active operation banner (always visible — not collapsible)
    if let Some(op_id) = active_op_id {
        let op_name = config.active_operation_name.as_deref().unwrap_or("?");
        let profile = config.active_operation_profile.as_deref().unwrap_or("test");
        ui.add_space(4.0);
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(8))
            .fill(egui::Color32::from_rgba_premultiplied(68, 136, 255, 25))
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("ACTIVE")
                        .color(GREEN_COLLECT).size(FONT_SIZE_DATA).strong());
                    ui.label(egui::RichText::new(format!("{} (id={}, {})", op_name, op_id, profile))
                        .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new("STOP").color(RED_WATCHDOG).size(FONT_SIZE_DATA)).clicked() {
                            stop_operation(state, op_id);
                            ps.last_poll = 0.0; // force refresh
                        }
                        if ui.button(egui::RichText::new("PAUSE").color(AMBER_WARNING).size(FONT_SIZE_DATA)).clicked() {
                            pause_operation(state, op_id);
                            ps.last_poll = 0.0;
                        }
                    });
                });
            });
        ui.add_space(4.0);
    }

    // Operations section
    let ops_header = format!("OPERATIONS ({})", ps.operations.len());
    egui::CollapsingHeader::new(egui::RichText::new(ops_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(true)
        .show(ui, |ui| {
            // Create button
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new(if ps.show_create_form { "CANCEL" } else { "+ NEW" })
                    .color(BLUE_PLAN).size(FONT_SIZE_DATA)).clicked()
                {
                    ps.show_create_form = !ps.show_create_form;
                }
            });

            // Create form
            if ps.show_create_form {
                show_create_op_form(ui, state, ps);
            }

            ui.add_space(4.0);

            // Operations table
            if ps.operations.is_empty() {
                ui.label(egui::RichText::new("No operations found.")
                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            } else {
                show_ops_table(ui, state, ps, active_op_id);
            }
        });

    // Selected operation detail
    if let Some(sel_id) = ps.selected_op_id {
        egui::CollapsingHeader::new(egui::RichText::new(format!("OPERATION #{} DETAIL", sel_id)).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
            .default_open(true)
            .show(ui, |ui| show_op_detail(ui, state, ps, sel_id, active_op_id));
    }

    // Operators section
    let opr_header = format!("OPERATORS ({})", ps.operators.len());
    egui::CollapsingHeader::new(egui::RichText::new(opr_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_operators_section_body(ui, state, ps));
}

// ── Create operation form ────────────────────────────────

fn show_create_op_form(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(8))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("CREATE OPERATION")
                .color(BLUE_PLAN).size(FONT_SIZE_HUD));
            ui.add_space(4.0);

            egui::Grid::new("create_op_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("Name").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.new_op_name)
                    .desired_width(300.0).font(egui::TextStyle::Monospace));
                ui.end_row();

                ui.label(egui::RichText::new("Description").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::multiline(&mut ps.new_op_desc)
                    .desired_width(300.0).desired_rows(2).font(egui::TextStyle::Monospace));
                ui.end_row();

                ui.label(egui::RichText::new("Profile").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut ps.new_op_profile, "test".to_string(),
                        egui::RichText::new("TEST").color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                    ui.selectable_value(&mut ps.new_op_profile, "live".to_string(),
                        egui::RichText::new("LIVE").color(GREEN_COLLECT).size(FONT_SIZE_DATA));
                });
                ui.end_row();
            });

            ui.add_space(4.0);
            if ui.add_enabled(
                !ps.new_op_name.trim().is_empty(),
                egui::Button::new(egui::RichText::new("CREATE").color(BLUE_PLAN).size(FONT_SIZE_DATA)),
            ).clicked() {
                let name = ps.new_op_name.trim().to_string();
                let desc = ps.new_op_desc.trim().to_string();
                let profile = ps.new_op_profile.clone();
                let config = state.config();
                let created_by = config.active_operator_id;
                match state.db().create_operation(&name, "{}", Some(&desc), created_by, Some(&profile)) {
                    Ok(id) => {
                        tracing::info!("Created operation: {} (id={})", name, id);
                        ps.new_op_name.clear();
                        ps.new_op_desc.clear();
                        ps.new_op_profile = "test".to_string();
                        ps.show_create_form = false;
                        ps.last_poll = 0.0;
                    }
                    Err(e) => tracing::error!("Failed to create operation: {}", e),
                }
            }
        });
}

// ── Operations table ─────────────────────────────────────

fn show_ops_table(
    ui: &mut egui::Ui,
    state: &rf_web::AppState,
    ps: &mut PlanState,
    active_op_id: Option<i64>,
) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("ops_table")
                .num_columns(6)
                .spacing([12.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    // Header
                    for h in ["ID", "NAME", "STATUS", "PROFILE", "CREATED", "ACTIONS"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let ops_snapshot: Vec<rf_db::Operation> = ps.operations.clone();
                    for op in &ops_snapshot {
                        let is_active = active_op_id == Some(op.id);
                        let is_selected = ps.selected_op_id == Some(op.id);

                        // ID
                        let id_color = if is_active { GREEN_COLLECT } else { TEXT_PRIMARY };
                        if ui.add(egui::Label::new(
                            egui::RichText::new(format!("{}", op.id)).color(id_color).size(FONT_SIZE_DATA)
                        ).sense(egui::Sense::click())).clicked() {
                            ps.selected_op_id = if is_selected { None } else { Some(op.id) };
                            ps.op_stats_id = None; // force stats refresh
                            ps.op_operators_id = None;
                            ps.last_poll = 0.0;
                        }

                        // Name
                        ui.label(egui::RichText::new(&op.name)
                            .color(if is_active { GREEN_COLLECT } else { TEXT_PRIMARY }).size(FONT_SIZE_DATA));

                        // Status
                        let (status_color, status_text) = match op.status.as_str() {
                            "active" => (GREEN_COLLECT, "ACTIVE"),
                            "paused" => (AMBER_WARNING, "PAUSED"),
                            "completed" => (TEXT_SECONDARY, "COMPLETED"),
                            "archived" => (TEXT_SECONDARY, "ARCHIVED"),
                            "created" => (BLUE_PLAN, "CREATED"),
                            _ => (TEXT_SECONDARY, op.status.as_str()),
                        };
                        ui.label(egui::RichText::new(status_text)
                            .color(status_color).size(FONT_SIZE_DATA));

                        // Profile
                        let prof_color = if op.profile == "live" { GREEN_COLLECT } else { TEXT_SECONDARY };
                        ui.label(egui::RichText::new(&op.profile)
                            .color(prof_color).size(FONT_SIZE_DATA));

                        // Created
                        let created_short = op.created_at.get(..16).unwrap_or(&op.created_at);
                        ui.label(egui::RichText::new(created_short)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Actions
                        ui.horizontal(|ui| {
                            match op.status.as_str() {
                                "created" | "paused" => {
                                    if ui.small_button(egui::RichText::new("START").color(GREEN_COLLECT).size(FONT_SIZE_HUD)).clicked() {
                                        start_operation(state, op.id);
                                        ps.last_poll = 0.0;
                                    }
                                }
                                "active" => {
                                    if ui.small_button(egui::RichText::new("PAUSE").color(AMBER_WARNING).size(FONT_SIZE_HUD)).clicked() {
                                        pause_operation(state, op.id);
                                        ps.last_poll = 0.0;
                                    }
                                    if ui.small_button(egui::RichText::new("STOP").color(RED_WATCHDOG).size(FONT_SIZE_HUD)).clicked() {
                                        stop_operation(state, op.id);
                                        ps.last_poll = 0.0;
                                    }
                                }
                                "completed" => {
                                    if ui.small_button(egui::RichText::new("ARCHIVE").color(TEXT_SECONDARY).size(FONT_SIZE_HUD)).clicked() {
                                        let _ = state.db().update_operation_status(op.id, "archived");
                                        ps.last_poll = 0.0;
                                    }
                                }
                                _ => {}
                            }
                        });
                        ui.end_row();
                    }
                });
        });
}

// ── Operation detail panel ───────────────────────────────

fn show_op_detail(
    ui: &mut egui::Ui,
    state: &rf_web::AppState,
    ps: &mut PlanState,
    op_id: i64,
    active_op_id: Option<i64>,
) {
    let op = match ps.operations.iter().find(|o| o.id == op_id) {
        Some(o) => o.clone(),
        None => return,
    };

    // Description
    if !op.description.is_empty() {
        ui.label(egui::RichText::new(&op.description)
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        ui.add_space(4.0);
    }

    // Metadata grid
    egui::Grid::new("op_detail_grid").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
        detail_row(ui, "Status", &op.status);
        detail_row(ui, "Profile", &op.profile);
        detail_row(ui, "Created", &op.created_at);
        if let Some(ref started) = op.started_at {
            detail_row(ui, "Started", started);
        }
        if let Some(ref stopped) = op.stopped_at {
            detail_row(ui, "Stopped", stopped);
        }
    });

    // Stats card
    if ps.op_stats_id == Some(op_id) {
        if let Some(ref stats) = ps.op_stats {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("STATISTICS")
                .color(BLUE_PLAN).size(FONT_SIZE_HUD));
            egui::Grid::new("op_stats_grid").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                stat_row(ui, "Signal Hits", stats.get("signal_hits"));
                stat_row(ui, "Traffic Sessions", stats.get("traffic_sessions"));
                stat_row(ui, "Channel Grants", stats.get("channel_grants"));
                stat_row(ui, "Recordings", stats.get("recordings"));
                stat_row(ui, "SIGEX Events", stats.get("sigex_events"));
                stat_row(ui, "Sessions", stats.get("sessions"));
                stat_row(ui, "Encrypted Grants", stats.get("encrypted_grants"));
                stat_row(ui, "Unique TGs", stats.get("unique_tgs"));
            });
        }
    }

    // Assigned operators
    if ps.op_operators_id == Some(op_id) && !ps.op_operators.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("ASSIGNED OPERATORS")
            .color(BLUE_PLAN).size(FONT_SIZE_HUD));
        for opr in &ps.op_operators.clone() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&opr.callsign)
                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA).strong());
                ui.label(egui::RichText::new(&opr.display_name)
                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                if ui.small_button(egui::RichText::new("x").color(RED_WATCHDOG).size(FONT_SIZE_HUD)).clicked() {
                    let _ = state.db().remove_operator_from_operation(op_id, opr.id);
                    ps.op_operators_id = None;
                    ps.last_poll = 0.0;
                }
            });
        }
    }

    // Assign operator dropdown
    let unassigned: Vec<&rf_db::Operator> = ps.operators.iter()
        .filter(|o| !ps.op_operators.iter().any(|a| a.id == o.id))
        .collect();
    if !unassigned.is_empty() && active_op_id == Some(op_id) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Assign:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            for opr in &unassigned {
                if ui.small_button(egui::RichText::new(&opr.callsign).color(BLUE_PLAN).size(FONT_SIZE_HUD)).clicked() {
                    let _ = state.db().assign_operator_to_operation(op_id, opr.id, "operator");
                    ps.op_operators_id = None;
                    ps.last_poll = 0.0;
                }
            }
        });
    }
}

// ── Operators section ────────────────────────────────────

fn show_operators_section_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(egui::RichText::new(if ps.show_create_operator { "CANCEL" } else { "+ NEW" })
                .color(BLUE_PLAN).size(FONT_SIZE_DATA)).clicked()
            {
                ps.show_create_operator = !ps.show_create_operator;
            }
        });
    });

    // Create operator form
    if ps.show_create_operator {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(8))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("create_opr_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(egui::RichText::new("Callsign").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::TextEdit::singleline(&mut ps.new_opr_callsign)
                        .desired_width(200.0).font(egui::TextStyle::Monospace));
                    ui.end_row();

                    ui.label(egui::RichText::new("Display Name").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::TextEdit::singleline(&mut ps.new_opr_display_name)
                        .desired_width(200.0).font(egui::TextStyle::Monospace));
                    ui.end_row();

                    ui.label(egui::RichText::new("Notes").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::TextEdit::singleline(&mut ps.new_opr_notes)
                        .desired_width(200.0).font(egui::TextStyle::Monospace));
                    ui.end_row();
                });

                ui.add_space(4.0);
                if ui.add_enabled(
                    !ps.new_opr_callsign.trim().is_empty(),
                    egui::Button::new(egui::RichText::new("CREATE").color(BLUE_PLAN).size(FONT_SIZE_DATA)),
                ).clicked() {
                    let cs = ps.new_opr_callsign.trim().to_string();
                    let dn = ps.new_opr_display_name.trim().to_string();
                    let notes = ps.new_opr_notes.trim().to_string();
                    match state.db().create_operator(&cs, &dn, &notes) {
                        Ok(id) => {
                            tracing::info!("Created operator: {} (id={})", cs, id);
                            ps.new_opr_callsign.clear();
                            ps.new_opr_display_name.clear();
                            ps.new_opr_notes.clear();
                            ps.show_create_operator = false;
                            ps.last_poll = 0.0;
                        }
                        Err(e) => tracing::error!("Failed to create operator: {}", e),
                    }
                }
            });
    }

    // Operators table
    if ps.operators.is_empty() {
        ui.label(egui::RichText::new("No operators found.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    } else {
        let config = state.config();
        let active_opr_id = config.active_operator_id;

        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_SURFACE)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("operators_table")
                    .num_columns(5)
                    .spacing([12.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["ID", "CALLSIGN", "NAME", "LAST LOGIN", "ACTIONS"] {
                            ui.label(egui::RichText::new(h)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                        }
                        ui.end_row();

                        let oprs_snapshot: Vec<rf_db::Operator> = ps.operators.clone();
                        for opr in &oprs_snapshot {
                            let is_active = active_opr_id == Some(opr.id);

                            ui.label(egui::RichText::new(format!("{}", opr.id))
                                .color(if is_active { GREEN_COLLECT } else { TEXT_PRIMARY }).size(FONT_SIZE_DATA));
                            ui.label(egui::RichText::new(&opr.callsign)
                                .color(if is_active { GREEN_COLLECT } else { TEXT_PRIMARY }).size(FONT_SIZE_DATA).strong());
                            ui.label(egui::RichText::new(&opr.display_name)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                            ui.label(egui::RichText::new(
                                opr.last_login.as_deref().and_then(|s| s.get(..16)).unwrap_or("—")
                            ).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                            ui.horizontal(|ui| {
                                if !is_active {
                                    if ui.small_button(egui::RichText::new("SET ACTIVE").color(GREEN_COLLECT).size(FONT_SIZE_HUD)).clicked() {
                                        set_active_operator(state, opr.id, &opr.callsign);
                                        ps.last_poll = 0.0;
                                    }
                                } else {
                                    if ui.small_button(egui::RichText::new("CLEAR").color(AMBER_WARNING).size(FONT_SIZE_HUD)).clicked() {
                                        clear_active_operator(state);
                                        ps.last_poll = 0.0;
                                    }
                                }
                                if ui.small_button(egui::RichText::new("DEL").color(RED_WATCHDOG).size(FONT_SIZE_HUD)).clicked() {
                                    if is_active {
                                        clear_active_operator(state);
                                    }
                                    let _ = state.db().delete_operator(opr.id);
                                    ps.last_poll = 0.0;
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
    }
}

// ── Operation lifecycle actions ──────────────────────────

fn start_operation(state: &rf_web::AppState, op_id: i64) {
    if let Err(e) = state.db().update_operation_status(op_id, "active") {
        tracing::error!("Failed to start operation {}: {}", op_id, e);
        return;
    }
    if let Ok(Some(op)) = state.db().get_operation(op_id) {
        state.update_config(|c| {
            c.active_operation_id = Some(op_id);
            c.active_operation_name = Some(op.name.clone());
            c.active_operation_profile = Some(op.profile.clone());
        });
        // Create session for active operator
        let config = state.config();
        if let Some(opr_id) = config.active_operator_id {
            match state.db().create_session(op_id, Some(opr_id), "collection") {
                Ok(sess_id) => {
                    state.update_config(|c| { c.active_session_id = Some(sess_id); });
                }
                Err(e) => tracing::warn!("Failed to create session: {}", e),
            }
        }
        tracing::info!("Operation started: {} (id={})", op.name, op_id);
    }
}

fn stop_operation(state: &rf_web::AppState, op_id: i64) {
    let config = state.config();
    // Close active session
    if let Some(sess_id) = config.active_session_id {
        let _ = state.db().close_session(sess_id);
    }
    // Save config snapshot
    if let Ok(config_json) = serde_json::to_string(&config) {
        let _ = state.db().save_operation_config(op_id, &config_json);
    }
    let _ = state.db().update_operation_status(op_id, "completed");
    state.update_config(|c| {
        c.active_operation_id = None;
        c.active_operation_name = None;
        c.active_operation_profile = None;
        c.active_session_id = None;
    });
    tracing::info!("Operation stopped (id={})", op_id);
}

fn pause_operation(state: &rf_web::AppState, op_id: i64) {
    let config = state.config();
    if let Some(sess_id) = config.active_session_id {
        let _ = state.db().close_session(sess_id);
    }
    let _ = state.db().update_operation_status(op_id, "paused");
    state.update_config(|c| {
        c.active_session_id = None;
    });
    tracing::info!("Operation paused (id={})", op_id);
}

fn set_active_operator(state: &rf_web::AppState, opr_id: i64, callsign: &str) {
    let _ = state.db().update_operator_login(opr_id);
    state.update_config(|c| {
        c.active_operator_id = Some(opr_id);
        c.active_operator_callsign = Some(callsign.to_string());
    });
    // If an operation is active, create a session for this operator
    let config = state.config();
    if let Some(op_id) = config.active_operation_id {
        if config.active_session_id.is_none() {
            match state.db().create_session(op_id, Some(opr_id), "collection") {
                Ok(sess_id) => {
                    state.update_config(|c| { c.active_session_id = Some(sess_id); });
                }
                Err(e) => tracing::warn!("Failed to create session: {}", e),
            }
        }
    }
    tracing::info!("Active operator set: {} (id={})", callsign, opr_id);
}

fn clear_active_operator(state: &rf_web::AppState) {
    let config = state.config();
    if let Some(sess_id) = config.active_session_id {
        let _ = state.db().close_session(sess_id);
    }
    state.update_config(|c| {
        c.active_operator_id = None;
        c.active_operator_callsign = None;
        c.active_session_id = None;
    });
    tracing::info!("Active operator cleared");
}

// ── Sites tab ────────────────────────────────────────────

fn show_sites_tab(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    let config = state.config();
    let active_site_id = config.active_site_id;

    // Active site banner
    if let Some(site_id) = active_site_id {
        let site_name = ps.sites.iter()
            .find(|s| s.id == site_id)
            .map(|s| s.name.as_str())
            .unwrap_or("?");
        ui.add_space(4.0);
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(8))
            .fill(egui::Color32::from_rgba_premultiplied(68, 136, 255, 25))
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("AT SITE")
                        .color(GREEN_COLLECT).size(FONT_SIZE_DATA).strong());
                    ui.label(egui::RichText::new(format!("{} (id={})", site_name, site_id))
                        .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                    if let Some(sess_id) = config.active_site_session_id {
                        ui.label(egui::RichText::new(format!("session={}", sess_id))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    }
                });
            });
        ui.add_space(4.0);
    }

    // Intel Sites section
    let sites_header = format!("INTEL SITES ({})", ps.sites.len());
    egui::CollapsingHeader::new(egui::RichText::new(sites_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new(if ps.show_site_form { "CANCEL" } else { "+ NEW" })
                    .color(BLUE_PLAN).size(FONT_SIZE_DATA)).clicked()
                {
                    ps.show_site_form = !ps.show_site_form;
                    if ps.show_site_form {
                        ps.editing_site_id = None;
                        clear_site_form(ps);
                    }
                }
            });

            // Create/edit form
            if ps.show_site_form {
                show_site_form(ui, state, ps);
            }

            ui.add_space(4.0);

            // Sites table
            if ps.sites.is_empty() {
                ui.label(egui::RichText::new("No sites found.")
                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            } else {
                show_sites_table(ui, state, ps, active_site_id);
            }
        });

    // Selected site detail
    if let Some(sel_id) = ps.selected_site_id {
        let site_name = ps.sites.iter()
            .find(|s| s.id == sel_id)
            .map(|s| s.name.as_str())
            .unwrap_or("?");
        let detail_header = format!("SITE DETAIL: {}", site_name);
        egui::CollapsingHeader::new(egui::RichText::new(detail_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
            .default_open(true)
            .show(ui, |ui| show_site_detail(ui, state, ps, sel_id));
    }
}

fn clear_site_form(ps: &mut PlanState) {
    ps.site_form_name.clear();
    ps.site_form_type.clear();
    ps.site_form_lat.clear();
    ps.site_form_lon.clear();
    ps.site_form_radius = "500".to_string();
    ps.site_form_address.clear();
    ps.site_form_notes.clear();
}

fn show_site_form(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    let is_edit = ps.editing_site_id.is_some();
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(8))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(if is_edit { "EDIT SITE" } else { "CREATE SITE" })
                .color(BLUE_PLAN).size(FONT_SIZE_HUD));
            ui.add_space(4.0);

            egui::Grid::new("site_form_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("Name *").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                crate::theme::required_text_field(ui, &mut ps.site_form_name, 250.0, "Site name (required)");
                ui.end_row();

                ui.label(egui::RichText::new("Type").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.horizontal(|ui| {
                    for t in ["collection", "target", "fixed", "mobile"] {
                        ui.selectable_value(&mut ps.site_form_type, t.to_string(),
                            egui::RichText::new(t.to_uppercase()).size(FONT_SIZE_HUD));
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new("Latitude").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.site_form_lat)
                    .desired_width(120.0).font(egui::TextStyle::Monospace)
                    .hint_text("45.5172"));
                ui.end_row();

                ui.label(egui::RichText::new("Longitude").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.site_form_lon)
                    .desired_width(120.0).font(egui::TextStyle::Monospace)
                    .hint_text("-122.6766"));
                ui.end_row();

                ui.label(egui::RichText::new("Geofence (m)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.site_form_radius)
                    .desired_width(80.0).font(egui::TextStyle::Monospace));
                ui.end_row();

                ui.label(egui::RichText::new("Address").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.site_form_address)
                    .desired_width(250.0).font(egui::TextStyle::Monospace));
                ui.end_row();

                ui.label(egui::RichText::new("Notes").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::multiline(&mut ps.site_form_notes)
                    .desired_width(250.0).desired_rows(2).font(egui::TextStyle::Monospace));
                ui.end_row();
            });

            // Use current GPS position button
            if let Some(gps) = state.gps_position() {
                if gps.fix_type != "none" {
                    ui.add_space(2.0);
                    if ui.button(egui::RichText::new("USE CURRENT GPS")
                        .color(GREEN_COLLECT).size(FONT_SIZE_HUD)).clicked()
                    {
                        ps.site_form_lat = format!("{:.6}", gps.latitude);
                        ps.site_form_lon = format!("{:.6}", gps.longitude);
                    }
                }
            }

            ui.add_space(4.0);
            let can_submit = !ps.site_form_name.trim().is_empty();
            let btn_label = if is_edit { "UPDATE" } else { "CREATE" };
            if ui.add_enabled(can_submit,
                egui::Button::new(egui::RichText::new(btn_label)
                    .color(if can_submit { BLUE_PLAN } else { TEXT_SECONDARY }).size(FONT_SIZE_DATA)),
            ).clicked() {
                let name = ps.site_form_name.trim().to_string();
                let site_type = if ps.site_form_type.is_empty() { None } else { Some(ps.site_form_type.as_str()) };
                let lat: Option<f64> = ps.site_form_lat.trim().parse().ok();
                let lon: Option<f64> = ps.site_form_lon.trim().parse().ok();
                let radius: Option<f64> = ps.site_form_radius.trim().parse().ok();
                let address = if ps.site_form_address.is_empty() { None } else { Some(ps.site_form_address.as_str()) };
                let notes = if ps.site_form_notes.is_empty() { None } else { Some(ps.site_form_notes.as_str()) };

                let result = if let Some(edit_id) = ps.editing_site_id {
                    state.db().update_intel_site(edit_id, &name, site_type, lat, lon, None, address, notes, radius)
                        .map(|_| edit_id)
                } else {
                    state.db().create_intel_site(&name, site_type, lat, lon, None, address, notes, radius)
                };

                match result {
                    Ok(id) => {
                        tracing::info!("{} site: {} (id={})", if is_edit { "Updated" } else { "Created" }, name, id);
                        ps.show_site_form = false;
                        ps.editing_site_id = None;
                        clear_site_form(ps);
                        ps.last_poll = 0.0;
                    }
                    Err(e) => tracing::error!("Failed to save site: {}", e),
                }
            }
        });
}

fn show_sites_table(
    ui: &mut egui::Ui,
    state: &rf_web::AppState,
    ps: &mut PlanState,
    active_site_id: Option<i64>,
) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("sites_table")
                .num_columns(7)
                .spacing([12.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["ID", "NAME", "TYPE", "LAT/LON", "RADIUS", "ADDRESS", "ACTIONS"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let sites_snapshot: Vec<rf_db::IntelSite> = ps.sites.clone();
                    for site in &sites_snapshot {
                        let is_active = active_site_id == Some(site.id);
                        let is_selected = ps.selected_site_id == Some(site.id);
                        let name_color = if is_active { GREEN_COLLECT } else { TEXT_PRIMARY };

                        // ID (clickable for selection)
                        if ui.add(egui::Label::new(
                            egui::RichText::new(format!("{}", site.id)).color(name_color).size(FONT_SIZE_DATA)
                        ).sense(egui::Sense::click())).clicked() {
                            ps.selected_site_id = if is_selected { None } else { Some(site.id) };
                            // Force detail refresh
                            ps.site_dashboard_id = None;
                            ps.site_sessions_id = None;
                            ps.site_talkgroups_id = None;
                            ps.site_radio_ids_id = None;
                            ps.last_poll = 0.0;
                        }

                        // Name
                        ui.label(egui::RichText::new(&site.name)
                            .color(name_color).size(FONT_SIZE_DATA));

                        // Type
                        ui.label(egui::RichText::new(site.site_type.as_deref().unwrap_or("—"))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Coords
                        let coords = match (site.latitude, site.longitude) {
                            (Some(lat), Some(lon)) => format!("{:.4},{:.4}", lat, lon),
                            _ => "—".to_string(),
                        };
                        ui.label(egui::RichText::new(coords)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Radius
                        ui.label(egui::RichText::new(format!("{}m", site.geofence_radius_m as i64))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Address
                        ui.label(egui::RichText::new(site.address.as_deref().unwrap_or("—"))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Actions
                        ui.horizontal(|ui| {
                            if ui.small_button(egui::RichText::new("EDIT").color(BLUE_PLAN).size(FONT_SIZE_HUD)).clicked() {
                                ps.show_site_form = true;
                                ps.editing_site_id = Some(site.id);
                                ps.site_form_name = site.name.clone();
                                ps.site_form_type = site.site_type.clone().unwrap_or_default();
                                ps.site_form_lat = site.latitude.map(|v| format!("{:.6}", v)).unwrap_or_default();
                                ps.site_form_lon = site.longitude.map(|v| format!("{:.6}", v)).unwrap_or_default();
                                ps.site_form_radius = format!("{}", site.geofence_radius_m as i64);
                                ps.site_form_address = site.address.clone().unwrap_or_default();
                                ps.site_form_notes = site.notes.clone().unwrap_or_default();
                            }
                            if ui.small_button(egui::RichText::new("DEL").color(RED_WATCHDOG).size(FONT_SIZE_HUD)).clicked() {
                                let _ = state.db().delete_intel_site(site.id);
                                if ps.selected_site_id == Some(site.id) {
                                    ps.selected_site_id = None;
                                }
                                ps.last_poll = 0.0;
                            }
                        });
                        ui.end_row();
                    }
                });
        });
}

fn show_site_detail(
    ui: &mut egui::Ui,
    _state: &rf_web::AppState,
    ps: &mut PlanState,
    site_id: i64,
) {
    let site = match ps.sites.iter().find(|s| s.id == site_id) {
        Some(s) => s.clone(),
        None => return,
    };

    // Notes
    if let Some(ref notes) = site.notes {
        if !notes.is_empty() {
            ui.label(egui::RichText::new(notes)
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            ui.add_space(4.0);
        }
    }

    // Dashboard stats
    if ps.site_dashboard_id == Some(site_id) {
        if let Some(ref dash) = ps.site_dashboard {
            ui.label(egui::RichText::new("DASHBOARD")
                .color(BLUE_PLAN).size(FONT_SIZE_HUD));
            egui::Grid::new("site_dash_grid").num_columns(4).spacing([16.0, 2.0]).show(ui, |ui| {
                dash_cell(ui, "Sessions", dash.total_sessions);
                dash_cell(ui, "Grants", dash.total_grants);
                dash_cell(ui, "Talkgroups", dash.unique_tgids);
                dash_cell(ui, "Radio IDs", dash.unique_uids);
                ui.end_row();
                dash_cell(ui, "Encrypted", dash.encrypted_grants);
                dash_cell(ui, "Global Grants", dash.global_grants);
                ui.end_row();
            });

            // Active session indicator
            if let Some(ref sess) = dash.active_session {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Active session:")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.label(egui::RichText::new(format!("#{} since {}", sess.id, &sess.start_time))
                        .color(GREEN_COLLECT).size(FONT_SIZE_DATA));
                });
            }
        }
    }

    // Talkgroups
    if ps.site_talkgroups_id == Some(site_id) && !ps.site_talkgroups.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("TALKGROUPS ({})", ps.site_talkgroups.len()))
            .color(BLUE_PLAN).size(FONT_SIZE_HUD));
        egui::Grid::new("site_tgs_grid").num_columns(5).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
            for h in ["TGID", "NAME", "DEPT", "GRANTS", "LAST SEEN"] {
                ui.label(egui::RichText::new(h).color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
            }
            ui.end_row();
            for tg in &ps.site_talkgroups {
                let tgid = tg.get("tgid").and_then(|v| v.as_i64()).unwrap_or(0);
                let name = tg.get("name").and_then(|v| v.as_str()).unwrap_or("—");
                let dept = tg.get("department").and_then(|v| v.as_str()).unwrap_or("—");
                let grants = tg.get("grants").and_then(|v| v.as_i64()).unwrap_or(0);
                let last = tg.get("last_seen").and_then(|v| v.as_str()).unwrap_or("—");
                let last_short = last.get(..16).unwrap_or(last);
                let enc = tg.get("encrypted").and_then(|v| v.as_i64()).unwrap_or(0) > 0;

                let color = if enc { AMBER_WARNING } else { TEXT_PRIMARY };
                ui.label(egui::RichText::new(format!("{}", tgid)).color(color).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(name).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(dept).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(format!("{}", grants)).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(last_short).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.end_row();
            }
        });
    }

    // Radio IDs
    if ps.site_radio_ids_id == Some(site_id) && !ps.site_radio_ids.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("RADIO IDS ({})", ps.site_radio_ids.len()))
            .color(BLUE_PLAN).size(FONT_SIZE_HUD));
        egui::Grid::new("site_uids_grid").num_columns(4).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
            for h in ["UID", "OBSERVATIONS", "FIRST SEEN", "LAST SEEN"] {
                ui.label(egui::RichText::new(h).color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
            }
            ui.end_row();
            for uid_val in &ps.site_radio_ids {
                let uid = uid_val.get("uid").and_then(|v| v.as_i64()).unwrap_or(0);
                let obs = uid_val.get("observations").and_then(|v| v.as_i64()).unwrap_or(0);
                let first = uid_val.get("first_seen").and_then(|v| v.as_str()).unwrap_or("—");
                let last = uid_val.get("last_seen").and_then(|v| v.as_str()).unwrap_or("—");
                let first_short = first.get(..16).unwrap_or(first);
                let last_short = last.get(..16).unwrap_or(last);

                ui.label(egui::RichText::new(format!("{}", uid)).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(format!("{}", obs)).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(first_short).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.label(egui::RichText::new(last_short).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.end_row();
            }
        });
    }

    // Session history
    if ps.site_sessions_id == Some(site_id) && !ps.site_sessions.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("SESSION HISTORY ({})", ps.site_sessions.len()))
            .color(BLUE_PLAN).size(FONT_SIZE_HUD));
        egui::Grid::new("site_sessions_grid").num_columns(4).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
            for h in ["ID", "START", "END", "POSITION"] {
                ui.label(egui::RichText::new(h).color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
            }
            ui.end_row();
            for sess in &ps.site_sessions {
                let is_open = sess.end_time.is_none();
                let id_color = if is_open { GREEN_COLLECT } else { TEXT_PRIMARY };
                ui.label(egui::RichText::new(format!("{}", sess.id)).color(id_color).size(FONT_SIZE_DATA));
                let start_short = sess.start_time.get(..16).unwrap_or(&sess.start_time);
                ui.label(egui::RichText::new(start_short).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                let end_str = sess.end_time.as_deref()
                    .map(|s| s.get(..16).unwrap_or(s))
                    .unwrap_or("ACTIVE");
                let end_color = if is_open { GREEN_COLLECT } else { TEXT_SECONDARY };
                ui.label(egui::RichText::new(end_str).color(end_color).size(FONT_SIZE_DATA));
                let pos = match (sess.start_lat, sess.start_lon) {
                    (Some(lat), Some(lon)) => format!("{:.4},{:.4}", lat, lon),
                    _ => "—".to_string(),
                };
                ui.label(egui::RichText::new(pos).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.end_row();
            }
        });
    }
}

fn dash_cell(ui: &mut egui::Ui, label: &str, value: i64) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(format!("{}", value))
            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA).strong());
        ui.label(egui::RichText::new(label)
            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
    });
}

// ── Config tab ──────────────────────────────────────────

fn show_config_tab(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // 1. SDR Devices
    egui::CollapsingHeader::new(egui::RichText::new("SDR DEVICES").color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(true)
        .show(ui, |ui| show_config_sdr_devices_body(ui, state, ps));

    // 2. Antennas
    let ant_header = format!("ANTENNAS ({})", ps.antennas.len());
    egui::CollapsingHeader::new(egui::RichText::new(ant_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_config_antennas_body(ui, state, ps));

    // 3. Scan Configuration
    egui::CollapsingHeader::new(egui::RichText::new("SCAN CONFIGURATION").color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_config_scan_body(ui, state));

    // 4. GPS Configuration
    egui::CollapsingHeader::new(egui::RichText::new("GPS CONFIGURATION").color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_config_gps_body(ui, state));

    // 5. Audio Configuration
    egui::CollapsingHeader::new(egui::RichText::new("AUDIO CONFIGURATION").color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_config_audio_body(ui, state));
}

fn show_config_sdr_devices_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    let slots = state.sdr_slots();
    if slots.is_empty() {
        ui.label(egui::RichText::new("No SDR devices detected — simulation mode.")
            .color(AMBER_WARNING).size(FONT_SIZE_DATA));
        return;
    }

    let config = state.config();

    for slot in &slots {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(6))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                // Header: device name, serial, status
                ui.horizontal(|ui| {
                    let status_color = if slot.quarantined {
                        RED_WATCHDOG
                    } else if slot.alive {
                        GREEN_COLLECT
                    } else {
                        TEXT_SECONDARY
                    };
                    let status_label = if slot.quarantined {
                        "QUARANTINED"
                    } else if slot.alive {
                        "ONLINE"
                    } else {
                        "OFFLINE"
                    };

                    let display = if slot.user_name.is_empty() { &slot.label } else { &slot.user_name };
                    ui.label(egui::RichText::new(display)
                        .color(TEXT_PRIMARY).size(FONT_SIZE_DATA).strong());
                    ui.label(egui::RichText::new(format!("[{}]", slot.serial))
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    ui.label(egui::RichText::new(status_label)
                        .color(status_color).size(FONT_SIZE_HUD));
                    ui.label(egui::RichText::new(format!("role:{}", slot.role.to_uppercase()))
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    if !slot.assigned_bands.is_empty() {
                        ui.label(egui::RichText::new(format!("bands:{}", slot.assigned_bands.join(",")))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    }
                });

                ui.add_space(2.0);

                egui::Grid::new(format!("sdr_cfg_{}", slot.serial))
                    .num_columns(2)
                    .spacing([8.0, 2.0])
                    .show(ui, |ui| {
                        // Device name
                        ui.label(egui::RichText::new("Name").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        let name_edit = ps.device_name_edits
                            .entry(slot.serial.clone())
                            .or_insert_with(|| {
                                ps.device_names.get(&slot.serial).cloned().unwrap_or_default()
                            });
                        let resp = ui.add(egui::TextEdit::singleline(name_edit)
                            .desired_width(180.0).font(egui::TextStyle::Monospace)
                            .hint_text("display name"));
                        if resp.lost_focus() {
                            let trimmed = name_edit.trim().to_string();
                            let _ = state.db().set_device_name(&slot.serial, &trimmed);
                            ps.device_names.insert(slot.serial.clone(), trimmed);
                        }
                        ui.end_row();

                        // Gain
                        let dev_gain = config.per_device_gain.get(&slot.device_key)
                            .copied().unwrap_or(config.gain);
                        ui.label(egui::RichText::new("Gain (dB)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        let mut gain_f32 = dev_gain as f32;
                        if ui.add(egui::Slider::new(&mut gain_f32, 0.0..=49.6)
                            .step_by(0.1)
                            .text("dB")).changed()
                        {
                            state.update_config(|c| {
                                c.per_device_gain.insert(slot.device_key.clone(), gain_f32 as f64);
                            });
                        }
                        ui.end_row();

                        // AGC
                        let dev_agc = config.per_device_agc.get(&slot.device_key)
                            .copied().unwrap_or(false);
                        ui.label(egui::RichText::new("AGC").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        let mut agc = dev_agc;
                        if ui.checkbox(&mut agc, "").changed() {
                            state.update_config(|c| {
                                c.per_device_agc.insert(slot.device_key.clone(), agc);
                            });
                        }
                        ui.end_row();

                        // PPM correction
                        let dev_ppm = config.per_device_ppm.get(&slot.device_key)
                            .copied().unwrap_or(0.0);
                        ui.label(egui::RichText::new("PPM").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        let mut ppm_f32 = dev_ppm as f32;
                        if ui.add(egui::Slider::new(&mut ppm_f32, -100.0..=100.0)
                            .step_by(1.0)
                            .text("ppm")).changed()
                        {
                            state.update_config(|c| {
                                c.per_device_ppm.insert(slot.device_key.clone(), ppm_f32 as f64);
                            });
                        }
                        ui.end_row();

                        // Offset tuning
                        let dev_offset = config.per_device_offset_tuning.get(&slot.device_key)
                            .copied().unwrap_or(false);
                        ui.label(egui::RichText::new("Offset Tuning").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        let mut offset = dev_offset;
                        if ui.checkbox(&mut offset, "").changed() {
                            state.update_config(|c| {
                                c.per_device_offset_tuning.insert(slot.device_key.clone(), offset);
                            });
                        }
                        ui.end_row();

                        // Antenna assignment
                        ui.label(egui::RichText::new("Antenna").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        let current_ant = state.db().get_device_antenna(&slot.serial).ok().flatten();
                        let current_ant_id = current_ant.map(|a| a.antenna_id);
                        ui.horizontal(|ui| {
                            let mut selected = current_ant_id.unwrap_or(0);
                            egui::ComboBox::from_id_salt(format!("ant_sel_{}", slot.serial))
                                .width(160.0)
                                .selected_text(
                                    if selected == 0 {
                                        "None".to_string()
                                    } else {
                                        ps.antennas.iter()
                                            .find(|a| a.id == selected)
                                            .map(|a| a.name.clone())
                                            .unwrap_or_else(|| format!("#{}", selected))
                                    }
                                )
                                .show_ui(ui, |ui| {
                                    if ui.selectable_value(&mut selected, 0, "None").changed() {
                                        let _ = state.db().clear_device_antenna(&slot.serial);
                                        ps.last_poll = 0.0;
                                    }
                                    let antennas_snapshot = ps.antennas.clone();
                                    for ant in &antennas_snapshot {
                                        if ui.selectable_value(&mut selected, ant.id, &ant.name).changed() {
                                            let _ = state.db().set_device_antenna(&slot.serial, ant.id);
                                            ps.last_poll = 0.0;
                                        }
                                    }
                                });
                        });
                        ui.end_row();
                    });
            });
        ui.add_space(4.0);
    }
}

fn show_config_antennas_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(egui::RichText::new(if ps.show_antenna_form { "CANCEL" } else { "+ NEW" })
                .color(BLUE_PLAN).size(FONT_SIZE_DATA)).clicked()
            {
                ps.show_antenna_form = !ps.show_antenna_form;
                if ps.show_antenna_form {
                    ps.editing_antenna_id = None;
                    clear_antenna_form(ps);
                }
            }
        });
    });

    // Create/edit form
    if ps.show_antenna_form {
        show_antenna_form(ui, state, ps);
    }

    // Antenna table
    if ps.antennas.is_empty() {
        ui.label(egui::RichText::new("No antennas configured.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    } else {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_SURFACE)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("antennas_table")
                    .num_columns(7)
                    .spacing([12.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["NAME", "TYPE", "CONNECTOR", "FREQ RANGE", "GAIN", "ACTIVE", "ACTIONS"] {
                            ui.label(egui::RichText::new(h)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                        }
                        ui.end_row();

                        let antennas_snapshot = ps.antennas.clone();
                        for ant in &antennas_snapshot {
                            let color = if ant.active { TEXT_PRIMARY } else { TEXT_SECONDARY };
                            ui.label(egui::RichText::new(&ant.name).color(color).size(FONT_SIZE_DATA));
                            ui.label(egui::RichText::new(&ant.antenna_type).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                            ui.label(egui::RichText::new(&ant.connector).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                            let freq = match (ant.freq_min_mhz, ant.freq_max_mhz) {
                                (Some(min), Some(max)) => format!("{:.0}-{:.0} MHz", min, max),
                                (Some(min), None) => format!("{:.0}+ MHz", min),
                                _ => "—".to_string(),
                            };
                            ui.label(egui::RichText::new(freq).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                            let gain = ant.gain_dbi.map(|g| format!("{:.1} dBi", g)).unwrap_or_else(|| "—".to_string());
                            ui.label(egui::RichText::new(gain).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                            let active_str = if ant.active { "YES" } else { "NO" };
                            let active_color = if ant.active { GREEN_COLLECT } else { TEXT_SECONDARY };
                            ui.label(egui::RichText::new(active_str).color(active_color).size(FONT_SIZE_DATA));

                            ui.horizontal(|ui| {
                                if ui.small_button(egui::RichText::new("EDIT").color(BLUE_PLAN).size(FONT_SIZE_HUD)).clicked() {
                                    ps.show_antenna_form = true;
                                    ps.editing_antenna_id = Some(ant.id);
                                    ps.ant_form_name = ant.name.clone();
                                    ps.ant_form_type = ant.antenna_type.clone();
                                    ps.ant_form_connector = ant.connector.clone();
                                    ps.ant_form_freq_min = ant.freq_min_mhz.map(|v| format!("{:.1}", v)).unwrap_or_default();
                                    ps.ant_form_freq_max = ant.freq_max_mhz.map(|v| format!("{:.1}", v)).unwrap_or_default();
                                    ps.ant_form_gain = ant.gain_dbi.map(|v| format!("{:.1}", v)).unwrap_or_default();
                                    ps.ant_form_notes = ant.notes.clone();
                                }
                                if ui.small_button(egui::RichText::new("DEL").color(RED_WATCHDOG).size(FONT_SIZE_HUD)).clicked() {
                                    let _ = state.db().delete_antenna(ant.id);
                                    ps.last_poll = 0.0;
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
    }
}

fn clear_antenna_form(ps: &mut PlanState) {
    ps.ant_form_name.clear();
    ps.ant_form_type = "whip".to_string();
    ps.ant_form_connector = "SMA".to_string();
    ps.ant_form_freq_min.clear();
    ps.ant_form_freq_max.clear();
    ps.ant_form_gain.clear();
    ps.ant_form_notes.clear();
}

fn show_antenna_form(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    let is_edit = ps.editing_antenna_id.is_some();
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(8))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(if is_edit { "EDIT ANTENNA" } else { "CREATE ANTENNA" })
                .color(BLUE_PLAN).size(FONT_SIZE_HUD));
            ui.add_space(4.0);

            egui::Grid::new("antenna_form_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("Name *").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                crate::theme::required_text_field(ui, &mut ps.ant_form_name, 200.0, "Antenna name (required)");
                ui.end_row();

                ui.label(egui::RichText::new("Type").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.horizontal(|ui| {
                    for t in ["whip", "discone", "yagi", "dipole", "mag-mount", "other"] {
                        ui.selectable_value(&mut ps.ant_form_type, t.to_string(),
                            egui::RichText::new(t.to_uppercase()).size(FONT_SIZE_HUD));
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new("Connector").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.horizontal(|ui| {
                    for c in ["SMA", "BNC", "N", "SO-239", "MCX"] {
                        ui.selectable_value(&mut ps.ant_form_connector, c.to_string(),
                            egui::RichText::new(c).size(FONT_SIZE_HUD));
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new("Freq Min (MHz)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.ant_form_freq_min)
                    .desired_width(100.0).font(egui::TextStyle::Monospace).hint_text("25"));
                ui.end_row();

                ui.label(egui::RichText::new("Freq Max (MHz)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.ant_form_freq_max)
                    .desired_width(100.0).font(egui::TextStyle::Monospace).hint_text("1700"));
                ui.end_row();

                ui.label(egui::RichText::new("Gain (dBi)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::singleline(&mut ps.ant_form_gain)
                    .desired_width(80.0).font(egui::TextStyle::Monospace).hint_text("3.0"));
                ui.end_row();

                ui.label(egui::RichText::new("Notes").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                ui.add(egui::TextEdit::multiline(&mut ps.ant_form_notes)
                    .desired_width(200.0).desired_rows(2).font(egui::TextStyle::Monospace));
                ui.end_row();
            });

            ui.add_space(4.0);
            let can_submit = !ps.ant_form_name.trim().is_empty();
            let btn_label = if is_edit { "UPDATE" } else { "CREATE" };
            if ui.add_enabled(can_submit,
                egui::Button::new(egui::RichText::new(btn_label)
                    .color(if can_submit { BLUE_PLAN } else { TEXT_SECONDARY }).size(FONT_SIZE_DATA)),
            ).clicked() {
                let name = ps.ant_form_name.trim().to_string();
                let atype = ps.ant_form_type.clone();
                let connector = ps.ant_form_connector.clone();
                let freq_min: Option<f64> = ps.ant_form_freq_min.trim().parse().ok();
                let freq_max: Option<f64> = ps.ant_form_freq_max.trim().parse().ok();
                let gain: Option<f64> = ps.ant_form_gain.trim().parse().ok();
                let notes = ps.ant_form_notes.trim().to_string();

                let result = if let Some(edit_id) = ps.editing_antenna_id {
                    state.db().update_antenna(edit_id, &name, &atype, &connector, freq_min, freq_max, gain, &notes, true)
                        .map(|_| edit_id)
                } else {
                    state.db().create_antenna(&name, &atype, &connector, freq_min, freq_max, gain, &notes)
                };

                match result {
                    Ok(id) => {
                        tracing::info!("{} antenna: {} (id={})", if is_edit { "Updated" } else { "Created" }, name, id);
                        ps.show_antenna_form = false;
                        ps.editing_antenna_id = None;
                        clear_antenna_form(ps);
                        ps.last_poll = 0.0;
                    }
                    Err(e) => tracing::error!("Failed to save antenna: {}", e),
                }
            }
        });
}

fn show_config_scan_body(ui: &mut egui::Ui, state: &rf_web::AppState) {
    let config = state.config();

    egui::Grid::new("scan_config_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        // Threshold
        ui.label(egui::RichText::new("Threshold (dBFS)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut threshold = config.threshold as f32;
        if ui.add(egui::Slider::new(&mut threshold, -80.0..=-10.0)
            .step_by(0.5)
            .text("dBFS")).changed()
        {
            crate::commands::set_threshold(state, threshold as f64);
        }
        ui.end_row();

        // SNR margin
        ui.label(egui::RichText::new("SNR Margin (dB)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut snr = config.snr_margin as f32;
        if ui.add(egui::Slider::new(&mut snr, 0.0..=30.0)
            .step_by(0.5)
            .text("dB")).changed()
        {
            crate::commands::set_snr_margin(state, snr as f64);
        }
        ui.end_row();

        // Persistence min hits
        ui.label(egui::RichText::new("Persist Min Hits").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut hits = config.persist_min_hits as f32;
        if ui.add(egui::Slider::new(&mut hits, 1.0..=20.0)
            .step_by(1.0)).changed()
        {
            state.update_config(|c| { c.persist_min_hits = hits as u32; });
        }
        ui.end_row();

        // Persistence window
        ui.label(egui::RichText::new("Persist Window (s)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut window = config.persist_window as f32;
        if ui.add(egui::Slider::new(&mut window, 1.0..=60.0)
            .step_by(1.0)
            .text("s")).changed()
        {
            state.update_config(|c| { c.persist_window = window as u32; });
        }
        ui.end_row();

        // Debug logging
        ui.label(egui::RichText::new("Debug Logging").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut debug = config.debug_logging;
        if ui.checkbox(&mut debug, "").changed() {
            crate::commands::set_debug_logging(state, debug);
        }
        ui.end_row();
    });

    // Per-band thresholds
    if !config.per_band_threshold.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("PER-BAND THRESHOLDS")
            .color(BLUE_PLAN).size(FONT_SIZE_HUD));
        egui::Grid::new("per_band_thresh_grid").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
            let mut entries: Vec<_> = config.per_band_threshold.iter().collect();
            entries.sort_by_key(|(k, _)| (*k).clone());
            for (band, thresh) in entries {
                ui.label(egui::RichText::new(band).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                let mut t = *thresh as f32;
                if ui.add(egui::Slider::new(&mut t, -80.0..=-10.0)
                    .step_by(0.5)
                    .text("dBFS")).changed()
                {
                    let band_key = band.clone();
                    state.update_config(|c| {
                        c.per_band_threshold.insert(band_key, t as f64);
                    });
                }
                ui.end_row();
            }
        });
    }
}

fn show_config_gps_body(ui: &mut egui::Ui, state: &rf_web::AppState) {
    let config = state.config();

    // Current position display
    if let Some(gps) = state.gps_position() {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let fix_color = match gps.fix_type.as_str() {
                        "3d" | "dgps" => GREEN_COLLECT,
                        "2d" => AMBER_WARNING,
                        _ => TEXT_SECONDARY,
                    };
                    ui.label(egui::RichText::new(format!("FIX:{}", gps.fix_type.to_uppercase()))
                        .color(fix_color).size(FONT_SIZE_DATA));
                    ui.label(egui::RichText::new(format!("{:.6},{:.6}", gps.latitude, gps.longitude))
                        .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                    if let Some(alt) = gps.altitude_m {
                        ui.label(egui::RichText::new(format!("{:.0}m", alt))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    }
                    ui.label(egui::RichText::new(format!("src:{}", gps.source.to_uppercase()))
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    ui.label(egui::RichText::new(format!("sats:{}", gps.satellite_count))
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                });
            });
        ui.add_space(4.0);
    }

    egui::Grid::new("gps_config_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        // GPS source
        ui.label(egui::RichText::new("Source").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        ui.horizontal(|ui| {
            let sources = ["simulation", "external", "fixed", "none"];
            for src in sources {
                let current = config.gps_source == src;
                let color = if current { GREEN_COLLECT } else { TEXT_SECONDARY };
                if ui.selectable_label(current,
                    egui::RichText::new(src.to_uppercase()).color(color).size(FONT_SIZE_HUD),
                ).clicked() && !current {
                    state.update_config(|c| {
                        c.gps_source = src.to_string();
                        c.gps_enabled = src != "none";
                    });
                }
            }
        });
        ui.end_row();

        // Serial port (only for external)
        if config.gps_source == "external" {
            ui.label(egui::RichText::new("Port").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            let mut port = config.gps_port.clone();
            if ui.add(egui::TextEdit::singleline(&mut port)
                .desired_width(120.0).font(egui::TextStyle::Monospace)
                .hint_text("COM8")).lost_focus()
            {
                state.update_config(|c| { c.gps_port = port; });
            }
            ui.end_row();

            ui.label(egui::RichText::new("Baud").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            let mut baud = config.gps_baud as f32;
            if ui.add(egui::Slider::new(&mut baud, 4800.0..=115200.0)
                .step_by(1.0)
                .logarithmic(true)).changed()
            {
                state.update_config(|c| { c.gps_baud = baud as u32; });
            }
            ui.end_row();
        }

        // Fixed position (only for fixed source)
        if config.gps_source == "fixed" {
            ui.label(egui::RichText::new("Latitude").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            let mut lat_str = config.fixed_lat.map(|v| format!("{:.6}", v)).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut lat_str)
                .desired_width(120.0).font(egui::TextStyle::Monospace)
                .hint_text("45.5172")).lost_focus()
            {
                let val: Option<f64> = lat_str.parse().ok();
                state.update_config(|c| { c.fixed_lat = val; });
            }
            ui.end_row();

            ui.label(egui::RichText::new("Longitude").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            let mut lon_str = config.fixed_lon.map(|v| format!("{:.6}", v)).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut lon_str)
                .desired_width(120.0).font(egui::TextStyle::Monospace)
                .hint_text("-122.6766")).lost_focus()
            {
                let val: Option<f64> = lon_str.parse().ok();
                state.update_config(|c| { c.fixed_lon = val; });
            }
            ui.end_row();

            ui.label(egui::RichText::new("Altitude (m)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            let mut alt_str = config.fixed_alt_m.map(|v| format!("{:.0}", v)).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut alt_str)
                .desired_width(80.0).font(egui::TextStyle::Monospace)
                .hint_text("50")).lost_focus()
            {
                let val: Option<f64> = alt_str.parse().ok();
                state.update_config(|c| { c.fixed_alt_m = val; });
            }
            ui.end_row();
        }

        // Update rate
        ui.label(egui::RichText::new("Update Hz").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut hz = config.gps_update_hz as f32;
        if ui.add(egui::Slider::new(&mut hz, 1.0..=10.0)
            .step_by(1.0)
            .text("Hz")).changed()
        {
            state.update_config(|c| { c.gps_update_hz = hz as u8; });
        }
        ui.end_row();
    });
}

fn show_config_audio_body(ui: &mut egui::Ui, state: &rf_web::AppState) {
    // Show available output devices
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let default_name = host.default_output_device()
            .and_then(|d| d.name().ok())
            .unwrap_or_else(|| "None".to_string());

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Output Device:")
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            ui.label(egui::RichText::new(&default_name)
                .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                .family(egui::FontFamily::Monospace));
        });

        if let Ok(devices) = host.output_devices() {
            let names: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();
            if names.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Available:")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    for name in &names {
                        let color = if *name == default_name { GREEN_COLLECT } else { TEXT_SECONDARY };
                        ui.label(egui::RichText::new(name)
                            .color(color).size(FONT_SIZE_HUD));
                    }
                });
            }
        }

        ui.add_space(4.0);
    }

    let config = state.config();

    egui::Grid::new("audio_config_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        // Volume
        ui.label(egui::RichText::new("Volume").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut vol = config.volume as f32;
        if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0)
            .step_by(1.0)
            .text("%")).changed()
        {
            crate::commands::set_volume(state, vol as u8, config.muted);
        }
        ui.end_row();

        // Mute
        ui.label(egui::RichText::new("Muted").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut muted = config.muted;
        if ui.checkbox(&mut muted, "").changed() {
            crate::commands::set_volume(state, config.volume, muted);
        }
        ui.end_row();

        // Duck volume
        ui.label(egui::RichText::new("Duck Volume").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut duck = config.duck_volume as f32;
        if ui.add(egui::Slider::new(&mut duck, 0.0..=100.0)
            .step_by(1.0)
            .text("%")).changed()
        {
            state.update_config(|c| { c.duck_volume = duck as u8; });
        }
        ui.end_row();

        // Bandwidth
        ui.label(egui::RichText::new("Filter BW (Hz)").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut bw = config.bandwidth_hz as f32;
        if ui.add(egui::Slider::new(&mut bw, 0.0..=25000.0)
            .step_by(100.0)
            .text("Hz")).changed()
        {
            state.update_config(|c| { c.bandwidth_hz = bw as f64; });
        }
        ui.end_row();

        // Auto-clip
        ui.label(egui::RichText::new("Auto Clip").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        let mut clip = config.auto_clip_enabled;
        if ui.checkbox(&mut clip, "").changed() {
            state.update_config(|c| { c.auto_clip_enabled = clip; });
        }
        ui.end_row();
    });
}

// ── Helpers ──────────────────────────────────────────────

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    ui.label(egui::RichText::new(value).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
    ui.end_row();
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: Option<&serde_json::Value>) {
    ui.label(egui::RichText::new(label).color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    let v = value.and_then(|v| v.as_i64()).unwrap_or(0);
    ui.label(egui::RichText::new(format!("{}", v)).color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
    ui.end_row();
}

// ── Targets tab ─────────────────────────────────────────

fn show_targets_tab(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // 1. Observation targets — default open (primary section)
    let target_header = format!("OBSERVATION TARGETS ({})", ps.targets.len());
    egui::CollapsingHeader::new(egui::RichText::new(target_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(true)
        .show(ui, |ui| show_observation_targets_body(ui, state, ps));

    // 2. Collection requirements
    let req_header = format!("COLLECTION REQUIREMENTS ({})", ps.requirements.len());
    egui::CollapsingHeader::new(egui::RichText::new(req_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_collection_requirements_body(ui, state, ps));

    // 3. Auto-IQ rules
    let iq_header = format!("AUTO-IQ RULES ({})", ps.auto_iq_rules.len());
    egui::CollapsingHeader::new(egui::RichText::new(iq_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_auto_iq_rules_body(ui, state, ps));

    // 4. Scan packages
    let pkg_header = format!("SCAN PACKAGES ({})", ps.packages.len());
    egui::CollapsingHeader::new(egui::RichText::new(pkg_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_scan_packages_body(ui, state, ps));

    // 5. Observations
    let obs_header = if let Some(tid) = ps.selected_target_id {
        format!("OBSERVATIONS — TARGET #{} ({})", tid, ps.observations.len())
    } else {
        format!("OBSERVATIONS ({})", ps.observations.len())
    };
    egui::CollapsingHeader::new(egui::RichText::new(obs_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_observations_body(ui, ps));

    // 6. Observation alerts
    let alert_header = format!("OBSERVATION ALERTS ({})", ps.obs_alerts.len());
    egui::CollapsingHeader::new(egui::RichText::new(alert_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_observation_alerts_body(ui, state, ps));
}

// ── Observation Targets ─────────────────────────────────

fn show_observation_targets_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // Create/Edit form toggle
    ui.horizontal(|ui| {
        let form_label = if ps.show_target_form {
            "CANCEL"
        } else {
            "+ NEW TARGET"
        };
        if ui.add(egui::Button::new(
            egui::RichText::new(form_label).color(BLUE_PLAN).size(FONT_SIZE_DATA)
        )).clicked() {
            ps.show_target_form = !ps.show_target_form;
            if !ps.show_target_form {
                // Reset edit state on cancel
                ps.editing_target_id = None;
            }
            if ps.show_target_form && ps.editing_target_id.is_none() {
                // Reset form for new target
                ps.target_form_type = "frequency".to_string();
                ps.target_form_key.clear();
                ps.target_form_label.clear();
                ps.target_form_priority = "0".to_string();
                ps.target_form_notes.clear();
            }
        }
    });

    // Create/Edit form
    if ps.show_target_form {
        let is_editing = ps.editing_target_id.is_some();
        let form_title = if is_editing { "EDIT TARGET" } else { "NEW TARGET" };

        egui::Frame::NONE
            .inner_margin(egui::Margin::same(8))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(form_title)
                    .color(BLUE_PLAN).size(FONT_SIZE_DATA).strong());
                ui.add_space(4.0);

                egui::Grid::new("target_form_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Type").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        if is_editing {
                            // Type is immutable when editing
                            ui.label(egui::RichText::new(&ps.target_form_type)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                        } else {
                            let old_type = ps.target_form_type.clone();
                            egui::ComboBox::from_id_salt("target_type_combo")
                                .selected_text(&ps.target_form_type)
                                .show_ui(ui, |ui| {
                                    for t in ["frequency", "talkgroup", "radio_id", "channel", "network", "site"] {
                                        ui.selectable_value(&mut ps.target_form_type, t.to_string(), t);
                                    }
                                });
                            // Clear key when type changes
                            if ps.target_form_type != old_type {
                                ps.target_form_key.clear();
                            }
                        }
                        ui.end_row();

                        ui.label(egui::RichText::new("Key *").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        if is_editing {
                            // Key is immutable when editing
                            ui.label(egui::RichText::new(&ps.target_form_key)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));
                        } else {
                            // Auto-suggest key based on type
                            let suggestions = match ps.target_form_type.as_str() {
                                "frequency" => &ps.suggest_frequencies,
                                "talkgroup" => &ps.suggest_talkgroups,
                                "radio_id" => &ps.suggest_radio_ids,
                                "channel" => &ps.suggest_channels,
                                "network" => &ps.suggest_networks,
                                "site" => &ps.suggest_sites,
                                _ => &ps.suggest_frequencies,
                            };
                            let hint = match ps.target_form_type.as_str() {
                                "frequency" => "e.g. 155.7300",
                                "talkgroup" => "e.g. 1001",
                                "radio_id" => "e.g. 12345",
                                "channel" => "channel name",
                                "network" => "system name",
                                "site" => "site name",
                                _ => "key value",
                            };
                            ui.vertical(|ui| {
                                crate::theme::required_text_field(ui, &mut ps.target_form_key, 250.0, hint);
                                // Show filtered suggestions below the text field
                                let filter = ps.target_form_key.trim().to_lowercase();
                                if !filter.is_empty() && !suggestions.is_empty() {
                                    let matches: Vec<&String> = suggestions.iter()
                                        .filter(|s| s.to_lowercase().contains(&filter))
                                        .take(8)
                                        .collect();
                                    if !matches.is_empty() {
                                        egui::Frame::NONE
                                            .fill(BG_ELEVATED)
                                            .inner_margin(egui::Margin::same(2))
                                            .corner_radius(2.0)
                                            .show(ui, |ui| {
                                                for m in &matches {
                                                    if ui.add(egui::Label::new(
                                                        egui::RichText::new(m.as_str())
                                                            .color(CYAN_P25).size(FONT_SIZE_DATA)
                                                            .family(egui::FontFamily::Monospace)
                                                    ).sense(egui::Sense::click())).clicked() {
                                                        // For talkgroup suggestions like "1001 — PPB Dispatch", extract just the ID
                                                        if ps.target_form_type == "talkgroup" {
                                                            if let Some(id_part) = m.split(" — ").next() {
                                                                ps.target_form_key = id_part.trim().to_string();
                                                            } else {
                                                                ps.target_form_key = m.to_string();
                                                            }
                                                        } else {
                                                            ps.target_form_key = m.to_string();
                                                        }
                                                    }
                                                }
                                            });
                                    }
                                }
                            });
                        }
                        ui.end_row();

                        ui.label(egui::RichText::new("Label").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        ui.add(egui::TextEdit::singleline(&mut ps.target_form_label)
                            .desired_width(250.0)
                            .hint_text("Optional label"));
                        ui.end_row();

                        ui.label(egui::RichText::new("Priority").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        egui::ComboBox::from_id_salt("target_priority_combo")
                            .selected_text(match ps.target_form_priority.as_str() {
                                "3" => "critical (3)",
                                "2" => "high (2)",
                                "1" => "medium (1)",
                                "0" => "low (0)",
                                other => other,
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut ps.target_form_priority, "3".to_string(), "critical (3)");
                                ui.selectable_value(&mut ps.target_form_priority, "2".to_string(), "high (2)");
                                ui.selectable_value(&mut ps.target_form_priority, "1".to_string(), "medium (1)");
                                ui.selectable_value(&mut ps.target_form_priority, "0".to_string(), "low (0)");
                            });
                        ui.end_row();

                        ui.label(egui::RichText::new("Notes").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                        ui.add(egui::TextEdit::singleline(&mut ps.target_form_notes)
                            .desired_width(250.0));
                        ui.end_row();
                    });

                ui.add_space(4.0);

                if is_editing {
                    // UPDATE button
                    if ui.add(egui::Button::new(
                        egui::RichText::new("UPDATE").color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                    )).clicked() {
                        if let Some(edit_id) = ps.editing_target_id {
                            let priority = ps.target_form_priority.parse::<i32>().ok();
                            let label = if ps.target_form_label.is_empty() { None } else { Some(ps.target_form_label.as_str()) };
                            let notes = if ps.target_form_notes.is_empty() { None } else { Some(ps.target_form_notes.as_str()) };
                            if state.db().update_observation_target(edit_id, label, priority, notes).is_ok() {
                                ps.editing_target_id = None;
                                ps.show_target_form = false;
                                ps.target_form_key.clear();
                                ps.target_form_label.clear();
                                ps.target_form_notes.clear();
                                ps.last_poll = 0.0;
                            }
                        }
                    }
                } else {
                    // CREATE button — disabled when key is empty
                    let can_create = !ps.target_form_key.trim().is_empty();
                    if ui.add_enabled(can_create, egui::Button::new(
                        egui::RichText::new("CREATE").color(if can_create { GREEN_COLLECT } else { TEXT_SECONDARY }).size(FONT_SIZE_DATA)
                    )).clicked() {
                        let priority = ps.target_form_priority.parse::<i32>().unwrap_or(0);
                        let label = if ps.target_form_label.is_empty() { None } else { Some(ps.target_form_label.as_str()) };
                        let notes = if ps.target_form_notes.is_empty() { None } else { Some(ps.target_form_notes.as_str()) };
                        if state.db().create_observation_target(
                            &ps.target_form_type, &ps.target_form_key,
                            label, None, priority, notes,
                        ).is_ok() {
                            ps.target_form_key.clear();
                            ps.target_form_label.clear();
                            ps.target_form_notes.clear();
                            ps.show_target_form = false;
                            ps.last_poll = 0.0;
                        }
                    }
                }
            });
    }

    if ps.targets.is_empty() {
        ui.label(egui::RichText::new("No observation targets defined.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Targets table
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("targets_table")
                .num_columns(8)
                .spacing([8.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["TYPE", "KEY", "LABEL", "PRI", "COVERAGE", "CREATED", "", ""] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let targets_snapshot = ps.targets.clone();
                    for t in &targets_snapshot {
                        // Type
                        let type_color = match t.target_type.as_str() {
                            "frequency" => GREEN_COLLECT,
                            "talkgroup" => CYAN_P25,
                            "uid" => AMBER_WARNING,
                            _ => TEXT_PRIMARY,
                        };
                        ui.label(egui::RichText::new(t.target_type.to_uppercase())
                            .color(type_color).size(FONT_SIZE_DATA));

                        // Key (clickable to filter observations)
                        let is_selected = ps.selected_target_id == Some(t.id);
                        let key_color = if is_selected { CYAN_P25 } else { TEXT_PRIMARY };
                        if ui.add(egui::Label::new(
                            egui::RichText::new(&t.target_key)
                                .color(key_color).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace)
                        ).sense(egui::Sense::click())).clicked() {
                            ps.selected_target_id = if is_selected { None } else { Some(t.id) };
                            ps.last_poll = 0.0;
                        }

                        // Label
                        let label = t.target_label.as_deref().unwrap_or("—");
                        ui.label(egui::RichText::new(label)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                        // Priority
                        let pri_color = if t.priority > 0 { AMBER_WARNING } else { TEXT_SECONDARY };
                        ui.label(egui::RichText::new(format!("{}", t.priority))
                            .color(pri_color).size(FONT_SIZE_DATA));

                        // Coverage target hours
                        ui.label(egui::RichText::new(format!("{:.0}h", t.coverage_target_hours))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Created
                        let created = t.created_at.get(..10).unwrap_or(&t.created_at);
                        ui.label(egui::RichText::new(created)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Edit button
                        if ui.add(egui::Button::new(
                            egui::RichText::new("EDIT").color(BLUE_PLAN).size(FONT_SIZE_HUD)
                        ).small()).clicked() {
                            ps.editing_target_id = Some(t.id);
                            ps.target_form_type = t.target_type.clone();
                            ps.target_form_key = t.target_key.clone();
                            ps.target_form_label = t.target_label.clone().unwrap_or_default();
                            ps.target_form_priority = format!("{}", t.priority);
                            ps.target_form_notes = t.notes.clone().unwrap_or_default();
                            ps.show_target_form = true;
                        }

                        // Delete button with confirmation
                        if ps.confirm_delete_target == Some(t.id) {
                            // Show confirm/cancel
                            ui.horizontal(|ui| {
                                if ui.add(egui::Button::new(
                                    egui::RichText::new("CONFIRM").color(RED_WATCHDOG).size(FONT_SIZE_HUD)
                                ).small()).clicked() {
                                    let _ = state.db().delete_observation_target(t.id);
                                    ps.confirm_delete_target = None;
                                    ps.last_poll = 0.0;
                                }
                                if ui.add(egui::Button::new(
                                    egui::RichText::new("NO").color(TEXT_SECONDARY).size(FONT_SIZE_HUD)
                                ).small()).clicked() {
                                    ps.confirm_delete_target = None;
                                }
                            });
                        } else if ui.add(egui::Button::new(
                            egui::RichText::new("DEL").color(RED_WATCHDOG).size(FONT_SIZE_HUD)
                        ).small()).clicked() {
                            ps.confirm_delete_target = Some(t.id);
                        }

                        ui.end_row();
                    }
                });
        });

    // Target filter indicator
    if let Some(tid) = ps.selected_target_id {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Filtering observations by target #{}", tid))
                .color(CYAN_P25).size(FONT_SIZE_HUD));
            if ui.small_button(egui::RichText::new("CLEAR").color(TEXT_SECONDARY).size(FONT_SIZE_HUD)).clicked() {
                ps.selected_target_id = None;
                ps.last_poll = 0.0;
            }
        });
    }
}

// ── Collection Requirements ─────────────────────────────

fn show_collection_requirements_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // Create form toggle
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new(
            egui::RichText::new(if ps.show_req_form { "CANCEL" } else { "+ NEW REQUIREMENT" })
                .color(BLUE_PLAN).size(FONT_SIZE_DATA)
        )).clicked() {
            ps.show_req_form = !ps.show_req_form;
        }
    });

    if ps.show_req_form {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(8))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Label:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::TextEdit::singleline(&mut ps.req_form_label)
                        .desired_width(250.0)
                        .hint_text("e.g. Verify CC on 772.6063"));

                    ui.label(egui::RichText::new("Type:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    egui::ComboBox::from_id_salt("req_type_combo")
                        .selected_text(&ps.req_form_type)
                        .show_ui(ui, |ui| {
                            for t in ["manual", "auto_detect", "coverage", "signal_check"] {
                                ui.selectable_value(&mut ps.req_form_type, t.to_string(), t);
                            }
                        });

                    if ui.add(egui::Button::new(
                        egui::RichText::new("CREATE").color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                    )).clicked() && !ps.req_form_label.is_empty() {
                        if state.db().create_collection_requirement(
                            &ps.req_form_label, &ps.req_form_type, None,
                        ).is_ok() {
                            ps.req_form_label.clear();
                            ps.show_req_form = false;
                            ps.last_poll = 0.0;
                        }
                    }
                });
            });
    }

    if ps.requirements.is_empty() {
        ui.label(egui::RichText::new("No collection requirements defined.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("requirements_table")
                .num_columns(6)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["STATUS", "LABEL", "TYPE", "CHECKED", "CREATED", ""] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let reqs_snapshot = ps.requirements.clone();
                    for req in &reqs_snapshot {
                        // Met checkbox
                        let met_str = if req.met { "✓" } else { "○" };
                        let met_color = if req.met { GREEN_COLLECT } else { TEXT_SECONDARY };
                        if ui.add(egui::Label::new(
                            egui::RichText::new(met_str).color(met_color).size(FONT_SIZE_DATA)
                        ).sense(egui::Sense::click())).clicked() {
                            let _ = state.db().toggle_collection_requirement(req.id);
                            ps.last_poll = 0.0;
                        }

                        // Label
                        ui.label(egui::RichText::new(&req.label)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                        // Type
                        ui.label(egui::RichText::new(&req.check_type)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Last checked
                        let checked = req.last_checked.as_deref()
                            .map(|s| s.get(..16).unwrap_or(s))
                            .unwrap_or("never");
                        ui.label(egui::RichText::new(checked)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Created
                        let created = req.created_at.get(..10).unwrap_or(&req.created_at);
                        ui.label(egui::RichText::new(created)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Delete button with confirmation
                        if ps.confirm_delete_req == Some(req.id) {
                            ui.horizontal(|ui| {
                                if ui.add(egui::Button::new(
                                    egui::RichText::new("YES").color(RED_WATCHDOG).size(FONT_SIZE_HUD)
                                ).small()).clicked() {
                                    let _ = state.db().delete_collection_requirement(req.id);
                                    ps.confirm_delete_req = None;
                                    ps.last_poll = 0.0;
                                }
                                if ui.add(egui::Button::new(
                                    egui::RichText::new("NO").color(TEXT_SECONDARY).size(FONT_SIZE_HUD)
                                ).small()).clicked() {
                                    ps.confirm_delete_req = None;
                                }
                            });
                        } else if ui.add(egui::Button::new(
                            egui::RichText::new("DEL").color(RED_WATCHDOG).size(FONT_SIZE_HUD)
                        ).small()).clicked() {
                            ps.confirm_delete_req = Some(req.id);
                        }

                        ui.end_row();
                    }
                });
        });

    // Summary
    let met_count = ps.requirements.iter().filter(|r| r.met).count();
    let total = ps.requirements.len();
    let color = if met_count == total { GREEN_COLLECT } else { AMBER_WARNING };
    ui.label(egui::RichText::new(format!("{}/{} requirements met", met_count, total))
        .color(color).size(FONT_SIZE_HUD));
}

// ── Auto-IQ Rules ───────────────────────────────────────

fn show_auto_iq_rules_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // Create form toggle
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new(
            egui::RichText::new(if ps.show_iq_rule_form { "CANCEL" } else { "+ NEW RULE" })
                .color(BLUE_PLAN).size(FONT_SIZE_DATA)
        )).clicked() {
            ps.show_iq_rule_form = !ps.show_iq_rule_form;
        }
    });

    if ps.show_iq_rule_form {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(8))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Trigger:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    egui::ComboBox::from_id_salt("iq_rule_type_combo")
                        .selected_text(&ps.iq_rule_form_type)
                        .show_ui(ui, |ui| {
                            for t in ["frequency", "talkgroup", "encryption", "new_uid", "emergency"] {
                                ui.selectable_value(&mut ps.iq_rule_form_type, t.to_string(), t);
                            }
                        });

                    ui.label(egui::RichText::new("Max dur (s):").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::TextEdit::singleline(&mut ps.iq_rule_form_duration)
                        .desired_width(50.0));

                    if ui.add(egui::Button::new(
                        egui::RichText::new("CREATE").color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                    )).clicked() {
                        let duration = ps.iq_rule_form_duration.parse::<i32>().unwrap_or(30);
                        if state.db().create_auto_iq_rule(
                            &ps.iq_rule_form_type, None, None, duration,
                        ).is_ok() {
                            ps.show_iq_rule_form = false;
                            ps.last_poll = 0.0;
                        }
                    }
                });
            });
    }

    if ps.auto_iq_rules.is_empty() {
        ui.label(egui::RichText::new("No auto-IQ capture rules defined.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("auto_iq_rules_table")
                .num_columns(5)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["ENABLED", "TRIGGER", "MAX DUR", "CREATED", ""] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let rules_snapshot = ps.auto_iq_rules.clone();
                    for rule in &rules_snapshot {
                        // Enabled toggle
                        let en_str = if rule.enabled { "ON" } else { "OFF" };
                        let en_color = if rule.enabled { GREEN_COLLECT } else { TEXT_SECONDARY };
                        if ui.add(egui::Label::new(
                            egui::RichText::new(en_str).color(en_color).size(FONT_SIZE_DATA)
                        ).sense(egui::Sense::click())).clicked() {
                            let _ = state.db().toggle_auto_iq_rule(rule.id, !rule.enabled);
                            ps.last_poll = 0.0;
                        }

                        // Trigger type
                        ui.label(egui::RichText::new(rule.trigger_type.to_uppercase())
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                        // Max duration
                        ui.label(egui::RichText::new(format!("{}s", rule.max_duration_sec))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Created
                        let created = rule.created_at.get(..10).unwrap_or(&rule.created_at);
                        ui.label(egui::RichText::new(created)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Delete
                        if ui.add(egui::Button::new(
                            egui::RichText::new("×").color(RED_WATCHDOG).size(FONT_SIZE_DATA)
                        ).small()).clicked() {
                            let _ = state.db().delete_auto_iq_rule(rule.id);
                            ps.last_poll = 0.0;
                        }

                        ui.end_row();
                    }
                });
        });
}

// ── Scan Packages ───────────────────────────────────────

fn show_scan_packages_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    if ps.packages.is_empty() {
        ui.label(egui::RichText::new("No scan packages defined.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Package pills — collect pill data to avoid borrow conflict
    let pill_data: Vec<(i64, String)> = ps.packages.iter()
        .map(|p| (p.id, format!("{} ({})", p.name, p.item_count)))
        .collect();

    ui.horizontal_wrapped(|ui| {
        for (pkg_id, label) in &pill_data {
            let is_selected = ps.selected_package_id == Some(*pkg_id);
            let color = if is_selected { CYAN_P25 } else { TEXT_PRIMARY };
            if ui.selectable_label(is_selected,
                egui::RichText::new(label).color(color).size(FONT_SIZE_DATA)
            ).clicked() {
                if is_selected {
                    ps.selected_package_id = None;
                    ps.package_items.clear();
                } else {
                    ps.selected_package_id = Some(*pkg_id);
                    if let Ok(items) = state.db().get_package_items(*pkg_id) {
                        ps.package_items = items;
                    }
                }
            }
        }
    });

    // Package items
    if ps.selected_package_id.is_some() && !ps.package_items.is_empty() {
        ui.add_space(4.0);
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_SURFACE)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("package_items_table")
                    .num_columns(5)
                    .spacing([10.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["#", "TYPE", "NAME", "TGID", "FREQ"] {
                            ui.label(egui::RichText::new(h)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                        }
                        ui.end_row();

                        for item in &ps.package_items {
                            ui.label(egui::RichText::new(format!("{}", item.target_index))
                                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.label(egui::RichText::new(&item.target_type)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                            ui.label(egui::RichText::new(&item.target_name)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                            let tgid = item.tgid
                                .map(|t| format!("{}", t))
                                .unwrap_or_else(|| "—".to_string());
                            ui.label(egui::RichText::new(&tgid)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            let freq = item.freq_mhz
                                .map(|f| format!("{:.4}", f))
                                .unwrap_or_else(|| "—".to_string());
                            ui.label(egui::RichText::new(&freq)
                                .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.end_row();
                        }
                    });
            });
    }
}

// ── Observations ────────────────────────────────────────

fn show_observations_body(ui: &mut egui::Ui, ps: &PlanState) {
    if ps.observations.is_empty() {
        ui.label(egui::RichText::new("No observations recorded.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("observations_table")
                .num_columns(8)
                .spacing([8.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["TARGET", "TYPE", "FREQ", "TG", "UID", "ENCR", "SIG", "TIME"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    for obs in ps.observations.iter().take(50) {
                        // Target key
                        let target = obs.target_key.as_deref().unwrap_or("—");
                        ui.label(egui::RichText::new(target)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Observation type
                        ui.label(egui::RichText::new(&obs.observation_type)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Freq
                        let freq = obs.freq_mhz
                            .map(|f| format!("{:.4}", f))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&freq)
                            .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // TG
                        let tg = obs.tgid
                            .map(|t| format!("{}", t))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&tg)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // UID
                        let uid = obs.uid
                            .map(|u| format!("{}", u))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&uid)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Encrypted
                        let enc = if obs.encrypted { "ENC" } else { "—" };
                        let enc_color = if obs.encrypted { RED_WATCHDOG } else { TEXT_SECONDARY };
                        ui.label(egui::RichText::new(enc)
                            .color(enc_color).size(FONT_SIZE_DATA));

                        // Signal
                        let sig = obs.signal_dbfs
                            .map(|s| format!("{:.1}", s))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&sig)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Time
                        let time = obs.start_time.get(..16).unwrap_or(&obs.start_time);
                        ui.label(egui::RichText::new(time)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        ui.end_row();
                    }
                });
        });
}

// ── Observation Alerts ──────────────────────────────────

fn show_observation_alerts_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    if ps.obs_alerts.is_empty() {
        ui.label(egui::RichText::new("No observation alerts configured.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("obs_alerts_table")
                .num_columns(6)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["ENABLED", "TARGET", "TYPE", "COOLDOWN", "FIRED", "LAST"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let alerts_snapshot = ps.obs_alerts.clone();
                    for alert in &alerts_snapshot {
                        // Enabled toggle
                        let en_str = if alert.enabled { "ON" } else { "OFF" };
                        let en_color = if alert.enabled { GREEN_COLLECT } else { TEXT_SECONDARY };
                        if ui.add(egui::Label::new(
                            egui::RichText::new(en_str).color(en_color).size(FONT_SIZE_DATA)
                        ).sense(egui::Sense::click())).clicked() {
                            let _ = state.db().toggle_observation_alert(alert.id, !alert.enabled);
                            ps.last_poll = 0.0;
                        }

                        // Target ID
                        ui.label(egui::RichText::new(format!("#{}", alert.target_id))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Alert type
                        ui.label(egui::RichText::new(&alert.alert_type)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                        // Cooldown
                        ui.label(egui::RichText::new(format!("{}s", alert.cooldown_sec))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Fire count
                        let fire_color = if alert.fire_count > 0 { AMBER_WARNING } else { TEXT_SECONDARY };
                        ui.label(egui::RichText::new(format!("{}", alert.fire_count))
                            .color(fire_color).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Last fired
                        let last = alert.last_fired.as_deref()
                            .map(|s| s.get(..16).unwrap_or(s))
                            .unwrap_or("never");
                        ui.label(egui::RichText::new(last)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        ui.end_row();
                    }
                });
        });
}

// ══════════════════════════════════════════════════════════
//  DATA TAB — Query Builder
// ══════════════════════════════════════════════════════════

/// Pre-built intelligence queries with name and SQL.
const PREBUILT_QUERIES: &[(&str, &str)] = &[
    ("Active talkgroups (last 24h)", "SELECT tgid, tg_name, department, priority, encryption_type, last_seen FROM network_talkgroups WHERE last_seen > datetime('now', '-24 hours') ORDER BY last_seen DESC LIMIT 100"),
    ("Encrypted traffic sessions", "SELECT id, uid, tgid, freq_mhz, modulation, duration_sec, avg_signal, start_time FROM traffic_sessions WHERE encrypted = 1 ORDER BY start_time DESC LIMIT 100"),
    ("Top 20 busiest frequencies", "SELECT freq_mhz, COUNT(*) as session_count, SUM(duration_sec) as total_sec, AVG(avg_signal) as avg_sig FROM traffic_sessions GROUP BY freq_mhz ORDER BY session_count DESC LIMIT 20"),
    ("Recent channel grants", "SELECT id, tgid, source_uid, freq_mhz, encrypted, algorithm, created_at FROM channel_grants ORDER BY created_at DESC LIMIT 100"),
    ("Key rotation events", "SELECT id, tgid, system, old_algorithm, new_algorithm, old_key_id, new_key_id, detected_at FROM key_rotation_events ORDER BY detected_at DESC LIMIT 50"),
    ("Radio ID sightings (last 48h)", "SELECT uid, tgid, freq_mhz, system, first_seen, last_seen FROM radio_id_sightings WHERE last_seen > datetime('now', '-48 hours') ORDER BY last_seen DESC LIMIT 100"),
    ("Observation targets summary", "SELECT t.id, t.target_type, t.target_key, t.target_label, t.priority, COUNT(o.id) as obs_count FROM observation_targets t LEFT JOIN observations o ON o.target_id = t.id GROUP BY t.id ORDER BY t.priority DESC"),
    ("Signals by classification", "SELECT classification, COUNT(*) as count, AVG(power_dbfs) as avg_power, MIN(freq_mhz) as min_freq, MAX(freq_mhz) as max_freq FROM signals GROUP BY classification ORDER BY count DESC"),
    ("Recording stats by type", "SELECT rec_type, COUNT(*) as count, SUM(duration_sec) as total_duration, SUM(file_size_bytes) as total_bytes FROM recordings GROUP BY rec_type"),
    ("Database table sizes", "SELECT name, (SELECT COUNT(*) FROM pragma_table_info(name)) as columns FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"),
];

fn show_data_tab(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // 1. Pre-built queries — default open
    egui::CollapsingHeader::new(egui::RichText::new("INTELLIGENCE QUERIES").color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(true)
        .show(ui, |ui| show_prebuilt_queries_body(ui, ps));

    // 2. SQL editor — default open
    egui::CollapsingHeader::new(egui::RichText::new("SQL EDITOR").color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(true)
        .show(ui, |ui| show_sql_editor_body(ui, state, ps));

    // 3. Results table
    if ps.query_result.is_some() {
        let result_header = if let Some(ref r) = ps.query_result {
            format!("RESULTS — {} rows in {}ms", r.row_count, r.elapsed_ms)
        } else {
            "RESULTS".to_string()
        };
        egui::CollapsingHeader::new(egui::RichText::new(result_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
            .default_open(true)
            .show(ui, |ui| show_query_results_body(ui, ps));
    }

    // 4. Saved queries
    let saved_header = format!("SAVED QUERIES ({})", ps.saved_queries.len());
    egui::CollapsingHeader::new(egui::RichText::new(saved_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_saved_queries_body(ui, state, ps));

    // 5. Schema browser
    let schema_header = format!("SCHEMA BROWSER ({} tables)", ps.schema.len());
    egui::CollapsingHeader::new(egui::RichText::new(schema_header).color(BLUE_PLAN).size(FONT_SIZE_HEADER).strong())
        .default_open(false)
        .show(ui, |ui| show_schema_browser_body(ui, state, ps));
}

// ── Pre-built Queries ────────────────────────────────────

fn show_prebuilt_queries_body(ui: &mut egui::Ui, ps: &mut PlanState) {
    ui.horizontal_wrapped(|ui| {
        for (i, (name, _sql)) in PREBUILT_QUERIES.iter().enumerate() {
            let is_selected = ps.selected_prebuilt == Some(i);
            let color = if is_selected { CYAN_P25 } else { TEXT_PRIMARY };
            if ui.selectable_label(is_selected,
                egui::RichText::new(*name).color(color).size(FONT_SIZE_DATA)
            ).clicked() {
                ps.selected_prebuilt = Some(i);
                ps.sql_input = _sql.to_string();
            }
        }
    });
}

// ── SQL Editor ───────────────────────────────────────────

fn show_sql_editor_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // SQL text area
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut ps.sql_input)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("SELECT * FROM signals LIMIT 50"),
            );
        });

    ui.add_space(4.0);

    // Action buttons
    ui.horizontal(|ui| {
        let can_execute = !ps.sql_input.trim().is_empty();

        if ui.add_enabled(can_execute, egui::Button::new(
            egui::RichText::new("EXECUTE").color(GREEN_COLLECT).size(FONT_SIZE_DATA)
        )).clicked() {
            match state.db().execute_query(&ps.sql_input) {
                Ok(result) => {
                    ps.query_error = None;
                    ps.query_result = Some(result);
                }
                Err(e) => {
                    ps.query_error = Some(e);
                    ps.query_result = None;
                }
            }
        }

        ui.add_space(8.0);

        if ui.add_enabled(can_execute, egui::Button::new(
            egui::RichText::new("CLEAR").color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
        )).clicked() {
            ps.sql_input.clear();
            ps.query_result = None;
            ps.query_error = None;
            ps.selected_prebuilt = None;
        }

        ui.add_space(16.0);

        // Save query
        if can_execute {
            ui.label(egui::RichText::new("Save as:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            ui.add(egui::TextEdit::singleline(&mut ps.save_query_name)
                .desired_width(150.0)
                .hint_text("query name"));

            if ui.add_enabled(!ps.save_query_name.trim().is_empty(), egui::Button::new(
                egui::RichText::new("SAVE").color(BLUE_PLAN).size(FONT_SIZE_DATA)
            )).clicked() {
                let _ = state.db().save_query(
                    ps.save_query_name.trim(),
                    &ps.sql_input,
                    None,
                );
                ps.save_query_name.clear();
                ps.last_poll = 0.0;
            }
        }
    });

    // Error display
    if let Some(err) = &ps.query_error {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("ERROR: {}", err))
            .color(RED_WATCHDOG).size(FONT_SIZE_DATA));
    }
}

// ── Query Results ────────────────────────────────────────

fn show_query_results_body(ui: &mut egui::Ui, ps: &PlanState) {
    let result = match &ps.query_result {
        Some(r) => r,
        None => return,
    };

    if result.columns.is_empty() {
        ui.label(egui::RichText::new("Query returned no columns.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            let col_count = result.columns.len();

            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("query_results_table")
                    .num_columns(col_count)
                    .spacing([8.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header row
                        for col in &result.columns {
                            ui.label(egui::RichText::new(col.to_uppercase())
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                        }
                        ui.end_row();

                        // Data rows (cap display to 500)
                        for row in result.rows.iter().take(500) {
                            for val in row {
                                let (text, color) = format_cell_value(val);
                                ui.label(egui::RichText::new(text)
                                    .color(color).size(FONT_SIZE_DATA)
                                    .family(egui::FontFamily::Monospace));
                            }
                            ui.end_row();
                        }
                    });
            });
        });

    if result.row_count > 500 {
        ui.label(egui::RichText::new(format!(
            "Showing 500 of {} rows", result.row_count
        )).color(AMBER_WARNING).size(FONT_SIZE_HUD));
    }
}

/// Format a JSON cell value for display in the results table.
fn format_cell_value(val: &serde_json::Value) -> (String, egui::Color32) {
    match val {
        serde_json::Value::Null => ("NULL".to_string(), TEXT_SECONDARY),
        serde_json::Value::Bool(b) => (b.to_string(), if *b { GREEN_COLLECT } else { RED_WATCHDOG }),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    (format!("{}", f as i64), TEXT_PRIMARY)
                } else {
                    (format!("{:.4}", f), TEXT_PRIMARY)
                }
            } else {
                (n.to_string(), TEXT_PRIMARY)
            }
        }
        serde_json::Value::String(s) => {
            // Truncate long strings
            if s.len() > 80 {
                let truncated: String = s.chars().take(77).collect();
                (format!("{}...", truncated), TEXT_PRIMARY)
            } else {
                (s.clone(), TEXT_PRIMARY)
            }
        }
        _ => (val.to_string(), TEXT_SECONDARY),
    }
}

// ── Saved Queries ────────────────────────────────────────

fn show_saved_queries_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    if ps.saved_queries.is_empty() {
        ui.label(egui::RichText::new("No saved queries. Execute a query and save it above.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("saved_queries_table")
                .num_columns(4)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["NAME", "SQL", "CREATED", ""] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let queries_snapshot = ps.saved_queries.clone();
                    for sq in &queries_snapshot {
                        // Name (clickable to load)
                        if ui.add(egui::Label::new(
                            egui::RichText::new(&sq.name)
                                .color(CYAN_P25).size(FONT_SIZE_DATA)
                        ).sense(egui::Sense::click())).clicked() {
                            ps.sql_input = sq.sql_text.clone();
                            ps.selected_prebuilt = None;
                        }

                        // SQL preview (truncated)
                        let preview: String = sq.sql_text.chars().take(60).collect();
                        let suffix = if sq.sql_text.len() > 60 { "..." } else { "" };
                        ui.label(egui::RichText::new(format!("{}{}", preview, suffix))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Created
                        let created = sq.created_at.get(..10).unwrap_or(&sq.created_at);
                        ui.label(egui::RichText::new(created)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Delete
                        if ui.add(egui::Button::new(
                            egui::RichText::new("×").color(RED_WATCHDOG).size(FONT_SIZE_DATA)
                        ).small()).clicked() {
                            let _ = state.db().delete_saved_query(sq.id);
                            ps.last_poll = 0.0;
                        }

                        ui.end_row();
                    }
                });
        });
}

// ── Schema Browser ───────────────────────────────────────

fn show_schema_browser_body(ui: &mut egui::Ui, state: &rf_web::AppState, ps: &mut PlanState) {
    // Refresh schema when section is shown
    if ps.schema.is_empty() {
        if let Ok(schema) = state.db().query_schema() {
            ps.schema = schema;
        }
    }

    // Table pills
    ui.horizontal_wrapped(|ui| {
        for (i, ts) in ps.schema.iter().enumerate() {
            let is_selected = ps.selected_schema_table == Some(i);
            let color = if is_selected { CYAN_P25 } else { TEXT_PRIMARY };
            let label = format!("{} ({})", ts.table, ts.row_count);
            if ui.selectable_label(is_selected,
                egui::RichText::new(&label).color(color).size(FONT_SIZE_DATA)
            ).clicked() {
                ps.selected_schema_table = if is_selected { None } else { Some(i) };
            }
        }
    });

    // Column detail for selected table
    if let Some(idx) = ps.selected_schema_table {
        if let Some(ts) = ps.schema.get(idx) {
            ui.add_space(4.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(4))
                .fill(BG_SURFACE)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(format!(
                        "TABLE: {}  ({} rows, {} columns)", ts.table, ts.row_count, ts.columns.len()
                    )).color(BLUE_PLAN).size(FONT_SIZE_DATA).strong());

                    ui.add_space(4.0);

                    egui::Grid::new("schema_columns_table")
                        .num_columns(4)
                        .spacing([10.0, 2.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["COLUMN", "TYPE", "NOT NULL", "PK"] {
                                ui.label(egui::RichText::new(h)
                                    .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                            }
                            ui.end_row();

                            for col in &ts.columns {
                                ui.label(egui::RichText::new(&col.name)
                                    .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                    .family(egui::FontFamily::Monospace));

                                ui.label(egui::RichText::new(&col.col_type)
                                    .color(AMBER_WARNING).size(FONT_SIZE_DATA));

                                let nn = if col.notnull { "YES" } else { "—" };
                                ui.label(egui::RichText::new(nn)
                                    .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                                let pk = if col.pk { "PK" } else { "—" };
                                let pk_color = if col.pk { GREEN_COLLECT } else { TEXT_SECONDARY };
                                ui.label(egui::RichText::new(pk)
                                    .color(pk_color).size(FONT_SIZE_DATA));

                                ui.end_row();
                            }
                        });

                    // Quick query button
                    ui.add_space(4.0);
                    if ui.add(egui::Button::new(
                        egui::RichText::new(format!("SELECT * FROM {} LIMIT 50", ts.table))
                            .color(CYAN_P25).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace)
                    )).clicked() {
                        ps.sql_input = format!("SELECT * FROM {} LIMIT 50", ts.table);
                        ps.selected_prebuilt = None;
                    }
                });
        }
    }
}
