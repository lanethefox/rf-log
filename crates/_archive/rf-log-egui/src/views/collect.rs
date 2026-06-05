use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use eframe::egui::{self, Color32};
use num_complex::Complex32;

use crate::bridge::UiBridge;
use crate::state::{CollectTab, UiState};
use crate::theme::*;
use crate::widgets::{band_tabs, signal_log, spectrum, tab_bar, waterfall};

/// Portland system name as stored in the database
const SYSTEM_NAME: &str = "Portland";

/// Per-band waterfall state, kept across frames
pub struct CollectState {
    pub waterfalls: HashMap<String, waterfall::WaterfallState>,
    /// Aggregated signals from all bands for the signal log
    pub signal_rows: Vec<signal_log::SignalRow>,
    pub selected_signal: Option<usize>,
    /// Which bands have waterfall enabled
    pub waterfall_enabled: HashMap<String, bool>,
    /// TG search text (persisted across frames)
    pub tg_search: String,
    /// Frequency input text for manual tune
    pub freq_input: String,
    /// Cached TG names: tgid → name (refreshed periodically)
    pub tg_name_cache: HashMap<u32, String>,
    /// Cached TG departments: tgid → department
    pub tg_dept_cache: HashMap<u32, String>,
    pub tg_cache_age: std::time::Instant,
    // --- Dispatch log state ---
    /// Search/filter text for dispatch log
    pub dispatch_search: String,
    /// Type filters: which event types to show
    pub dispatch_type_filters: HashSet<String>,
    /// Whether dispatch log auto-scrolls to newest
    pub dispatch_live_tail: bool,
    /// Expanded row index (for detail view)
    pub dispatch_expanded: Option<usize>,
    /// Filter by specific TG (click-to-filter)
    pub dispatch_filter_tg: Option<u32>,
    /// Filter by specific UID (click-to-filter)
    pub dispatch_filter_uid: Option<u32>,
    /// Filter: show only encrypted traffic
    pub dispatch_enc_only: bool,
    /// Filter by department name (click-to-filter)
    pub dispatch_filter_dept: Option<String>,
    /// Show absolute or relative timestamps
    pub dispatch_abs_time: bool,
    /// Previous protocol log length (for detecting new events)
    pub dispatch_prev_len: usize,
    /// Search history panel visible
    #[allow(dead_code)] // Planned for dispatch search UI
    pub dispatch_search_panel: bool,
    /// Search history entries (recent searches)
    #[allow(dead_code)] // Planned for dispatch search UI
    pub dispatch_search_history: Vec<String>,
    // --- TG Group editing state ---
    /// Text input for new group name
    pub new_group_name: String,
    /// Group currently being renamed (old name)
    pub renaming_group: Option<String>,
    /// Text input for rename
    pub rename_group_text: String,
    // --- Weather / SAME alerts ---
    pub wx_alerts: Vec<rf_db::WxAlert>,
    pub active_wx_count: i64,
    pub wx_last_poll: f64,
    // --- Recording tab state ---
    pub rec_stats: Option<rf_db::RecordingStats>,
    pub rec_history: Vec<rf_db::Recording>,
    pub rec_clips: Vec<rf_db::Recording>,
    pub rec_clip_stats: Option<rf_db::ClipStats>,
    pub rec_tg_groups: Vec<rf_db::TgGroup>,
    pub rec_freq_groups: Vec<rf_db::FreqGroup>,
    pub rec_iq_captures: Vec<rf_db::Recording>,
    pub rec_selected_clip_tgid: Option<i32>,
    pub rec_last_poll: f64,
    /// User label for manual recordings
    pub rec_label: String,
    // --- Playback state (REC-06) ---
    /// Whether audio playback is active
    pub playback_active: bool,
    /// DB id of the recording being played
    pub playback_db_id: Option<i64>,
    /// Stop signal for the playback thread
    pub playback_stop: Arc<AtomicBool>,
    // --- IQ Viewer state (REC-07/08/09) ---
    /// IQ viewer window open
    pub iq_viewer_open: bool,
    /// File path of the IQ file being viewed
    pub iq_viewer_file: Option<String>,
    /// Center frequency of the IQ capture
    pub iq_viewer_freq: f64,
    /// Sample rate of the IQ capture
    pub iq_viewer_sample_rate: f64,
    /// IQ data loaded into memory (I, Q pairs)
    pub iq_viewer_data: Option<Vec<(f32, f32)>>,
    /// Current offset (sample index) for panning
    pub iq_viewer_offset: usize,
    /// Window size (number of samples visible)
    pub iq_viewer_zoom: usize,
    /// Active tab in the IQ viewer
    pub iq_viewer_tab: IqViewerTab,
    /// Cached fingerprint result
    pub iq_viewer_fingerprint: Option<rf_dsp::fingerprint::RfFingerprint>,
    /// Fingerprint match results from DB
    pub iq_viewer_fp_matches: Vec<rf_db::RadioFingerprint>,
    /// Status message for fingerprint save
    pub iq_viewer_fp_status: Option<String>,
    // --- Auto-IQ rule management (REC-10) ---
    pub auto_iq_rules: Vec<rf_db::AutoIqRule>,
    pub auto_iq_last_poll: f64,
    pub new_rule_trigger: String,
    pub new_rule_config: String,
    pub new_rule_max_dur: i32,
    pub new_rule_creating: bool,
    // --- Storage management (REC-12) ---
    pub storage_delete_iq_armed: bool,
    pub storage_delete_clips_armed: bool,
    // --- P25 Grant cache (P25-01) ---
    pub grants: Vec<rf_db::ChannelGrant>,
    pub grants_last_poll: f64,
    // --- P25 Talkgroup cache (avoid per-frame DB query) ---
    pub cached_talkgroups: Vec<rf_db::NetworkTalkgroup>,
    pub talkgroups_last_poll: f64,
    // --- GPS / Field panel state (Phase 3) ---
    /// Fixed lat text input
    pub gps_fixed_lat_input: String,
    /// Fixed lon text input
    pub gps_fixed_lon_input: String,
    /// Selected GPS source for radio buttons
    pub gps_selected_source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IqViewerTab {
    TimeDomain,
    Spectrum,
    Fingerprint,
}

impl Default for CollectState {
    fn default() -> Self {
        // Default: only P25 voice events. TSBK (network signaling) and RDS
        // are network-level chatter — operators toggle them on when needed.
        let mut type_filters = HashSet::new();
        type_filters.insert("p25".to_string());

        Self {
            waterfalls: HashMap::new(),
            signal_rows: Vec::new(),
            selected_signal: None,
            waterfall_enabled: HashMap::new(),
            tg_search: String::new(),
            freq_input: String::new(),
            tg_name_cache: HashMap::new(),
            tg_dept_cache: HashMap::new(),
            tg_cache_age: std::time::Instant::now(),
            dispatch_search: String::new(),
            dispatch_type_filters: type_filters,
            dispatch_live_tail: true,
            dispatch_expanded: None,
            dispatch_filter_tg: None,
            dispatch_filter_uid: None,
            dispatch_enc_only: false,
            dispatch_filter_dept: None,
            dispatch_abs_time: false,
            dispatch_prev_len: 0,
            dispatch_search_panel: false,
            dispatch_search_history: Vec::new(),
            new_group_name: String::new(),
            renaming_group: None,
            rename_group_text: String::new(),
            wx_alerts: Vec::new(),
            active_wx_count: 0,
            wx_last_poll: 0.0,
            rec_stats: None,
            rec_history: Vec::new(),
            rec_clips: Vec::new(),
            rec_clip_stats: None,
            rec_tg_groups: Vec::new(),
            rec_freq_groups: Vec::new(),
            rec_iq_captures: Vec::new(),
            rec_selected_clip_tgid: None,
            rec_last_poll: 0.0,
            rec_label: String::new(),
            // Playback
            playback_active: false,
            playback_db_id: None,
            playback_stop: Arc::new(AtomicBool::new(false)),
            // IQ viewer
            iq_viewer_open: false,
            iq_viewer_file: None,
            iq_viewer_freq: 0.0,
            iq_viewer_sample_rate: 0.0,
            iq_viewer_data: None,
            iq_viewer_offset: 0,
            iq_viewer_zoom: 4096,
            iq_viewer_tab: IqViewerTab::TimeDomain,
            iq_viewer_fingerprint: None,
            iq_viewer_fp_matches: Vec::new(),
            iq_viewer_fp_status: None,
            // Auto-IQ rules (REC-10)
            auto_iq_rules: Vec::new(),
            auto_iq_last_poll: 0.0,
            new_rule_trigger: "tgid".to_string(),
            new_rule_config: String::new(),
            new_rule_max_dur: 30,
            new_rule_creating: false,
            // Storage management (REC-12)
            storage_delete_iq_armed: false,
            storage_delete_clips_armed: false,
            // P25 Grant cache (P25-01)
            grants: Vec::new(),
            grants_last_poll: 0.0,
            // P25 Talkgroup cache
            cached_talkgroups: Vec::new(),
            talkgroups_last_poll: 0.0,
            // GPS / Field panel
            gps_fixed_lat_input: String::new(),
            gps_fixed_lon_input: String::new(),
            gps_selected_source: String::new(),
        }
    }
}

/// All known band keys in display order
const ALL_BANDS: &[&str] = &["AM", "HF", "FM", "VHF", "FEDV", "BIII", "UHF", "GMRS", "P25"];

/// COLLECT view — tab bar at top, content depends on active tab
pub fn show(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    bridge: &UiBridge,
    state: &rf_web::AppState,
    collect_state: &mut CollectState,
    rec_cmd_tx: &mpsc::Sender<rf_recorder::RecorderCommand>,
    playback_tx: &mpsc::Sender<Vec<f32>>,
) {
    // Poll WX alerts (runs on every tab so status bar badge stays current)
    let now = ui.input(|i| i.time);
    if now - collect_state.wx_last_poll > 5.0 {
        collect_state.wx_last_poll = now;
        if let Ok(alerts) = state.db().list_wx_alerts(50) {
            let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            collect_state.active_wx_count = alerts.iter().filter(|a| {
                a.expires_at.as_ref().map_or(true, |exp| exp.as_str() >= now_str.as_str())
            }).count() as i64;
            collect_state.wx_alerts = alerts;
        }
    }

    ui.vertical(|ui| {
        // Tab bar (always visible at top)
        tab_bar::show(
            ui,
            CollectTab::ALL,
            &mut ui_state.collect_tab,
            |t| t.label(),
            GREEN_COLLECT,
        );
        ui.separator();

        // Route to full-page content based on active tab
        match ui_state.collect_tab {
            CollectTab::Signals => {
                update_signal_rows(bridge, collect_state);
                show_signals_view(ui, ui_state, bridge, state, collect_state);
            }
            CollectTab::P25 => {
                show_p25_panel(ui, ui_state, bridge, state, collect_state);
            }
            CollectTab::Dispatch => {
                show_dispatch_panel(ui, bridge, collect_state, state);
            }
            CollectTab::Field => {
                show_field_panel(ui, bridge, state, collect_state);
            }
            CollectTab::Rec => {
                show_rec_panel(ui, bridge, state, collect_state, rec_cmd_tx, playback_tx);
            }
        }
    });
}

/// SIGNALS tab — analog spectrum/waterfall top, signal log bottom
fn show_signals_view(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    bridge: &UiBridge,
    state: &rf_web::AppState,
    collect_state: &mut CollectState,
) {
    let available = ui.available_rect_before_wrap();
    let split_y = available.top() + available.height() * ui_state.collect_split_v;

    // --- Top area: band tabs + spectrum + waterfall ---
    let top_rect =
        egui::Rect::from_min_max(available.left_top(), egui::pos2(available.right(), split_y));
    let mut top_ui = ui.new_child(egui::UiBuilder::new().max_rect(top_rect));
    show_spectrum_area(&mut top_ui, ui_state, bridge, state, collect_state);

    // --- Divider ---
    ui.painter().line_segment(
        [
            egui::pos2(available.left(), split_y),
            egui::pos2(available.right(), split_y),
        ],
        egui::Stroke::new(1.0, BORDER),
    );

    // --- Bottom area: signal log (click-to-monitor) ---
    let bottom_rect = egui::Rect::from_min_max(
        egui::pos2(available.left(), split_y + 1.0),
        available.right_bottom(),
    );
    let mut bottom_ui = ui.new_child(egui::UiBuilder::new().max_rect(bottom_rect));
    if let Some(freq) = signal_log::show(
        &mut bottom_ui,
        &collect_state.signal_rows,
        &mut collect_state.selected_signal,
    ) {
        crate::commands::monitor_signal(state, freq);
    }

    // Consume the full rect so egui knows it's used
    ui.allocate_rect(available, egui::Sense::hover());
}

fn show_spectrum_area(
    ui: &mut egui::Ui,
    _ui_state: &mut UiState,
    bridge: &UiBridge,
    state: &rf_web::AppState,
    collect_state: &mut CollectState,
) {
    let active_bands = bridge.active_bands();

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            // Band tab bar
            if let Some(new_bands) = band_tabs::show(ui, &active_bands, ALL_BANDS) {
                crate::commands::set_bands(state, new_bands);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Monitor frequency display
                let monitor_freq = bridge.hb_f64("monitor_freq");
                let mode = bridge.hb_str("mode");
                if mode == "monitor" && monitor_freq > 0.0 {
                    ui.label(
                        egui::RichText::new(format!("{:.4} MHz", monitor_freq))
                            .size(FONT_SIZE_DATA)
                            .color(GREEN_COLLECT)
                            .family(egui::FontFamily::Monospace),
                    );
                    if ui.small_button("SCAN").on_hover_text("Return to scan mode").clicked() {
                        crate::commands::set_mode(state, "scan");
                        crate::commands::set_scanning(state, true);
                    }
                }

                // Frequency input
                let response = ui.add(
                    egui::TextEdit::singleline(&mut collect_state.freq_input)
                        .desired_width(100.0)
                        .hint_text("MHz")
                        .font(egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace)),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(freq) = collect_state.freq_input.trim().parse::<f64>() {
                        if freq > 0.1 && freq < 2000.0 {
                            crate::commands::monitor_signal(state, freq);
                        }
                    }
                }
                ui.label(
                    egui::RichText::new("TUNE:")
                        .size(FONT_SIZE_HUD)
                        .color(TEXT_SECONDARY)
                        .family(egui::FontFamily::Monospace),
                );
            });
        });

        ui.add_space(2.0);

        // Per-band spectrum + waterfall, stacked vertically
        let bands_to_show: Vec<String> = if active_bands.is_empty() {
            // Show all bands we have data for
            bridge.spectrum.keys().cloned().collect()
        } else {
            active_bands
        };

        if bands_to_show.is_empty() {
            spectrum::show_empty(ui, "ALL", ui.available_height());
            return;
        }

        let threshold = bridge.hb_f64("threshold");
        let band_count = bands_to_show.len().max(1) as f32;
        let available_height = ui.available_height();
        // Divide height among bands (spectrum + optional waterfall)
        let height_per_band = (available_height / band_count).max(80.0);

        for band in &bands_to_show {
            let wf_enabled = collect_state
                .waterfall_enabled
                .get(band.as_str())
                .copied()
                .unwrap_or(true);

            let spectrum_height = if wf_enabled {
                height_per_band * 0.6
            } else {
                height_per_band
            };

            // Band header with waterfall toggle
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(band)
                        .size(FONT_SIZE_HUD)
                        .color(GREEN_COLLECT),
                );
                let wf_label = if wf_enabled { "WF:ON" } else { "WF:OFF" };
                if ui
                    .small_button(
                        egui::RichText::new(wf_label)
                            .size(FONT_SIZE_HUD)
                            .color(TEXT_SECONDARY),
                    )
                    .clicked()
                {
                    collect_state
                        .waterfall_enabled
                        .insert(band.clone(), !wf_enabled);
                }
            });

            // Spectrum plot (click-to-tune)
            if let Some(frame) = bridge.spectrum.get(band.as_str()) {
                if let Some(freq) = spectrum::show(ui, band, frame, threshold, spectrum_height) {
                    crate::commands::monitor_signal(state, freq);
                }

                // Waterfall (click-to-tune)
                if wf_enabled {
                    let wf = collect_state
                        .waterfalls
                        .entry(band.clone())
                        .or_insert_with(|| waterfall::WaterfallState::new(128));
                    wf.push_row(&frame.powers);
                    let freq_range = frame.freqs.first().zip(frame.freqs.last())
                        .map(|(&lo, &hi)| (lo, hi));
                    if let Some(freq) = wf.show_with_freq(ui, height_per_band * 0.4, freq_range) {
                        crate::commands::monitor_signal(state, freq);
                    }
                }
            } else {
                spectrum::show_empty(ui, band, spectrum_height);
            }
        }
    });
}

// ── P25 Network Scanner Panel ───────────────────────────────

fn show_p25_panel(ui: &mut egui::Ui, ui_state: &mut UiState, bridge: &UiBridge, state: &rf_web::AppState, collect_state: &mut CollectState) {
    let ns_active = bridge.hb_bool("network_scan_active");
    let cc_freq = bridge.hb_f64("network_scan_cc_freq");
    let scan_mode = bridge.hb_str("network_scan_mode").to_string();
    let voice_slot = bridge.hb_str("network_scan_voice_slot").to_string();
    let cc_index = bridge.hb_u64("network_scan_cc_index");
    let channel_params = bridge.hb_u64("network_scan_channel_params");

    // --- CC Status bar ---
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        // CC Lock toggle
        let btn_text = if ns_active { "STOP CC" } else { "START CC" };
        let btn_color = if ns_active { RED_RECORDING } else { GREEN_COLLECT };
        if ui.button(
            egui::RichText::new(btn_text)
                .size(FONT_SIZE_DATA)
                .color(btn_color)
                .family(egui::FontFamily::Monospace),
        ).clicked() {
            if ns_active {
                state.update_config(|c| c.network_scan_active = false);
            } else {
                // Start with watched TGs from DB
                let watched: Vec<u32> = state.db()
                    .list_network_talkgroups(Some(SYSTEM_NAME), 2000)
                    .unwrap_or_default()
                    .iter()
                    .filter(|tg| tg.scan_enabled)
                    .map(|tg| tg.tgid as u32)
                    .collect();
                // Load CC list from DB
                let cc_list = state.db()
                    .get_all_cc_frequencies(SYSTEM_NAME)
                    .unwrap_or_default();
                state.update_config(|c| {
                    c.network_scan_active = true;
                    c.network_scan_tgids = watched;
                    if !cc_list.is_empty() {
                        c.network_scan_cc_list = cc_list;
                    }
                });
            }
        }

        ui.separator();

        // CC frequency dropdown
        let cc_list = state.config().network_scan_cc_list.clone();
        if !cc_list.is_empty() {
            let current_label = if cc_freq > 0.0 {
                format!("{:.5}", cc_freq)
            } else {
                "Select CC".to_string()
            };

            egui::ComboBox::from_id_salt("cc_freq_select")
                .selected_text(
                    egui::RichText::new(&current_label)
                        .size(FONT_SIZE_DATA)
                        .color(if ns_active { CYAN_P25 } else { TEXT_SECONDARY })
                        .family(egui::FontFamily::Monospace),
                )
                .show_ui(ui, |ui| {
                    for (i, &freq) in cc_list.iter().enumerate() {
                        let label = format!("{:.5} MHz {}", freq,
                            if i == cc_index as usize { "<" } else { "" });
                        if ui.selectable_label(
                            (cc_freq - freq).abs() < 0.001,
                            egui::RichText::new(label)
                                .size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace),
                        ).clicked() {
                            crate::commands::set_cc_freq(state, freq);
                        }
                    }
                });
        } else if cc_freq > 0.0 {
            ui.label(
                egui::RichText::new(format!("CC: {:.5} MHz", cc_freq))
                    .size(FONT_SIZE_DATA)
                    .color(if ns_active { CYAN_P25 } else { TEXT_SECONDARY })
                    .family(egui::FontFamily::Monospace),
            );
        }

        // Hunt status
        let hunting = ns_active && channel_params == 0;
        if ns_active {
            let status_text = if hunting { "HUNTING" } else { "LOCKED" };
            let status_color = if hunting { AMBER_WARNING } else { GREEN_COLLECT };
            ui.label(
                egui::RichText::new(status_text)
                    .size(FONT_SIZE_HUD)
                    .color(status_color)
                    .family(egui::FontFamily::Monospace),
            );
        }

        // Channel params count
        if channel_params > 0 {
            ui.label(
                egui::RichText::new(format!("PARAMS:{}", channel_params))
                    .size(FONT_SIZE_HUD)
                    .color(GREEN_COLLECT)
                    .family(egui::FontFamily::Monospace),
            );
        }

        // Voice slot
        if !voice_slot.is_empty() && voice_slot != "--" {
            ui.label(
                egui::RichText::new(format!("VOICE: {}", voice_slot))
                    .size(FONT_SIZE_HUD)
                    .color(GREEN_COLLECT)
                    .family(egui::FontFamily::Monospace),
            );
        }
    });

    ui.separator();

    // --- Scan mode selector + search ---
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("MODE:")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );

        for &(mode, label) in &[("id_scan", "WATCHED"), ("id_search", "ALL TGs")] {
            let is_active = scan_mode == mode;
            let text_color = if is_active { CYAN_P25 } else { TEXT_SECONDARY };
            if ui.selectable_label(is_active,
                egui::RichText::new(label)
                    .size(FONT_SIZE_DATA)
                    .color(text_color)
                    .family(egui::FontFamily::Monospace),
            ).clicked() {
                state.update_config(|c| c.network_scan_mode = mode.to_string());
            }
        }

        ui.add_space(12.0);

        // Department hold
        let dept_hold = bridge.hb_str("network_scan_dept_hold").to_string();
        if !dept_hold.is_empty() && dept_hold != "--" {
            ui.label(
                egui::RichText::new(format!("HOLD: {}", dept_hold))
                    .size(FONT_SIZE_HUD)
                    .color(CYAN_P25)
                    .family(egui::FontFamily::Monospace),
            );
            if ui.small_button("X").clicked() {
                state.update_config(|c| c.network_scan_dept_hold = None);
            }
        }

        ui.add_space(12.0);

        // TG Search
        ui.label(
            egui::RichText::new("SEARCH:")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
        let response = ui.add(
            egui::TextEdit::singleline(&mut collect_state.tg_search)
                .desired_width(160.0)
                .hint_text("TGID, name, dept...")
                .font(egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace)),
        );
        if !collect_state.tg_search.is_empty() {
            if ui.small_button("X").clicked() {
                collect_state.tg_search.clear();
                response.request_focus();
            }
        }
    });

    ui.separator();

    // --- Refresh TG name + dept cache every 5 seconds ---
    if collect_state.tg_cache_age.elapsed().as_secs() > 5 {
        collect_state.tg_name_cache.clear();
        collect_state.tg_dept_cache.clear();
        if let Ok(tgs) = state.db().list_network_talkgroups(Some(SYSTEM_NAME), 2000) {
            for tg in tgs {
                let name = tg.name.as_deref()
                    .or(tg.department.as_deref())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    collect_state.tg_name_cache.insert(tg.tgid as u32, name);
                }
                if let Some(dept) = &tg.department {
                    if !dept.is_empty() {
                        collect_state.tg_dept_cache.insert(tg.tgid as u32, dept.clone());
                    }
                }
            }
        }
        collect_state.tg_cache_age = std::time::Instant::now();
    }

    // --- Poll channel grants every 2 seconds (P25-01) ---
    let now = ui.input(|i| i.time);
    if now - collect_state.grants_last_poll > 2.0 {
        // Rolling 30-second window, max 50 grants
        let since = chrono::Utc::now() - chrono::Duration::seconds(30);
        let since_str = since.format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(grants) = state.db().list_channel_grants(None, None, Some(&since_str), 50) {
            collect_state.grants = grants;
        }
        collect_state.grants_last_poll = now;
    }

    // --- Live Transmission Panel ---
    show_p25_live_transmission(ui, bridge, &collect_state.tg_name_cache, &collect_state.tg_dept_cache);

    // --- Grant Activity Table (P25-02/03/04) ---
    show_grant_activity(ui, state, collect_state, now);

    ui.separator();

    // --- Department/Talkgroup Tree ---
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("TALKGROUPS")
                .size(FONT_SIZE_HEADER)
                .color(CYAN_P25),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(
                egui::RichText::new("NONE").size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
            ).clicked() {
                for tg in state.db().list_network_talkgroups(Some(SYSTEM_NAME), 5000).unwrap_or_default() {
                    if tg.scan_enabled {
                        let _ = state.db().set_talkgroup_scan_enabled(tg.tgid, &tg.system, false);
                    }
                }
                collect_state.talkgroups_last_poll = 0.0; // force refresh
            }
            if ui.small_button(
                egui::RichText::new("ALL").size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
            ).clicked() {
                for tg in state.db().list_network_talkgroups(Some(SYSTEM_NAME), 5000).unwrap_or_default() {
                    if !tg.scan_enabled {
                        let _ = state.db().set_talkgroup_scan_enabled(tg.tgid, &tg.system, true);
                    }
                }
                collect_state.talkgroups_last_poll = 0.0; // force refresh
            }
        });
    });

    // Refresh talkgroup cache every 3 seconds (avoids per-frame DB query)
    if now - collect_state.talkgroups_last_poll > 3.0 {
        collect_state.talkgroups_last_poll = now;
        if let Ok(tgs) = state.db().list_network_talkgroups(Some(SYSTEM_NAME), 2000) {
            collect_state.cached_talkgroups = tgs;
        }
    }
    let all_talkgroups = &collect_state.cached_talkgroups;
    let dept_hold = bridge.hb_str("network_scan_dept_hold").to_string();

    // Client-side search filter
    let search_lower = collect_state.tg_search.to_lowercase();
    let filtered: Vec<_> = if search_lower.is_empty() {
        all_talkgroups.iter().collect()
    } else {
        all_talkgroups.iter().filter(|tg| {
            tg.tgid.to_string().contains(&search_lower)
            || tg.name.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
            || tg.department.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
            || tg.tag.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
        }).collect()
    };

    // Group by department
    let mut dept_order: Vec<String> = Vec::new();
    let mut by_dept: HashMap<String, Vec<&rf_db::NetworkTalkgroup>> = HashMap::new();
    for tg in &filtered {
        let dept = tg.department.as_deref().unwrap_or("(Unassigned)").to_string();
        by_dept.entry(dept.clone()).or_default().push(tg);
        if !dept_order.contains(&dept) {
            dept_order.push(dept);
        }
    }

    // Build TG lookup for group display
    let tg_lookup: HashMap<i32, &rf_db::NetworkTalkgroup> = all_talkgroups.iter()
        .map(|tg| (tg.tgid as i32, tg))
        .collect();

    // Collect group names for the "add to group" dropdown
    let group_names: Vec<String> = ui_state.tg_groups.keys().cloned().collect();

    if !search_lower.is_empty() {
        ui.label(
            egui::RichText::new(format!("{} of {} talkgroups", filtered.len(), all_talkgroups.len()))
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY),
        );
    }

    // Deferred actions to apply after scroll area (avoids borrow conflicts)
    let mut deferred_group_delete: Option<String> = None;
    let mut deferred_group_rename: Option<(String, String)> = None;
    let mut deferred_group_remove_tg: Option<(String, i32)> = None;
    let mut deferred_group_add_tg: Option<(String, i32)> = None;
    let mut deferred_group_create: Option<String> = None;
    let mut deferred_group_scan: Option<(String, bool)> = None;
    let mut deferred_start_rename: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt("tg_tree_scroll")
        .show(ui, |ui| {
            // ═══ CUSTOM GROUPS SECTION ═══
            if !ui_state.tg_groups.is_empty() || !collect_state.new_group_name.is_empty() {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("GROUPS")
                            .size(FONT_SIZE_HEADER)
                            .color(MAGENTA_EXPLOIT),
                    );
                });

                // Show each custom group
                for (group_name, tg_ids) in &ui_state.tg_groups {
                    let grp_id = egui::Id::new(("tg_group", group_name));
                    let mut grp_open = ui.data(|d| d.get_temp::<bool>(grp_id).unwrap_or(false));

                    // Count how many in this group are scan-enabled
                    let watched = tg_ids.iter()
                        .filter(|id| tg_lookup.get(id).is_some_and(|tg| tg.scan_enabled))
                        .count();

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        // Expand/collapse
                        let arrow = if grp_open { "\u{25BC}" } else { "\u{25B6}" };
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(arrow)
                                    .size(FONT_SIZE_HUD)
                                    .color(TEXT_SECONDARY),
                            )
                            .fill(Color32::TRANSPARENT)
                            .frame(false)
                            .min_size(egui::vec2(14.0, 16.0)),
                        ).clicked() {
                            grp_open = !grp_open;
                        }
                        ui.data_mut(|d| d.insert_temp(grp_id, grp_open));

                        // Inline rename mode
                        if collect_state.renaming_group.as_deref() == Some(group_name) {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut collect_state.rename_group_text)
                                    .desired_width(120.0)
                                    .font(egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace)),
                            );
                            if resp.lost_focus() {
                                let new_name = collect_state.rename_group_text.trim().to_string();
                                if !new_name.is_empty() && new_name != *group_name {
                                    deferred_group_rename = Some((group_name.clone(), new_name));
                                }
                                collect_state.renaming_group = None;
                            }
                            // Auto-focus on first frame
                            if resp.gained_focus() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    collect_state.renaming_group = None;
                                }
                            }
                        } else {
                            // Group name label
                            ui.label(
                                egui::RichText::new(group_name)
                                    .size(FONT_SIZE_DATA)
                                    .color(MAGENTA_EXPLOIT)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }

                        // Count badge
                        let count_color = if watched > 0 { GREEN_COLLECT } else { TEXT_SECONDARY };
                        ui.label(
                            egui::RichText::new(format!("{}/{}", watched, tg_ids.len()))
                                .size(FONT_SIZE_HUD)
                                .color(count_color)
                                .family(egui::FontFamily::Monospace),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Delete group
                            if ui.small_button(
                                egui::RichText::new("\u{2715}").size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
                            ).on_hover_text("Delete group").clicked() {
                                deferred_group_delete = Some(group_name.clone());
                            }

                            // Rename group
                            if ui.small_button(
                                egui::RichText::new("\u{270E}").size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
                            ).on_hover_text("Rename group").clicked() {
                                deferred_start_rename = Some(group_name.clone());
                            }

                            // Enable all in group
                            if ui.small_button(
                                egui::RichText::new("ON").size(FONT_SIZE_HUD).color(GREEN_COLLECT),
                            ).on_hover_text("Enable scan for all TGs in group").clicked() {
                                deferred_group_scan = Some((group_name.clone(), true));
                            }

                            // Disable all in group
                            if ui.small_button(
                                egui::RichText::new("OFF").size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
                            ).on_hover_text("Disable scan for all TGs in group").clicked() {
                                deferred_group_scan = Some((group_name.clone(), false));
                            }
                        });
                    });

                    // Expanded: show TGs in group
                    if grp_open {
                        for &tgid in tg_ids {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.add_space(18.0);

                                // Scan checkbox
                                if let Some(tg) = tg_lookup.get(&tgid) {
                                    let mut enabled = tg.scan_enabled;
                                    if ui.checkbox(&mut enabled, "").changed() {
                                        let _ = state.db().set_talkgroup_scan_enabled(
                                            tg.tgid, &tg.system, enabled,
                                        );
                                        collect_state.talkgroups_last_poll = 0.0;
                                    }

                                    ui.label(
                                        egui::RichText::new(format!("{}", tgid))
                                            .size(FONT_SIZE_DATA)
                                            .color(TEXT_PRIMARY)
                                            .family(egui::FontFamily::Monospace),
                                    );

                                    let name = tg.name.as_deref().unwrap_or("--");
                                    ui.label(
                                        egui::RichText::new(name)
                                            .size(FONT_SIZE_HUD)
                                            .color(TEXT_PRIMARY)
                                            .family(egui::FontFamily::Monospace),
                                    );

                                    if let Some(dept) = &tg.department {
                                        if !dept.is_empty() {
                                            ui.label(
                                                egui::RichText::new(dept)
                                                    .size(FONT_SIZE_HUD)
                                                    .color(TEXT_SECONDARY)
                                                    .family(egui::FontFamily::Monospace),
                                            );
                                        }
                                    }
                                } else {
                                    // TG not in DB yet
                                    ui.label(
                                        egui::RichText::new(format!("{} (unknown)", tgid))
                                            .size(FONT_SIZE_DATA)
                                            .color(TEXT_SECONDARY)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                }

                                // Remove from group
                                if ui.small_button(
                                    egui::RichText::new("\u{2715}").size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
                                ).on_hover_text("Remove from group").clicked() {
                                    deferred_group_remove_tg = Some((group_name.clone(), tgid));
                                }
                            });
                        }

                        if tg_ids.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                ui.label(
                                    egui::RichText::new("Empty — add TGs from departments below")
                                        .size(FONT_SIZE_HUD)
                                        .color(TEXT_SECONDARY),
                                );
                            });
                        }
                    }
                }

                ui.add_space(2.0);
                ui.separator();
            }

            // ═══ NEW GROUP CREATION ═══
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("+")
                        .size(FONT_SIZE_DATA)
                        .color(MAGENTA_EXPLOIT),
                );
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut collect_state.new_group_name)
                        .desired_width(140.0)
                        .hint_text("New group name...")
                        .font(egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let name = collect_state.new_group_name.trim().to_string();
                    if !name.is_empty() && !ui_state.tg_groups.contains_key(&name) {
                        deferred_group_create = Some(name);
                    }
                    collect_state.new_group_name.clear();
                }
                if ui.small_button(
                    egui::RichText::new("CREATE").size(FONT_SIZE_HUD).color(MAGENTA_EXPLOIT),
                ).clicked() {
                    let name = collect_state.new_group_name.trim().to_string();
                    if !name.is_empty() && !ui_state.tg_groups.contains_key(&name) {
                        deferred_group_create = Some(name);
                    }
                    collect_state.new_group_name.clear();
                }
            });

            ui.add_space(4.0);

            // ═══ DEPARTMENTS SECTION ═══
            if filtered.is_empty() {
                ui.label(
                    egui::RichText::new(if all_talkgroups.is_empty() {
                        "No talkgroups — start CC scan to discover"
                    } else {
                        "No talkgroups match search"
                    })
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY),
                );
                return;
            }

            for dept_name in &dept_order {
                let tgs = &by_dept[dept_name];
                let watched = tgs.iter().filter(|t| t.scan_enabled).count();
                let total = tgs.len();
                let is_held = dept_hold == *dept_name;

                // Department checkbox state: all, some, none
                let all_watched = watched == total;
                let some_watched = watched > 0 && !all_watched;

                // Department header line
                let dept_id = egui::Id::new(("dept_tree", dept_name));
                let mut dept_open = ui.data(|d| d.get_temp::<bool>(dept_id).unwrap_or(false));

                // If searching, auto-expand all departments
                if !search_lower.is_empty() {
                    dept_open = true;
                }

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Expand/collapse toggle
                    let arrow = if dept_open { "\u{25BC}" } else { "\u{25B6}" }; // ▼ / ▶
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(arrow)
                                .size(FONT_SIZE_HUD)
                                .color(TEXT_SECONDARY),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false)
                        .min_size(egui::vec2(14.0, 16.0)),
                    ).clicked() {
                        dept_open = !dept_open;
                    }
                    ui.data_mut(|d| d.insert_temp(dept_id, dept_open));

                    // Department checkbox (tri-state visual)
                    let mut check_state = all_watched;
                    let checkbox_resp = ui.checkbox(&mut check_state, "");
                    // Draw dash for partial state
                    if some_watched && !all_watched {
                        let rect = checkbox_resp.rect;
                        let center = rect.center();
                        ui.painter().line_segment(
                            [
                                egui::pos2(center.x - 4.0, center.y),
                                egui::pos2(center.x + 4.0, center.y),
                            ],
                            egui::Stroke::new(2.0, CYAN_P25),
                        );
                    }
                    if checkbox_resp.changed() {
                        // Toggle: if any watched → disable all, else enable all
                        let enable = watched == 0;
                        let _ = state.db().set_department_scan_enabled(
                            dept_name, SYSTEM_NAME, enable,
                        );
                    }

                    // Department name (click to hold)
                    let dept_color = if is_held { CYAN_P25 } else { TEXT_PRIMARY };
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(dept_name)
                                .size(FONT_SIZE_DATA)
                                .color(dept_color)
                                .family(egui::FontFamily::Monospace),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false),
                    ).clicked() {
                        if is_held {
                            state.update_config(|c| c.network_scan_dept_hold = None);
                        } else {
                            state.update_config(|c| {
                                c.network_scan_dept_hold = Some(dept_name.clone());
                            });
                        }
                    }

                    // Count badge
                    let count_text = format!("{}/{}", watched, total);
                    let count_color = if watched > 0 { GREEN_COLLECT } else { TEXT_SECONDARY };
                    ui.label(
                        egui::RichText::new(count_text)
                            .size(FONT_SIZE_HUD)
                            .color(count_color)
                            .family(egui::FontFamily::Monospace),
                    );
                });

                // Expanded: show individual talkgroups
                if dept_open {
                    for tg in tgs {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.add_space(18.0); // indent under department

                            // Scan enabled checkbox
                            let mut enabled = tg.scan_enabled;
                            if ui.checkbox(&mut enabled, "").changed() {
                                let _ = state.db().set_talkgroup_scan_enabled(
                                    tg.tgid, &tg.system, enabled,
                                );
                                collect_state.talkgroups_last_poll = 0.0;
                            }

                            // TGID
                            ui.label(
                                egui::RichText::new(format!("{}", tg.tgid))
                                    .size(FONT_SIZE_DATA)
                                    .color(TEXT_PRIMARY)
                                    .family(egui::FontFamily::Monospace),
                            );

                            // Name
                            let name = tg.name.as_deref().unwrap_or("--");
                            ui.label(
                                egui::RichText::new(name)
                                    .size(FONT_SIZE_HUD)
                                    .color(TEXT_PRIMARY)
                                    .family(egui::FontFamily::Monospace),
                            );

                            // Encryption indicator
                            let (enc_label, enc_color) = match tg.encrypted.as_str() {
                                "always" => ("ENC", RED_RECORDING),
                                "mixed" => ("MIX", AMBER_WARNING),
                                _ => ("", Color32::TRANSPARENT),
                            };
                            if !enc_label.is_empty() {
                                ui.label(
                                    egui::RichText::new(enc_label)
                                        .size(FONT_SIZE_HUD)
                                        .color(enc_color)
                                        .family(egui::FontFamily::Monospace),
                                );
                            }

                            // Add to group dropdown (only if groups exist)
                            if !group_names.is_empty() {
                                let tgid_i32 = tg.tgid as i32;
                                // Check which groups already contain this TG
                                let in_groups: Vec<&str> = group_names.iter()
                                    .filter(|g| ui_state.tg_groups.get(*g)
                                        .is_some_and(|ids| ids.contains(&tgid_i32)))
                                    .map(|g| g.as_str())
                                    .collect();

                                let btn_color = if in_groups.is_empty() { TEXT_SECONDARY } else { MAGENTA_EXPLOIT };
                                let btn_label = if in_groups.is_empty() {
                                    "+G".to_string()
                                } else {
                                    format!("G:{}", in_groups.len())
                                };

                                egui::ComboBox::from_id_salt(format!("tg_grp_{}", tg.tgid))
                                    .selected_text(
                                        egui::RichText::new(&btn_label)
                                            .size(FONT_SIZE_HUD)
                                            .color(btn_color),
                                    )
                                    .width(100.0)
                                    .show_ui(ui, |ui| {
                                        for gn in &group_names {
                                            let is_member = ui_state.tg_groups.get(gn)
                                                .is_some_and(|ids| ids.contains(&tgid_i32));
                                            let label = if is_member {
                                                format!("\u{2713} {}", gn)
                                            } else {
                                                gn.clone()
                                            };
                                            if ui.selectable_label(is_member,
                                                egui::RichText::new(&label)
                                                    .size(FONT_SIZE_DATA)
                                                    .family(egui::FontFamily::Monospace),
                                            ).clicked() {
                                                if is_member {
                                                    deferred_group_remove_tg = Some((gn.clone(), tgid_i32));
                                                } else {
                                                    deferred_group_add_tg = Some((gn.clone(), tgid_i32));
                                                }
                                            }
                                        }
                                    });
                            }

                            // Grants (right-aligned)
                            if tg.total_grants > 0 {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{}", tg.total_grants))
                                            .size(FONT_SIZE_HUD)
                                            .color(TEXT_SECONDARY)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                });
                            }
                        });
                    }
                }
            }
        });

    // Apply deferred group actions
    if let Some(name) = deferred_group_create {
        ui_state.tg_groups.insert(name, Vec::new());
    }
    if let Some(name) = deferred_group_delete {
        ui_state.tg_groups.remove(&name);
    }
    if let Some((old_name, new_name)) = deferred_group_rename {
        if let Some(tgs) = ui_state.tg_groups.remove(&old_name) {
            ui_state.tg_groups.insert(new_name, tgs);
        }
    }
    if let Some((group, tgid)) = deferred_group_add_tg {
        if let Some(ids) = ui_state.tg_groups.get_mut(&group) {
            if !ids.contains(&tgid) {
                ids.push(tgid);
            }
        }
    }
    if let Some((group, tgid)) = deferred_group_remove_tg {
        if let Some(ids) = ui_state.tg_groups.get_mut(&group) {
            ids.retain(|&id| id != tgid);
        }
    }
    if let Some((group, enable)) = deferred_group_scan {
        if let Some(ids) = ui_state.tg_groups.get(&group) {
            for &tgid in ids {
                let _ = state.db().set_talkgroup_scan_enabled(
                    tgid, SYSTEM_NAME, enable,
                );
            }
            collect_state.talkgroups_last_poll = 0.0;
        }
    }
    if let Some(name) = deferred_start_rename {
        collect_state.renaming_group = Some(name.clone());
        collect_state.rename_group_text = name;
    }
}

// ── Live P25 Transmission Panel ─────────────────────────────

fn show_p25_live_transmission(ui: &mut egui::Ui, bridge: &UiBridge, tg_names: &HashMap<u32, String>, tg_depts: &HashMap<u32, String>) {
    // Voice slot from heartbeat
    let vs_tgid = bridge.hb_nested("network_scan_voice_slot", "current_tgid")
        .and_then(|v| v.as_u64()).map(|v| v as u32);
    let vs_uid = bridge.hb_nested("network_scan_voice_slot", "current_uid")
        .and_then(|v| v.as_u64()).map(|v| v as u32);
    let vs_freq = bridge.hb_nested("network_scan_voice_slot", "current_freq")
        .and_then(|v| v.as_f64());
    let vs_active = bridge.hb_nested("network_scan_voice_slot", "active")
        .and_then(|v| v.as_bool()).unwrap_or(false);

    // Latest CQPSK status
    let latest_cqpsk = bridge.protocol_log.iter()
        .find(|e| e.event_type == "cqpsk_status");
    let cqpsk_locked = latest_cqpsk
        .and_then(|e| e.raw.get("locked"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // --- NOW TALKING bar ---
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        // Active/idle indicator
        if vs_active && vs_tgid.is_some() {
            let t = ui.ctx().input(|i| i.time);
            let pulse = ((t * 3.0).sin() * 0.5 + 0.5) as f32;
            let green = Color32::from_rgba_unmultiplied(
                0, (200.0 + 55.0 * pulse) as u8, 0, 255,
            );
            ui.label(egui::RichText::new("TX").size(FONT_SIZE_DATA).color(green)
                .family(egui::FontFamily::Monospace));
            ui.ctx().request_repaint();

            // TG + name lookup
            if let Some(tgid) = vs_tgid {
                let tg_name = tg_names.get(&tgid).cloned();

                ui.label(egui::RichText::new(format!("TG:{}", tgid))
                    .size(14.0).color(CYAN_P25).family(egui::FontFamily::Monospace));

                if let Some(name) = &tg_name {
                    ui.label(egui::RichText::new(name)
                        .size(14.0).color(TEXT_PRIMARY).family(egui::FontFamily::Monospace));
                }
            }

            // Source UID
            if let Some(uid) = vs_uid {
                ui.label(egui::RichText::new(format!("UID:{}", uid))
                    .size(FONT_SIZE_DATA).color(TEXT_PRIMARY).family(egui::FontFamily::Monospace));
            }

            // Voice freq
            if let Some(freq) = vs_freq {
                ui.label(egui::RichText::new(format!("{:.4}MHz", freq))
                    .size(FONT_SIZE_HUD).color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
            }

            // Encryption from latest P25 event
            if let Some(evt) = bridge.protocol_log.iter().find(|e| e.event_type == "p25") {
                let enc = evt.encrypted.unwrap_or(false);
                if enc {
                    let algo = evt.raw.get("algorithm").and_then(|v| v.as_str()).unwrap_or("ENC");
                    let key_id = evt.raw.get("key_id").and_then(|v| v.as_u64());
                    let label = if let Some(k) = key_id { format!("{} K:{}", algo, k) } else { algo.to_string() };
                    ui.label(egui::RichText::new(label)
                        .size(FONT_SIZE_HUD).color(RED_RECORDING).family(egui::FontFamily::Monospace));
                } else {
                    ui.label(egui::RichText::new("CLR")
                        .size(FONT_SIZE_HUD).color(GREEN_COLLECT).family(egui::FontFamily::Monospace));
                }
            }
        } else {
            ui.label(egui::RichText::new("IDLE").size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace));
        }

        // Right side: CQPSK status
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(evt) = latest_cqpsk {
                if let Some(tsbks) = evt.raw.get("tsbks_decoded").and_then(|v| v.as_u64()) {
                    ui.label(egui::RichText::new(format!("TSBK:{}", tsbks))
                        .size(FONT_SIZE_HUD)
                        .color(if tsbks > 0 { GREEN_COLLECT } else { TEXT_SECONDARY })
                        .family(egui::FontFamily::Monospace));
                }
                if let Some(offset) = evt.raw.get("freq_offset_hz").and_then(|v| v.as_f64()) {
                    ui.label(egui::RichText::new(format!("{:+.0}Hz", offset))
                        .size(FONT_SIZE_HUD).color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
                }
            }
            let lock_color = if cqpsk_locked { GREEN_COLLECT } else { AMBER_WARNING };
            let lock_text = if cqpsk_locked { "LOCK" } else { "NOLOCK" };
            ui.label(egui::RichText::new(lock_text)
                .size(FONT_SIZE_HUD).color(lock_color).family(egui::FontFamily::Monospace));
        });
    });

    // --- Recent call log (voice grants only — TX activity) ---
    let voice_grants: Vec<_> = bridge.protocol_log.iter()
        .filter(|e| {
            (e.event_type == "tsbk" && e.opcode.as_deref() == Some("GroupVoiceGrant"))
            || (e.event_type == "tsbk" && e.opcode.as_deref() == Some("UnitVoiceGrant"))
        })
        .take(30)
        .collect();

    if !voice_grants.is_empty() {
        ui.add_space(2.0);
        // Column headers
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let hdr = |ui: &mut egui::Ui, text: &str, w: f32| {
                ui.allocate_ui(egui::vec2(w, 12.0), |ui| {
                    ui.label(egui::RichText::new(text).size(FONT_SIZE_HUD)
                        .color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
                });
            };
            hdr(ui, "TIME", 68.0);
            hdr(ui, "", 4.0); // emergency spacer
            hdr(ui, "TG", 50.0);
            hdr(ui, "NAME", 130.0);
            hdr(ui, "DEPT", 140.0);
            hdr(ui, "UID", 60.0);
            hdr(ui, "", 30.0); // ENC
        });

        let max_height = ui.available_height().min(180.0);
        egui::ScrollArea::vertical()
            .id_salt("call_log")
            .max_height(max_height)
            .show(ui, |ui| {
                for grant in &voice_grants {
                    let tgid = grant.talkgroup.unwrap_or(0);
                    let uid = grant.source_unit.unwrap_or(0);
                    let enc = grant.raw.get("payload")
                        .and_then(|p| p.get("encrypted"))
                        .and_then(|e| e.as_bool())
                        .unwrap_or(false);
                    let emergency = grant.raw.get("payload")
                        .and_then(|p| p.get("emergency"))
                        .and_then(|e| e.as_bool())
                        .unwrap_or(false);
                    let timestamp = format_timestamp(grant.timestamp, true);

                    let tg_name = tg_names.get(&tgid).cloned().unwrap_or_default();
                    let tg_dept = tg_depts.get(&tgid).cloned().unwrap_or_default();

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        // Timestamp
                        ui.allocate_ui(egui::vec2(68.0, 14.0), |ui| {
                            ui.label(egui::RichText::new(&timestamp)
                                .size(FONT_SIZE_HUD).color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
                        });

                        // Emergency indicator
                        if emergency {
                            ui.label(egui::RichText::new("!")
                                .size(FONT_SIZE_DATA).color(RED_RECORDING).family(egui::FontFamily::Monospace));
                        } else {
                            ui.allocate_exact_size(egui::vec2(4.0, 14.0), egui::Sense::hover());
                        }

                        // TG number
                        ui.allocate_ui(egui::vec2(50.0, 14.0), |ui| {
                            ui.label(egui::RichText::new(format!("{}", tgid))
                                .size(FONT_SIZE_DATA).color(CYAN_P25).family(egui::FontFamily::Monospace));
                        });

                        // TG name
                        ui.allocate_ui(egui::vec2(130.0, 14.0), |ui| {
                            ui.label(egui::RichText::new(&tg_name)
                                .size(FONT_SIZE_HUD).color(TEXT_PRIMARY).family(egui::FontFamily::Monospace));
                        });

                        // Department
                        ui.allocate_ui(egui::vec2(140.0, 14.0), |ui| {
                            ui.label(egui::RichText::new(&tg_dept)
                                .size(FONT_SIZE_HUD).color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
                        });

                        // UID
                        ui.allocate_ui(egui::vec2(60.0, 14.0), |ui| {
                            ui.label(egui::RichText::new(format!("{}", uid))
                                .size(FONT_SIZE_HUD).color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
                        });

                        // Encryption
                        let (enc_text, enc_color) = if enc { ("ENC", RED_RECORDING) } else { ("CLR", GREEN_COLLECT) };
                        ui.label(egui::RichText::new(enc_text)
                            .size(FONT_SIZE_HUD).color(enc_color).family(egui::FontFamily::Monospace));
                    });
                }
            });
    }
}

// ── Grant Activity Table (P25-02/03/04) ─────────────────────

fn show_grant_activity(
    ui: &mut egui::Ui,
    state: &rf_web::AppState,
    collect_state: &CollectState,
    _now: f64,
) {
    if collect_state.grants.is_empty() {
        return;
    }

    ui.separator();

    // Estimate grants/hour from the 30s window
    let grant_count = collect_state.grants.len();
    let grants_per_hr = grant_count as f64 * 120.0; // 30s window × 120 = 1hr

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("GRANT ACTIVITY")
                .size(FONT_SIZE_HEADER)
                .color(CYAN_P25),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{:.0}/hr", grants_per_hr))
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
        });
    });

    // Column headers
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let hdr = |ui: &mut egui::Ui, text: &str, w: f32| {
            ui.allocate_ui(egui::vec2(w, 12.0), |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .size(FONT_SIZE_HUD)
                        .color(TEXT_SECONDARY)
                        .family(egui::FontFamily::Monospace),
                );
            });
        };
        hdr(ui, "TIME", 50.0);
        hdr(ui, "TG", 50.0);
        hdr(ui, "NAME", 130.0);
        hdr(ui, "UID", 60.0);
        hdr(ui, "FREQ", 76.0);
        hdr(ui, "ENC", 30.0);
        hdr(ui, "TYPE", 36.0);
    });

    // Parse current UTC time for relative age calculation
    let utc_now = chrono::Utc::now();

    let max_height = ui.available_height().min(160.0).max(60.0);
    let mut clicked_freq: Option<f64> = None;

    egui::ScrollArea::vertical()
        .id_salt("grant_activity")
        .max_height(max_height)
        .show(ui, |ui| {
            for grant in &collect_state.grants {
                // Parse grant timestamp to compute age
                let age_secs = chrono::NaiveDateTime::parse_from_str(&grant.timestamp, "%Y-%m-%d %H:%M:%S")
                    .map(|ts| {
                        let grant_utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ts, chrono::Utc);
                        (utc_now - grant_utc).num_seconds().max(0)
                    })
                    .unwrap_or(999);

                let is_active = age_secs < 5;

                // TG name/dept enrichment (P25-03)
                let tgid = grant.tgid as u32;
                let tg_name = collect_state.tg_name_cache.get(&tgid).cloned().unwrap_or_default();

                // Encryption: grant_type contains "encrypted" for encrypted grants
                let is_encrypted = grant.grant_type.as_deref()
                    .is_some_and(|t| t.contains("encrypt"));

                // Row color: active=bright, old=dim
                let row_alpha = if is_active { 255u8 } else if age_secs < 15 { 200 } else { 140 };

                // Left border indicator for active grants
                let row_response = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Active indicator dot
                    if is_active {
                        let t = ui.ctx().input(|i| i.time);
                        let pulse = ((t * 3.0).sin() * 0.5 + 0.5) as f32;
                        let g = (180.0 + 75.0 * pulse) as u8;
                        ui.allocate_ui(egui::vec2(50.0, 14.0), |ui| {
                            ui.label(
                                egui::RichText::new(format!("{}{:>2}s", '\u{25CF}', age_secs))
                                    .size(FONT_SIZE_HUD)
                                    .color(Color32::from_rgba_unmultiplied(0, g, 0, 255))
                                    .family(egui::FontFamily::Monospace),
                            );
                        });
                        ui.ctx().request_repaint();
                    } else {
                        ui.allocate_ui(egui::vec2(50.0, 14.0), |ui| {
                            ui.label(
                                egui::RichText::new(format!("{:>3}s", age_secs))
                                    .size(FONT_SIZE_HUD)
                                    .color(Color32::from_rgba_unmultiplied(90, 106, 122, row_alpha))
                                    .family(egui::FontFamily::Monospace),
                            );
                        });
                    }

                    // TG ID
                    ui.allocate_ui(egui::vec2(50.0, 14.0), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{}", grant.tgid))
                                .size(FONT_SIZE_DATA)
                                .color(Color32::from_rgba_unmultiplied(0, 204, 255, row_alpha))
                                .family(egui::FontFamily::Monospace),
                        );
                    });

                    // TG Name (P25-03 enrichment)
                    ui.allocate_ui(egui::vec2(130.0, 14.0), |ui| {
                        ui.label(
                            egui::RichText::new(&tg_name)
                                .size(FONT_SIZE_HUD)
                                .color(Color32::from_rgba_unmultiplied(224, 232, 240, row_alpha))
                                .family(egui::FontFamily::Monospace),
                        );
                    });

                    // UID
                    let uid_str = grant.uid.map(|u| format!("{}", u)).unwrap_or_default();
                    ui.allocate_ui(egui::vec2(60.0, 14.0), |ui| {
                        ui.label(
                            egui::RichText::new(&uid_str)
                                .size(FONT_SIZE_HUD)
                                .color(Color32::from_rgba_unmultiplied(224, 232, 240, row_alpha))
                                .family(egui::FontFamily::Monospace),
                        );
                    });

                    // Frequency
                    let freq_str = grant.voice_freq
                        .map(|f| format!("{:.4}", f))
                        .unwrap_or_default();
                    ui.allocate_ui(egui::vec2(76.0, 14.0), |ui| {
                        ui.label(
                            egui::RichText::new(&freq_str)
                                .size(FONT_SIZE_HUD)
                                .color(Color32::from_rgba_unmultiplied(224, 232, 240, row_alpha))
                                .family(egui::FontFamily::Monospace),
                        );
                    });

                    // Encryption badge
                    let (enc_text, enc_color) = if is_encrypted {
                        ("ENC", Color32::from_rgba_unmultiplied(255, 68, 68, row_alpha))
                    } else {
                        ("CLR", Color32::from_rgba_unmultiplied(0, 255, 102, row_alpha))
                    };
                    ui.allocate_ui(egui::vec2(30.0, 14.0), |ui| {
                        ui.label(
                            egui::RichText::new(enc_text)
                                .size(FONT_SIZE_HUD)
                                .color(enc_color)
                                .family(egui::FontFamily::Monospace),
                        );
                    });

                    // Grant type (group/unit)
                    let type_str = match grant.grant_type.as_deref() {
                        Some(t) if t.contains("unit") => "UNT",
                        _ => "GRP",
                    };
                    ui.label(
                        egui::RichText::new(type_str)
                            .size(FONT_SIZE_HUD)
                            .color(Color32::from_rgba_unmultiplied(90, 106, 122, row_alpha))
                            .family(egui::FontFamily::Monospace),
                    );
                });

                // P25-04: Click-to-monitor
                if row_response.response.interact(egui::Sense::click()).clicked() {
                    if let Some(freq) = grant.voice_freq {
                        clicked_freq = Some(freq);
                    }
                }
            }
        });

    // Apply click-to-monitor outside the scroll area (P25-04)
    if let Some(freq) = clicked_freq {
        crate::commands::monitor_signal(state, freq);
    }
}

// ── Dispatch Log Panel (SigNoz-style) ───────────────────────

/// Event type badge color
fn event_type_color(event_type: &str) -> Color32 {
    match event_type {
        "p25" => CYAN_P25,
        "tsbk" => Color32::from_rgb(100, 140, 200),
        "rds" => MAGENTA_EXPLOIT,
        "cqpsk_status" => Color32::from_rgb(80, 80, 100),
        _ => TEXT_SECONDARY,
    }
}

/// Event type display label
fn event_type_label(event_type: &str) -> &str {
    match event_type {
        "p25" => "P25",
        "tsbk" => "TSBK",
        "rds" => "RDS",
        "cqpsk_status" => "CQPSK",
        _ => event_type,
    }
}

/// Format timestamp: relative or absolute
/// Format timestamp in UTC or local time
fn format_timestamp(ts: f64, utc: bool) -> String {
    if ts <= 0.0 {
        return "--:--:--".to_string();
    }
    use chrono::{TimeZone, Utc, Local};
    let secs = ts as i64;
    let nanos = ((ts - secs as f64) * 1_000_000_000.0) as u32;
    if utc {
        if let Some(dt) = Utc.timestamp_opt(secs, nanos).single() {
            dt.format("%H:%M:%S%.3f").to_string()
        } else {
            "--:--:--".to_string()
        }
    } else {
        if let Some(dt) = Local.timestamp_opt(secs, nanos).single() {
            dt.format("%H:%M:%S%.3f").to_string()
        } else {
            "--:--:--".to_string()
        }
    }
}

/// Build a summary string for an event (used in log row)
fn event_summary(evt: &crate::bridge::ProtocolEvent, tg_names: &HashMap<u32, String>) -> String {
    match evt.event_type.as_str() {
        "p25" => {
            let tg = evt.talkgroup.map(|t| t.to_string()).unwrap_or_else(|| "--".to_string());
            let tg_name = evt.talkgroup.and_then(|t| tg_names.get(&t)).map(|s| s.as_str()).unwrap_or("");
            let uid = evt.source_unit.map(|u| format!("UID:{u}")).unwrap_or_default();
            let enc = if evt.encrypted.unwrap_or(false) { " ENC" } else { "" };
            if tg_name.is_empty() {
                format!("TG:{tg} {uid}{enc}")
            } else {
                format!("TG:{tg} ({tg_name}) {uid}{enc}")
            }
        }
        "tsbk" => {
            let opcode = evt.opcode.as_deref().unwrap_or("??");
            let tg = evt.talkgroup.map(|t| format!(" TG:{t}")).unwrap_or_default();
            let uid = evt.source_unit.map(|u| format!(" UID:{u}")).unwrap_or_default();
            format!("{opcode}{tg}{uid}")
        }
        "rds" => {
            let ps = evt.rds_ps.as_deref().unwrap_or("--");
            let pi = evt.rds_pi.map(|p| format!(" PI:{p:04X}")).unwrap_or_default();
            format!("{ps}{pi}")
        }
        _ => format!("{}", evt.event_type),
    }
}

fn show_dispatch_panel(ui: &mut egui::Ui, bridge: &UiBridge, cs: &mut CollectState, _state: &rf_web::AppState) {
    // WX alert banner (if active)
    show_wx_alert_banner(ui, cs);

    // --- Compact toolbar (single row) ---
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Live tail toggle
        let tail_color = if cs.dispatch_live_tail { GREEN_COLLECT } else { AMBER_WARNING };
        if ui.add(
            egui::Button::new(
                egui::RichText::new(if cs.dispatch_live_tail { "LIVE" } else { "PAUSED" })
                    .size(FONT_SIZE_HUD)
                    .color(tail_color)
                    .family(egui::FontFamily::Monospace),
            )
            .fill(BG_ELEVATED)
            .stroke(egui::Stroke::new(1.0, tail_color)),
        ).clicked() {
            cs.dispatch_live_tail = !cs.dispatch_live_tail;
        }

        // Type filter pills (compact)
        for evt_type in &["p25", "tsbk", "rds"] {
            let active = cs.dispatch_type_filters.contains(*evt_type);
            let color = event_type_color(evt_type);
            if ui.add(
                egui::Button::new(
                    egui::RichText::new(event_type_label(evt_type))
                        .size(8.0)
                        .color(if active { color } else { TEXT_SECONDARY })
                        .family(egui::FontFamily::Monospace),
                )
                .fill(if active { BG_ELEVATED } else { BG_SURFACE })
                .stroke(egui::Stroke::new(1.0, if active { color } else { BORDER })),
            ).clicked() {
                if active { cs.dispatch_type_filters.remove(*evt_type); }
                else { cs.dispatch_type_filters.insert(evt_type.to_string()); }
            }
        }

        // ENC filter
        let enc_color = if cs.dispatch_enc_only { RED_RECORDING } else { TEXT_SECONDARY };
        if ui.add(
            egui::Button::new(
                egui::RichText::new("ENC").size(8.0).color(enc_color).family(egui::FontFamily::Monospace),
            )
            .fill(if cs.dispatch_enc_only { BG_ELEVATED } else { BG_SURFACE })
            .stroke(egui::Stroke::new(1.0, if cs.dispatch_enc_only { RED_RECORDING } else { BORDER })),
        ).clicked() {
            cs.dispatch_enc_only = !cs.dispatch_enc_only;
        }

        // Search
        let _search_resp = ui.add(
            egui::TextEdit::singleline(&mut cs.dispatch_search)
                .desired_width(120.0)
                .hint_text("search...")
                .font(egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace)),
        );
        if !cs.dispatch_search.is_empty() {
            if ui.small_button("\u{2715}").clicked() { cs.dispatch_search.clear(); }
        }

        // Active facet badges (inline)
        if let Some(tg) = cs.dispatch_filter_tg {
            let lbl = cs.tg_name_cache.get(&tg)
                .map(|n| format!("TG:{tg}({n})\u{2715}"))
                .unwrap_or_else(|| format!("TG:{tg}\u{2715}"));
            if ui.add(egui::Button::new(
                egui::RichText::new(&lbl).size(8.0).color(CYAN_P25),
            ).fill(BG_ELEVATED)).clicked() {
                cs.dispatch_filter_tg = None;
            }
        }
        if let Some(uid) = cs.dispatch_filter_uid {
            if ui.add(egui::Button::new(
                egui::RichText::new(format!("UID:{uid}\u{2715}")).size(8.0).color(Color32::from_rgb(180, 140, 60)),
            ).fill(BG_ELEVATED)).clicked() {
                cs.dispatch_filter_uid = None;
            }
        }
        if let Some(ref dept) = cs.dispatch_filter_dept.clone() {
            if ui.add(egui::Button::new(
                egui::RichText::new(format!("{dept}\u{2715}")).size(8.0).color(GREEN_COLLECT),
            ).fill(BG_ELEVATED)).clicked() {
                cs.dispatch_filter_dept = None;
            }
        }

        // Right: event count + time toggle
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{}", bridge.protocol_log.len()))
                .size(FONT_SIZE_HUD).color(TEXT_SECONDARY).family(egui::FontFamily::Monospace));
            let time_label = if cs.dispatch_abs_time { "UTC" } else { "LOCAL" };
            if ui.small_button(
                egui::RichText::new(time_label).size(FONT_SIZE_HUD).color(TEXT_SECONDARY),
            ).clicked() {
                cs.dispatch_abs_time = !cs.dispatch_abs_time;
            }
        });
    });

    // --- Filter events ---
    let search_lower = cs.dispatch_search.to_lowercase();
    let dept_filter = cs.dispatch_filter_dept.clone();
    let filtered: Vec<(usize, &crate::bridge::ProtocolEvent)> = bridge.protocol_log.iter()
        .enumerate()
        .filter(|(_, evt)| {
            if !cs.dispatch_type_filters.contains(&evt.event_type) { return false; }
            if let Some(filter_tg) = cs.dispatch_filter_tg {
                if evt.talkgroup != Some(filter_tg) { return false; }
            }
            if let Some(filter_uid) = cs.dispatch_filter_uid {
                if evt.source_unit != Some(filter_uid) { return false; }
            }
            if cs.dispatch_enc_only && !evt.encrypted.unwrap_or(false) { return false; }
            if let Some(ref dept) = dept_filter {
                if let Some(tg) = evt.talkgroup {
                    if cs.tg_dept_cache.get(&tg).map(|d| d != dept).unwrap_or(true) { return false; }
                } else { return false; }
            }
            if !search_lower.is_empty() {
                let summary = event_summary(evt, &cs.tg_name_cache).to_lowercase();
                let type_str = evt.event_type.to_lowercase();
                let dept_str = evt.talkgroup
                    .and_then(|tg| cs.tg_dept_cache.get(&tg))
                    .map(|d| d.to_lowercase())
                    .unwrap_or_default();
                if !summary.contains(&search_lower) && !type_str.contains(&search_lower) && !dept_str.contains(&search_lower) {
                    return false;
                }
            }
            true
        })
        .take(100)
        .collect();

    // --- Log (fills all remaining space) ---
    let expanded_idx = cs.dispatch_expanded;
    let abs_time = cs.dispatch_abs_time;

    let mut new_filter_tg: Option<Option<u32>> = None;
    let mut new_filter_uid: Option<Option<u32>> = None;
    let mut new_filter_dept: Option<Option<String>> = None;
    let mut new_expanded: Option<Option<usize>> = None;

    // Detect user scroll-away to auto-pause live tail
    let current_len = bridge.protocol_log.len();
    let _has_new = current_len != cs.dispatch_prev_len;
    cs.dispatch_prev_len = current_len;

    let scroll_out = egui::ScrollArea::vertical()
        .id_salt("dispatch_log")
        .auto_shrink([false, false])
        .stick_to_bottom(cs.dispatch_live_tail)
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("No events match filters")
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY),
                    );
                });
                return;
            }

            for &(idx, evt) in &filtered {
                let is_expanded = expanded_idx == Some(idx);
                let type_color = event_type_color(&evt.event_type);
                let type_label = event_type_label(&evt.event_type);
                let timestamp = format_timestamp(evt.timestamp, abs_time);
                let enc = evt.encrypted.unwrap_or(false);

                // Row background: subtle alternating + encryption tint
                let row_bg = if enc {
                    Color32::from_rgba_premultiplied(60, 10, 10, if idx % 2 == 0 { 30 } else { 45 })
                } else if idx % 2 == 1 {
                    Color32::from_rgba_premultiplied(20, 22, 30, 25)
                } else {
                    Color32::TRANSPARENT
                };

                let resp = ui.horizontal(|ui| {
                    let rect = ui.available_rect_before_wrap();
                    if row_bg.a() > 0 {
                        ui.painter().rect_filled(rect, 0.0, row_bg);
                    }
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Timestamp
                    ui.label(
                        egui::RichText::new(&timestamp)
                            .size(FONT_SIZE_HUD)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Type badge
                    ui.label(
                        egui::RichText::new(type_label)
                            .size(FONT_SIZE_HUD)
                            .color(type_color)
                            .family(egui::FontFamily::Monospace),
                    );

                    // Summary with clickable facets
                    match evt.event_type.as_str() {
                        "p25" | "tsbk" => {
                            if let Some(tg) = evt.talkgroup {
                                let tg_name = cs.tg_name_cache.get(&tg);
                                let tg_text = if let Some(name) = tg_name {
                                    format!("{tg} {name}")
                                } else {
                                    format!("{tg}")
                                };
                                if ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&tg_text)
                                            .size(FONT_SIZE_HUD)
                                            .color(CYAN_P25)
                                            .family(egui::FontFamily::Monospace),
                                    ).sense(egui::Sense::click()),
                                ).on_hover_text("Filter by TG").clicked() {
                                    new_filter_tg = Some(Some(tg));
                                }

                                if let Some(dept) = cs.tg_dept_cache.get(&tg) {
                                    if ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(dept)
                                                .size(FONT_SIZE_HUD)
                                                .color(GREEN_COLLECT)
                                                .family(egui::FontFamily::Monospace),
                                        ).sense(egui::Sense::click()),
                                    ).on_hover_text("Filter by dept").clicked() {
                                        new_filter_dept = Some(Some(dept.clone()));
                                    }
                                }
                            }

                            if let Some(uid) = evt.source_unit {
                                if ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("UID:{uid}"))
                                            .size(FONT_SIZE_HUD)
                                            .color(Color32::from_rgb(180, 140, 60))
                                            .family(egui::FontFamily::Monospace),
                                    ).sense(egui::Sense::click()),
                                ).on_hover_text("Filter by UID").clicked() {
                                    new_filter_uid = Some(Some(uid));
                                }
                            }

                            if let Some(ref opcode) = evt.opcode {
                                ui.label(
                                    egui::RichText::new(opcode)
                                        .size(FONT_SIZE_HUD)
                                        .color(TEXT_SECONDARY)
                                        .family(egui::FontFamily::Monospace),
                                );
                            }

                            if enc {
                                ui.label(
                                    egui::RichText::new("ENC")
                                        .size(FONT_SIZE_HUD)
                                        .color(RED_RECORDING)
                                        .family(egui::FontFamily::Monospace),
                                );
                            }
                        }
                        "rds" => {
                            let summary = event_summary(evt, &cs.tg_name_cache);
                            ui.label(
                                egui::RichText::new(&summary)
                                    .size(FONT_SIZE_HUD)
                                    .color(MAGENTA_EXPLOIT)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }
                        _ => {
                            let summary = event_summary(evt, &cs.tg_name_cache);
                            ui.label(
                                egui::RichText::new(&summary)
                                    .size(FONT_SIZE_HUD)
                                    .color(TEXT_PRIMARY)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }
                    }
                });

                // Click row to expand/collapse detail
                if resp.response.interact(egui::Sense::click()).clicked() {
                    new_expanded = Some(if is_expanded { None } else { Some(idx) });
                }

                // Expanded: raw JSON
                if is_expanded {
                    ui.indent("evt_detail", |ui| {
                        let json_str = serde_json::to_string_pretty(evt.raw.as_ref())
                            .unwrap_or_else(|_| format!("{:?}", evt.raw));
                        ui.add(
                            egui::TextEdit::multiline(&mut json_str.as_str())
                                .code_editor()
                                .desired_width(ui.available_width())
                                .desired_rows(json_str.lines().count().min(20))
                                .font(egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace)),
                        );
                    });
                }
            }
        });

    // Auto-pause live tail when user scrolls away from bottom
    if cs.dispatch_live_tail {
        let content_h = scroll_out.content_size.y;
        let inner_h = scroll_out.inner_rect.height();
        let cur_y = scroll_out.state.offset.y;
        let max_y = (content_h - inner_h).max(0.0);
        // Pause if scrolled away from bottom, or if user is actively scrolling up
        let scrolled_away = max_y > 0.0 && (max_y - cur_y) > 30.0;
        let user_scrolling = ui.input(|i| {
            i.raw_scroll_delta.y.abs() > 0.0 && i.raw_scroll_delta.y > 0.0
        });
        if scrolled_away || (user_scrolling && max_y > 0.0 && cur_y < max_y - 5.0) {
            cs.dispatch_live_tail = false;
        }
    }

    // "Jump to latest" floating button when not live-tailing
    if !cs.dispatch_live_tail {
        let btn_rect = scroll_out.inner_rect;
        let btn_pos = egui::pos2(
            btn_rect.center().x - 60.0,
            btn_rect.max.y - 32.0,
        );
        let btn_area = egui::Area::new(egui::Id::new("jump_to_latest"))
            .fixed_pos(btn_pos)
            .order(egui::Order::Foreground)
            .interactable(true);
        btn_area.show(ui.ctx(), |ui| {
            let btn = egui::Button::new(
                egui::RichText::new("\u{25BC} JUMP TO LATEST")
                    .size(FONT_SIZE_HUD)
                    .color(Color32::WHITE)
                    .family(egui::FontFamily::Monospace),
            )
            .fill(Color32::from_rgba_premultiplied(20, 120, 60, 220))
            .stroke(egui::Stroke::new(1.0, GREEN_COLLECT))
            .corner_radius(egui::CornerRadius::same(3));
            if ui.add(btn).clicked() {
                cs.dispatch_live_tail = true;
            }
        });
    }

    // Apply deferred state changes
    if let Some(tg) = new_filter_tg { cs.dispatch_filter_tg = tg; }
    if let Some(uid) = new_filter_uid { cs.dispatch_filter_uid = uid; }
    if let Some(dept) = new_filter_dept { cs.dispatch_filter_dept = dept; }
    if let Some(exp) = new_expanded { cs.dispatch_expanded = exp; }
}

// ── Field Panel ─────────────────────────────────────────────

fn show_field_panel(ui: &mut egui::Ui, bridge: &UiBridge, state: &rf_web::AppState, cs: &mut CollectState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        // ── GPS-01: GPS Instrument Card ──
        show_gps_instrument(ui, bridge);
        ui.add_space(8.0);

        // ── GPS-02: Source Configuration ──
        show_gps_source_config(ui, bridge, state, cs);
        ui.add_space(8.0);

        // ── GPS-03: Geofence Status ──
        show_geofence_status(ui, bridge);
        ui.add_space(8.0);

        // ── GPS-06: Collection Metrics ──
        show_collection_metrics(ui, bridge);
    });
}

/// GPS-01: Full GPS instrument display
fn show_gps_instrument(ui: &mut egui::Ui, bridge: &UiBridge) {
    let lat = bridge.hb_f64("gps_lat");
    let lon = bridge.hb_f64("gps_lon");
    let alt_m = bridge.hb_f64("gps_alt_m");
    let heading = bridge.hb_f64("gps_heading_deg");
    let speed_mps = bridge.hb_f64("gps_speed_mps");
    let sats = bridge.hb_u64("gps_sats");
    let hdop = bridge.hb_f64("gps_hdop");
    let fix = bridge.hb_str("gps_fix").to_string();
    let source = bridge.hb_str("gps_source").to_string();

    // Fix quality badge
    let (quality_label, quality_color) = if fix == "none" || fix == "--" {
        ("NO FIX", RED_WATCHDOG)
    } else if hdop > 0.0 && hdop < 2.0 {
        ("GOOD", GREEN_COLLECT)
    } else if hdop < 5.0 {
        ("FAIR", AMBER_WARNING)
    } else if hdop > 0.0 {
        ("POOR", RED_WATCHDOG)
    } else {
        // hdop == 0 means unknown
        ("FIX", GREEN_COLLECT)
    };

    // Header row
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("GPS POSITION")
            .size(FONT_SIZE_HEADER).color(GREEN_COLLECT)
            .family(egui::FontFamily::Monospace));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(quality_label)
                .size(FONT_SIZE_HUD).color(quality_color)
                .family(egui::FontFamily::Monospace));
            ui.label(egui::RichText::new(format!("{}", fix.to_uppercase()))
                .size(FONT_SIZE_HUD).color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace));
        });
    });
    ui.separator();

    if lat.abs() > 0.001 || lon.abs() > 0.001 {
        // Coordinates — large font, decimal degrees
        ui.label(egui::RichText::new(format!("{:.6}, {:.6}", lat, lon))
            .size(FONT_SIZE_LARGE).color(TEXT_PRIMARY)
            .family(egui::FontFamily::Monospace));

        ui.add_space(4.0);

        // Altitude + Heading + Speed row
        ui.horizontal(|ui| {
            let alt_ft = alt_m * 3.28084;
            field_kv(ui, "Alt", &format!("{:.0}m ({:.0}ft)", alt_m, alt_ft));

            let cardinal = heading_to_cardinal(heading);
            field_kv(ui, "Hdg", &format!("{:.0}{} {}", heading, '\u{00B0}', cardinal));

            let speed_kmh = speed_mps * 3.6;
            field_kv(ui, "Spd", &format!("{:.1} km/h", speed_kmh));
        });

        // Sats + HDOP row
        ui.horizontal(|ui| {
            field_kv(ui, "Sats", &format!("{}", sats));

            if hdop > 0.0 {
                let hdop_label = if hdop < 1.0 { "IDEAL" }
                    else if hdop < 2.0 { "EXCELLENT" }
                    else if hdop < 5.0 { "MODERATE" }
                    else { "POOR" };
                ui.label(egui::RichText::new(format!("HDOP: {:.1}", hdop))
                    .size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace));
                ui.label(egui::RichText::new(format!("[{}]", hdop_label))
                    .size(FONT_SIZE_DATA).color(quality_color)
                    .family(egui::FontFamily::Monospace));
            }

            field_kv(ui, "Src", &source.to_uppercase());
        });
    } else {
        ui.label(egui::RichText::new(format!("No position fix (source: {})", source))
            .size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
            .family(egui::FontFamily::Monospace));
    }
}

/// GPS-02: Source configuration panel
fn show_gps_source_config(ui: &mut egui::Ui, bridge: &UiBridge, state: &rf_web::AppState, cs: &mut CollectState) {
    ui.label(egui::RichText::new("SOURCE")
        .size(FONT_SIZE_HEADER).color(CYAN_P25)
        .family(egui::FontFamily::Monospace));
    ui.separator();

    // Read current config source from heartbeat
    let current_source = bridge.hb_str("gps_config_source").to_string();

    // Initialize selected source from heartbeat if empty
    if cs.gps_selected_source.is_empty() {
        cs.gps_selected_source = current_source.clone();
    }

    let sources = [
        ("external", "External GNSS (serial)"),
        ("simulation", "Simulation (Portland metro loop)"),
        ("fixed", "Fixed Position"),
        ("none", "None"),
    ];

    let mut changed = false;
    for (key, label) in &sources {
        let selected = cs.gps_selected_source == *key;
        if ui.radio(selected, egui::RichText::new(*label)
            .size(FONT_SIZE_DATA).color(TEXT_PRIMARY)
            .family(egui::FontFamily::Monospace)).clicked() {
            cs.gps_selected_source = key.to_string();
            changed = true;
        }
    }

    // GPS-04: Fixed position inputs (shown when fixed is selected)
    if cs.gps_selected_source == "fixed" {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Lat:")
                .size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace));
            ui.add(egui::TextEdit::singleline(&mut cs.gps_fixed_lat_input)
                .desired_width(100.0)
                .hint_text("45.5120")
                .font(egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace)));

            ui.label(egui::RichText::new("Lon:")
                .size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace));
            ui.add(egui::TextEdit::singleline(&mut cs.gps_fixed_lon_input)
                .desired_width(100.0)
                .hint_text("-122.6580")
                .font(egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace)));

            if ui.button(egui::RichText::new("SET FIXED")
                .size(FONT_SIZE_DATA).color(Color32::WHITE)
                .family(egui::FontFamily::Monospace)).clicked() {
                if let (Ok(lat), Ok(lon)) = (
                    cs.gps_fixed_lat_input.trim().parse::<f64>(),
                    cs.gps_fixed_lon_input.trim().parse::<f64>(),
                ) {
                    crate::commands::set_gps_fixed(state, lat, lon);
                }
            }
        });
    }

    // Apply source change
    if changed {
        crate::commands::set_gps_source(state, &cs.gps_selected_source);
    }
}

/// GPS-03: Geofence status panel
fn show_geofence_status(ui: &mut egui::Ui, bridge: &UiBridge) {
    ui.label(egui::RichText::new("GEOFENCE")
        .size(FONT_SIZE_HEADER).color(AMBER_WARNING)
        .family(egui::FontFamily::Monospace));
    ui.separator();

    let site_id = bridge.hb_u64("active_site_id");
    if site_id > 0 {
        let site_name = bridge.hb_str("active_site_name");
        let display_name = if site_name != "--" && !site_name.is_empty() {
            site_name.to_string()
        } else {
            format!("Site #{}", site_id)
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("AT SITE:")
                .size(FONT_SIZE_DATA).color(GREEN_COLLECT)
                .family(egui::FontFamily::Monospace));
            ui.label(egui::RichText::new(&display_name)
                .size(FONT_SIZE_DATA).color(TEXT_PRIMARY)
                .family(egui::FontFamily::Monospace));
        });
    } else {
        ui.label(egui::RichText::new("NO ACTIVE GEOFENCE")
            .size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
            .family(egui::FontFamily::Monospace));
    }
}

/// GPS-06: Collection metrics
fn show_collection_metrics(ui: &mut egui::Ui, bridge: &UiBridge) {
    ui.label(egui::RichText::new("COLLECTION")
        .size(FONT_SIZE_HEADER).color(TEXT_SECONDARY)
        .family(egui::FontFamily::Monospace));
    ui.separator();

    let sweeps = bridge.hb_u64("sweeps");
    let uptime = bridge.hb_u64("uptime");
    let op_name = bridge.hb_str("operation_name").to_string();
    let rec_count = bridge.hb_u64("recording_active_count");

    // Signals detected: count from signal rows in spectrum
    let total_signals: usize = bridge.spectrum.values().map(|f| f.signals.len()).sum();

    ui.horizontal(|ui| {
        field_kv(ui, "Sweeps", &format_count(sweeps));
        field_kv(ui, "Uptime", &format_duration(uptime as f64));
    });
    ui.horizontal(|ui| {
        field_kv(ui, "Signals", &format!("{}", total_signals));
        if rec_count > 0 {
            field_kv(ui, "Recordings", &format!("{} active", rec_count));
        }
    });
    if op_name != "--" && !op_name.is_empty() {
        ui.horizontal(|ui| {
            field_kv(ui, "Operation", &op_name);
        });
    }
}

/// Helper: key-value label pair
fn field_kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.label(egui::RichText::new(format!("{}: ", key))
        .size(FONT_SIZE_DATA).color(TEXT_SECONDARY)
        .family(egui::FontFamily::Monospace));
    ui.label(egui::RichText::new(value)
        .size(FONT_SIZE_DATA).color(TEXT_PRIMARY)
        .family(egui::FontFamily::Monospace));
    ui.add_space(8.0);
}

/// Convert heading degrees to cardinal direction
fn heading_to_cardinal(deg: f64) -> &'static str {
    if deg < 0.0 { return "?" }
    let idx = ((deg + 22.5) / 45.0) as usize % 8;
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][idx]
}

/// Format a large count with commas
fn format_count(n: u64) -> String {
    if n < 1000 { return n.to_string(); }
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Aggregate signals from all spectrum bands into a flat list
fn update_signal_rows(bridge: &UiBridge, collect_state: &mut CollectState) {
    collect_state.signal_rows.clear();
    for (band, frame) in &bridge.spectrum {
        for sig in &frame.signals {
            collect_state.signal_rows.push(signal_log::SignalRow {
                freq_mhz: sig.freq_mhz,
                power_db: sig.power_db,
                classification: sig.classification.clone(),
                name: sig.name.clone(),
                mode: sig.mode.clone(),
                band: band.clone(),
            });
        }
    }
    // Sort by power descending (strongest first)
    collect_state
        .signal_rows
        .sort_by(|a, b| b.power_db.partial_cmp(&a.power_db).unwrap_or(std::cmp::Ordering::Equal));
}

// ── Recording Controls ──────────────────────────────────

fn show_rec_controls(
    ui: &mut egui::Ui,
    bridge: &UiBridge,
    state: &rf_web::AppState,
    cs: &mut CollectState,
    rec_cmd_tx: &mpsc::Sender<rf_recorder::RecorderCommand>,
) {
    rec_section_header(ui, "MANUAL RECORDING");

    // Label input
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Label:")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
            .family(egui::FontFamily::Monospace));
        ui.add(egui::TextEdit::singleline(&mut cs.rec_label)
            .desired_width(200.0)
            .hint_text("optional label"));
    });

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        // RECORD AUDIO button
        let audio_btn = egui::Button::new(
            egui::RichText::new("REC AUDIO")
                .color(Color32::WHITE).size(FONT_SIZE_DATA).strong()
        ).fill(RED_RECORDING);
        if ui.add(audio_btn).clicked() {
            start_manual_recording(bridge, state, cs, rec_cmd_tx, "audio");
        }

        // RECORD IQ button
        let iq_btn = egui::Button::new(
            egui::RichText::new("REC IQ")
                .color(Color32::WHITE).size(FONT_SIZE_DATA).strong()
        ).fill(CYAN_P25);
        if ui.add(iq_btn).clicked() {
            start_manual_recording(bridge, state, cs, rec_cmd_tx, "iq");
        }

        // STOP ALL button
        let stop_btn = egui::Button::new(
            egui::RichText::new("STOP ALL")
                .color(Color32::WHITE).size(FONT_SIZE_DATA).strong()
        ).fill(Color32::from_rgb(120, 40, 40));
        if ui.add(stop_btn).clicked() {
            let _ = rec_cmd_tx.send(rf_recorder::RecorderCommand::StopAll);
        }

        // Show current freq/mod for context
        let freq = bridge.hb_f64("freq_mhz");
        let modulation = bridge.hb_str("modulation").to_string();
        if freq > 0.0 {
            ui.label(egui::RichText::new(format!("{:.4} MHz {}", freq, modulation))
                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD)
                .family(egui::FontFamily::Monospace));
        }
    });
}

fn start_manual_recording(
    bridge: &UiBridge,
    state: &rf_web::AppState,
    cs: &mut CollectState,
    rec_cmd_tx: &mpsc::Sender<rf_recorder::RecorderCommand>,
    rec_type: &str,
) {
    let freq_mhz = bridge.hb_f64("freq_mhz");
    let modulation = bridge.hb_str("modulation").to_string();
    let sample_rate = bridge.hb_u64("sample_rate") as i32;

    // Build file path under data/recordings/
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().and_then(|p| p.parent())
        .map(|p| p.join("data").join("recordings"))
        .unwrap_or_default();
    std::fs::create_dir_all(&data_dir).ok();

    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let file_name = if rec_type == "audio" {
        format!("audio_{}.wav", ts)
    } else {
        format!("iq_{}.raw", ts)
    };
    let file_path = data_dir.join(&file_name);

    let label = if cs.rec_label.is_empty() { None } else { Some(cs.rec_label.as_str()) };
    let sr = if rec_type == "iq" && sample_rate > 0 { sample_rate } else { 48000 };

    // Create DB row
    match state.db().create_recording(
        rec_type,
        freq_mhz,
        Some(&modulation),
        label,
        sr,
        &file_path.to_string_lossy(),
        "manual",
        None, // tgid
        None, // device_key
        None, // lat
        None, // lon
        None, // site_id
        None, // site_session_id
        None, // operation_id
        None, // source_unit
        false, // encrypted
        None, // algorithm
        None, // key_id
    ) {
        Ok(db_id) => {
            let cmd = if rec_type == "audio" {
                rf_recorder::RecorderCommand::StartAudio {
                    db_id,
                    freq_mhz,
                    file_path,
                }
            } else {
                rf_recorder::RecorderCommand::StartIq {
                    db_id,
                    freq_mhz,
                    file_path,
                    sample_rate: sr as u32,
                }
            };
            let _ = rec_cmd_tx.send(cmd);
            tracing::info!("Manual {} recording started: db_id={}, freq={:.4}", rec_type, db_id, freq_mhz);
        }
        Err(e) => {
            tracing::error!("Failed to create recording DB row: {}", e);
        }
    }
}

// ── Recording Panel ─────────────────────────────────────

const REC_POLL_INTERVAL: f64 = 3.0;

fn show_rec_panel(
    ui: &mut egui::Ui,
    bridge: &UiBridge,
    state: &rf_web::AppState,
    cs: &mut CollectState,
    rec_cmd_tx: &mpsc::Sender<rf_recorder::RecorderCommand>,
    playback_tx: &mpsc::Sender<Vec<f32>>,
) {
    // Periodic DB refresh
    let now = ui.input(|i| i.time);
    if now - cs.rec_last_poll > REC_POLL_INTERVAL {
        cs.rec_last_poll = now;
        refresh_rec_cache(state, cs);
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        // 0. Manual record/stop controls
        show_rec_controls(ui, bridge, state, cs, rec_cmd_tx);

        ui.add_space(8.0);
        ui.separator();

        // 1. Active recordings (live from heartbeat)
        show_active_recordings(ui, bridge, rec_cmd_tx);

        ui.add_space(8.0);
        ui.separator();

        // 2. Auto-clip toggle
        show_auto_clip_control(ui, bridge, state);

        ui.add_space(8.0);
        ui.separator();

        // 3. Recording stats + history
        show_recording_stats(ui, cs);

        ui.add_space(8.0);
        ui.separator();

        // 4. Recording history
        show_recording_history(ui, cs, playback_tx);

        ui.add_space(8.0);
        ui.separator();

        // 5. P25 clip browser
        show_clip_browser(ui, state, cs, playback_tx);

        ui.add_space(8.0);
        ui.separator();

        // 6. Clip grouping (TG + Freq)
        show_clip_grouping(ui, cs);

        ui.add_space(8.0);
        ui.separator();

        // 7. IQ captures
        show_iq_captures(ui, state, cs);

        ui.add_space(8.0);
        ui.separator();

        // 8. Auto-IQ rules (REC-10)
        show_auto_iq_rules(ui, state, cs);

        ui.add_space(8.0);
        ui.separator();

        // 9. Storage management (REC-12)
        show_storage_management(ui, state, cs);
    });

    // IQ Viewer window (rendered outside scroll area)
    show_iq_viewer_window(ui.ctx(), state, cs);
}

fn refresh_rec_cache(state: &rf_web::AppState, cs: &mut CollectState) {
    if let Ok(stats) = state.db().recording_stats() {
        cs.rec_stats = Some(stats);
    }
    if let Ok(recs) = state.db().list_recordings(None, 100) {
        cs.rec_history = recs;
    }
    if let Ok(clips) = state.db().list_clips(cs.rec_selected_clip_tgid, None, 100) {
        cs.rec_clips = clips;
    }
    if let Ok(clip_stats) = state.db().clip_stats(None) {
        cs.rec_clip_stats = Some(clip_stats);
    }
    if let Ok(tg_groups) = state.db().clip_tg_groups(None, None) {
        cs.rec_tg_groups = tg_groups;
    }
    if let Ok(freq_groups) = state.db().clip_freq_groups(None) {
        cs.rec_freq_groups = freq_groups;
    }
    if let Ok(iq) = state.db().list_iq_captures(None, 100) {
        cs.rec_iq_captures = iq;
    }
    if let Ok(rules) = state.db().list_auto_iq_rules(None) {
        cs.auto_iq_rules = rules;
    }
}

fn rec_section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(text)
            .size(FONT_SIZE_HEADER)
            .color(RED_RECORDING)
            .family(egui::FontFamily::Monospace),
    );
    ui.add_space(4.0);
}

// ── Active Recordings ───────────────────────────────────

fn show_active_recordings(
    ui: &mut egui::Ui,
    bridge: &UiBridge,
    rec_cmd_tx: &mpsc::Sender<rf_recorder::RecorderCommand>,
) {
    let active_count = bridge.hb_u64("recording_active_count");
    rec_section_header(ui, &format!("ACTIVE RECORDINGS ({})", active_count));

    if active_count == 0 {
        ui.label(egui::RichText::new("No active recordings.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Extract active audio/IQ slots from heartbeat JSON
    let hb = match &bridge.heartbeat {
        Some(hb) => hb,
        None => return,
    };

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(6))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            // Audio slots
            if let Some(audio_arr) = hb.get("recording_active_audio").and_then(|v| v.as_array()) {
                for slot in audio_arr {
                    show_active_slot(ui, slot, "AUDIO", rec_cmd_tx);
                }
            }
            // IQ slots
            if let Some(iq_arr) = hb.get("recording_active_iq").and_then(|v| v.as_array()) {
                for slot in iq_arr {
                    show_active_slot(ui, slot, "IQ", rec_cmd_tx);
                }
            }
        });
}

fn show_active_slot(
    ui: &mut egui::Ui,
    slot: &serde_json::Value,
    slot_type: &str,
    rec_cmd_tx: &mpsc::Sender<rf_recorder::RecorderCommand>,
) {
    let db_id = slot.get("db_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let freq = slot.get("freq_mhz").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let duration = slot.get("duration_sec").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let size = slot.get("file_size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);

    ui.horizontal(|ui| {
        // STOP button for this individual recording
        let stop_btn = egui::Button::new(
            egui::RichText::new("STOP").color(Color32::WHITE).size(FONT_SIZE_HUD)
        ).fill(Color32::from_rgb(160, 40, 40));
        if ui.add(stop_btn).clicked() {
            let _ = rec_cmd_tx.send(rf_recorder::RecorderCommand::Stop { db_id });
            tracing::info!("Stopping recording db_id={}", db_id);
        }

        // Pulsing red dot
        ui.label(egui::RichText::new("●").color(RED_RECORDING).size(FONT_SIZE_DATA));

        ui.label(egui::RichText::new(slot_type)
            .color(RED_RECORDING).size(FONT_SIZE_DATA).strong());

        ui.label(egui::RichText::new(format!("#{}", db_id))
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
            .family(egui::FontFamily::Monospace));

        ui.label(egui::RichText::new(format!("{:.4} MHz", freq))
            .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
            .family(egui::FontFamily::Monospace));

        let dur_str = format_duration(duration);
        ui.label(egui::RichText::new(&dur_str)
            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
            .family(egui::FontFamily::Monospace));

        ui.label(egui::RichText::new(format_size(size))
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
            .family(egui::FontFamily::Monospace));
    });
}

// ── Auto-Clip Control ───────────────────────────────────

fn show_auto_clip_control(ui: &mut egui::Ui, bridge: &UiBridge, state: &rf_web::AppState) {
    let auto_clip = bridge.hb_bool("auto_clip_enabled");

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("AUTO-CLIP")
            .color(RED_RECORDING).size(FONT_SIZE_HEADER)
            .family(egui::FontFamily::Monospace));

        let label = if auto_clip { "ENABLED" } else { "DISABLED" };
        let color = if auto_clip { GREEN_COLLECT } else { TEXT_SECONDARY };
        if ui.add(egui::Button::new(
            egui::RichText::new(label).color(color).size(FONT_SIZE_DATA)
        )).clicked() {
            state.update_config(|c| {
                c.auto_clip_enabled = !c.auto_clip_enabled;
            });
        }

        ui.label(egui::RichText::new("Per-transmission P25 clip recording")
            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
    });
}

// ── Recording Stats ─────────────────────────────────────

fn show_recording_stats(ui: &mut egui::Ui, cs: &CollectState) {
    rec_section_header(ui, "RECORDING STATISTICS");

    let stats = match &cs.rec_stats {
        Some(s) => s,
        None => {
            ui.label(egui::RichText::new("No recording data.")
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            return;
        }
    };

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(6))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;

                rec_stat_badge(ui, "TOTAL", stats.total_count, TEXT_PRIMARY);
                rec_stat_badge(ui, "AUDIO", stats.audio_count, GREEN_COLLECT);
                rec_stat_badge(ui, "IQ", stats.iq_count, CYAN_P25);

                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(format_size(stats.total_size_bytes as u64))
                        .color(AMBER_WARNING).size(FONT_SIZE_LARGE).strong()
                        .family(egui::FontFamily::Monospace));
                    ui.label(egui::RichText::new("TOTAL SIZE")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                });

                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(format_duration(stats.total_duration_sec))
                        .color(GREEN_COLLECT).size(FONT_SIZE_LARGE).strong()
                        .family(egui::FontFamily::Monospace));
                    ui.label(egui::RichText::new("TOTAL TIME")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                });
            });

            // Clip stats
            if let Some(ref clip_stats) = cs.rec_clip_stats {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    rec_stat_badge(ui, "CLIPS", clip_stats.total_clips, RED_RECORDING);
                    rec_stat_badge(ui, "P25", clip_stats.p25_clips, CYAN_P25);
                    rec_stat_badge(ui, "ANALOG", clip_stats.analog_clips, GREEN_COLLECT);
                    rec_stat_badge(ui, "TODAY", clip_stats.today_clips, AMBER_WARNING);
                });
            }
        });
}

fn rec_stat_badge(ui: &mut egui::Ui, label: &str, count: i64, color: Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(format!("{}", count))
            .color(color).size(FONT_SIZE_LARGE).strong()
            .family(egui::FontFamily::Monospace));
        ui.label(egui::RichText::new(label)
            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
    });
}

// ── Recording History ───────────────────────────────────

fn show_recording_history(
    ui: &mut egui::Ui,
    cs: &mut CollectState,
    playback_tx: &mpsc::Sender<Vec<f32>>,
) {
    rec_section_header(ui, &format!("RECORDING HISTORY ({})", cs.rec_history.len()));

    if cs.rec_history.is_empty() {
        ui.label(egui::RichText::new("No recordings in database.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    // Check if playback thread finished (stop flag was set by thread)
    if cs.playback_active && cs.playback_stop.load(Ordering::Relaxed) {
        cs.playback_active = false;
        cs.playback_db_id = None;
    }

    let history_snapshot = cs.rec_history.clone();

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("rec_history_table")
                .num_columns(9)
                .spacing([8.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["", "TYPE", "FREQ", "MOD", "LABEL", "DUR", "SIZE", "ENCR", "START"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    for rec in history_snapshot.iter().take(50) {
                        // PLAY/STOP button for WAV recordings
                        let is_playing = cs.playback_active && cs.playback_db_id == Some(rec.id);
                        if rec.rec_type == "audio" && std::path::Path::new(&rec.file_path).exists() {
                            if is_playing {
                                let stop_btn = egui::Button::new(
                                    egui::RichText::new("STOP").color(Color32::WHITE).size(FONT_SIZE_HUD)
                                ).fill(Color32::from_rgb(160, 40, 40));
                                if ui.add(stop_btn).clicked() {
                                    cs.playback_stop.store(true, Ordering::Relaxed);
                                    cs.playback_active = false;
                                    cs.playback_db_id = None;
                                }
                            } else {
                                let play_btn = egui::Button::new(
                                    egui::RichText::new("PLAY").color(Color32::WHITE).size(FONT_SIZE_HUD)
                                ).fill(Color32::from_rgb(40, 120, 40));
                                if ui.add(play_btn).clicked() {
                                    start_playback(cs, playback_tx, &rec.file_path, rec.id, rec.sample_rate);
                                }
                            }
                        } else {
                            ui.label(egui::RichText::new("—").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        }

                        // Type
                        let type_color = match rec.rec_type.as_str() {
                            "audio" => GREEN_COLLECT,
                            "iq" => CYAN_P25,
                            _ => TEXT_PRIMARY,
                        };
                        ui.label(egui::RichText::new(rec.rec_type.to_uppercase())
                            .color(type_color).size(FONT_SIZE_DATA));

                        // Freq
                        ui.label(egui::RichText::new(format!("{:.4}", rec.freq_mhz))
                            .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Modulation
                        let modulation = rec.modulation.as_deref().unwrap_or("—");
                        ui.label(egui::RichText::new(modulation)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Label
                        let label = rec.label.as_deref().unwrap_or("—");
                        ui.label(egui::RichText::new(truncate_str(label, 16))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                        // Duration
                        let dur = format_duration(rec.duration_sec);
                        ui.label(egui::RichText::new(&dur)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Size
                        let size = format_size(rec.file_size_bytes as u64);
                        ui.label(egui::RichText::new(&size)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Encrypted
                        let enc_str = if rec.encrypted { "ENC" } else { "—" };
                        let enc_color = if rec.encrypted { RED_WATCHDOG } else { TEXT_SECONDARY };
                        ui.label(egui::RichText::new(enc_str)
                            .color(enc_color).size(FONT_SIZE_DATA));

                        // Start time
                        let start = rec.start_time.get(..16).unwrap_or(&rec.start_time);
                        ui.label(egui::RichText::new(start)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        ui.end_row();
                    }
                });
        });
}

/// Start WAV playback on a background thread
fn start_playback(
    cs: &mut CollectState,
    playback_tx: &mpsc::Sender<Vec<f32>>,
    file_path: &str,
    db_id: i64,
    sample_rate: i32,
) {
    // Stop any existing playback
    cs.playback_stop.store(true, Ordering::Relaxed);

    let stop_flag = Arc::new(AtomicBool::new(false));
    cs.playback_stop = Arc::clone(&stop_flag);
    cs.playback_active = true;
    cs.playback_db_id = Some(db_id);

    let tx = playback_tx.clone();
    let path = file_path.to_string();
    let _sr = sample_rate;

    std::thread::Builder::new()
        .name("wav_playback".into())
        .spawn(move || {
            let reader = match hound::WavReader::open(&path) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Failed to open WAV for playback: {}", e);
                    stop_flag.store(true, Ordering::Relaxed);
                    return;
                }
            };

            let spec = reader.spec();
            let wav_sr = spec.sample_rate;
            let channels = spec.channels as usize;

            // Read all samples as f32
            let samples: Vec<f32> = match spec.sample_format {
                hound::SampleFormat::Float => {
                    reader.into_samples::<f32>()
                        .filter_map(|s| s.ok())
                        .collect()
                }
                hound::SampleFormat::Int => {
                    let bits = spec.bits_per_sample;
                    let max_val = (1i32 << (bits - 1)) as f32;
                    reader.into_samples::<i32>()
                        .filter_map(|s| s.ok())
                        .map(|s| s as f32 / max_val)
                        .collect()
                }
            };

            // Mono-mix if stereo
            let mono: Vec<f32> = if channels > 1 {
                samples.chunks(channels)
                    .map(|ch| ch.iter().sum::<f32>() / channels as f32)
                    .collect()
            } else {
                samples
            };

            // Simple nearest-neighbor resample if WAV rate != 48000
            let resampled: Vec<f32> = if wav_sr != 48000 {
                let ratio = wav_sr as f64 / 48000.0;
                let out_len = (mono.len() as f64 / ratio) as usize;
                (0..out_len)
                    .map(|i| {
                        let src_idx = ((i as f64) * ratio) as usize;
                        mono.get(src_idx).copied().unwrap_or(0.0)
                    })
                    .collect()
            } else {
                mono
            };

            // Send in chunks with timing to match real-time playback
            let chunk_size = 4096;
            let chunk_duration = std::time::Duration::from_secs_f64(chunk_size as f64 / 48000.0);

            for chunk in resampled.chunks(chunk_size) {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                if tx.send(chunk.to_vec()).is_err() {
                    break; // Channel disconnected
                }
                std::thread::sleep(chunk_duration);
            }

            // Signal completion
            stop_flag.store(true, Ordering::Relaxed);
        })
        .ok();
}

// ── P25 Clip Browser ────────────────────────────────────

fn show_clip_browser(
    ui: &mut egui::Ui,
    _state: &rf_web::AppState,
    cs: &mut CollectState,
    playback_tx: &mpsc::Sender<Vec<f32>>,
) {
    let title = if let Some(tgid) = cs.rec_selected_clip_tgid {
        format!("P25 CLIPS — TGID {} ({})", tgid, cs.rec_clips.len())
    } else {
        format!("P25 CLIPS ({})", cs.rec_clips.len())
    };
    rec_section_header(ui, &title);

    // TG filter
    if let Some(tgid) = cs.rec_selected_clip_tgid {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Filtering by TGID {}", tgid))
                .color(CYAN_P25).size(FONT_SIZE_HUD));
            if ui.small_button(egui::RichText::new("CLEAR")
                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD)
            ).clicked() {
                cs.rec_selected_clip_tgid = None;
                cs.rec_last_poll = 0.0; // force refresh
            }
        });
    }

    if cs.rec_clips.is_empty() {
        ui.label(egui::RichText::new("No auto-clips recorded.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("clip_browser_table")
                .num_columns(9)
                .spacing([8.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["", "TGID", "UID", "FREQ", "DUR", "SIZE", "ENCR", "ALGO", "TIME"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    let clips_snapshot = cs.rec_clips.clone();
                    for clip in clips_snapshot.iter().take(50) {
                        // PLAY button for clips (WAV files)
                        let is_playing = cs.playback_active && cs.playback_db_id == Some(clip.id);
                        if std::path::Path::new(&clip.file_path).exists() {
                            if is_playing {
                                let stop_btn = egui::Button::new(
                                    egui::RichText::new("STOP").color(Color32::WHITE).size(FONT_SIZE_HUD)
                                ).fill(Color32::from_rgb(160, 40, 40));
                                if ui.add(stop_btn).clicked() {
                                    cs.playback_stop.store(true, Ordering::Relaxed);
                                    cs.playback_active = false;
                                    cs.playback_db_id = None;
                                }
                            } else {
                                let play_btn = egui::Button::new(
                                    egui::RichText::new("PLAY").color(Color32::WHITE).size(FONT_SIZE_HUD)
                                ).fill(Color32::from_rgb(40, 120, 40));
                                if ui.add(play_btn).clicked() {
                                    start_playback(cs, playback_tx, &clip.file_path, clip.id, clip.sample_rate);
                                }
                            }
                        } else {
                            ui.label(egui::RichText::new("—").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        }

                        // TGID (clickable filter)
                        let tgid = clip.tgid.unwrap_or(0);
                        let is_filtered = cs.rec_selected_clip_tgid == Some(tgid);
                        let tgid_color = if is_filtered { CYAN_P25 } else { TEXT_PRIMARY };
                        if ui.add(egui::Label::new(
                            egui::RichText::new(format!("{}", tgid))
                                .color(tgid_color).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace)
                        ).sense(egui::Sense::click())).clicked() {
                            cs.rec_selected_clip_tgid = if is_filtered { None } else { Some(tgid) };
                            cs.rec_last_poll = 0.0;
                        }

                        // Source unit
                        let uid = clip.source_unit
                            .map(|u| format!("{}", u))
                            .unwrap_or_else(|| "—".to_string());
                        ui.label(egui::RichText::new(&uid)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Freq
                        ui.label(egui::RichText::new(format!("{:.4}", clip.freq_mhz))
                            .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Duration
                        let dur = format_duration(clip.duration_sec);
                        ui.label(egui::RichText::new(&dur)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Size
                        let size = format_size(clip.file_size_bytes as u64);
                        ui.label(egui::RichText::new(&size)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        // Encrypted
                        let enc_str = if clip.encrypted { "ENC" } else { "CLR" };
                        let enc_color = if clip.encrypted { RED_WATCHDOG } else { GREEN_COLLECT };
                        ui.label(egui::RichText::new(enc_str)
                            .color(enc_color).size(FONT_SIZE_DATA));

                        // Algorithm
                        let algo = clip.algorithm.as_deref().unwrap_or("—");
                        ui.label(egui::RichText::new(algo)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        // Time
                        let start = clip.start_time.get(..16).unwrap_or(&clip.start_time);
                        ui.label(egui::RichText::new(start)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        ui.end_row();
                    }
                });
        });
}

// ── Clip Grouping ───────────────────────────────────────

fn show_clip_grouping(ui: &mut egui::Ui, cs: &CollectState) {
    // TG groups
    rec_section_header(ui, &format!("CLIPS BY TALKGROUP ({})", cs.rec_tg_groups.len()));

    if !cs.rec_tg_groups.is_empty() {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_SURFACE)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("clip_tg_groups_table")
                    .num_columns(7)
                    .spacing([8.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["TGID", "NAME", "DEPT", "CLIPS", "DURATION", "SIZE", "UIDs"] {
                            ui.label(egui::RichText::new(h)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                        }
                        ui.end_row();

                        for g in &cs.rec_tg_groups {
                            let tgid_str = g.tgid.map(|t| format!("{}", t)).unwrap_or_else(|| "—".to_string());
                            ui.label(egui::RichText::new(&tgid_str)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            let name = g.tg_name.as_deref().unwrap_or("—");
                            ui.label(egui::RichText::new(truncate_str(name, 16))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));

                            let dept = g.department.as_deref().unwrap_or("—");
                            ui.label(egui::RichText::new(dept)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                            ui.label(egui::RichText::new(format!("{}", g.clip_count))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.label(egui::RichText::new(format_duration(g.total_duration))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.label(egui::RichText::new(format_size(g.total_size as u64))
                                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.label(egui::RichText::new(format!("{}", g.unique_uids))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.end_row();
                        }
                    });
            });
    } else {
        ui.label(egui::RichText::new("No talkgroup clip groups.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    }

    ui.add_space(8.0);

    // Freq groups
    rec_section_header(ui, &format!("CLIPS BY FREQUENCY ({})", cs.rec_freq_groups.len()));

    if !cs.rec_freq_groups.is_empty() {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_SURFACE)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("clip_freq_groups_table")
                    .num_columns(5)
                    .spacing([10.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["FREQ", "MOD", "CLIPS", "DURATION", "SIZE"] {
                            ui.label(egui::RichText::new(h)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                        }
                        ui.end_row();

                        for g in &cs.rec_freq_groups {
                            ui.label(egui::RichText::new(format!("{:.4} MHz", g.freq_mhz))
                                .color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            let modulation = g.modulation.as_deref().unwrap_or("—");
                            ui.label(egui::RichText::new(modulation)
                                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                            ui.label(egui::RichText::new(format!("{}", g.clip_count))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.label(egui::RichText::new(format_duration(g.total_duration))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.label(egui::RichText::new(format_size(g.total_size as u64))
                                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            ui.end_row();
                        }
                    });
            });
    } else {
        ui.label(egui::RichText::new("No frequency clip groups.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    }
}

// ── IQ Captures ─────────────────────────────────────────

fn show_iq_captures(ui: &mut egui::Ui, _state: &rf_web::AppState, cs: &mut CollectState) {
    rec_section_header(ui, &format!("IQ CAPTURES ({})", cs.rec_iq_captures.len()));

    if cs.rec_iq_captures.is_empty() {
        ui.label(egui::RichText::new("No IQ captures recorded.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
        return;
    }

    let iq_snapshot = cs.rec_iq_captures.clone();

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("iq_captures_table")
                .num_columns(7)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["", "FREQ", "RATE", "DUR", "SIZE", "TRIGGER", "TIME"] {
                        ui.label(egui::RichText::new(h)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                    }
                    ui.end_row();

                    for iq in &iq_snapshot {
                        // ANALYZE button
                        if std::path::Path::new(&iq.file_path).exists() {
                            let analyze_btn = egui::Button::new(
                                egui::RichText::new("ANALYZE").color(Color32::WHITE).size(FONT_SIZE_HUD)
                            ).fill(CYAN_P25);
                            if ui.add(analyze_btn).clicked() {
                                open_iq_viewer(cs, &iq.file_path, iq.freq_mhz, iq.sample_rate as f64);
                            }
                        } else {
                            ui.label(egui::RichText::new("—").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        }

                        ui.label(egui::RichText::new(format!("{:.4} MHz", iq.freq_mhz))
                            .color(CYAN_P25).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.label(egui::RichText::new(format!("{} Hz", iq.sample_rate))
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        let dur = format_duration(iq.duration_sec);
                        ui.label(egui::RichText::new(&dur)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        let size = format_size(iq.file_size_bytes as u64);
                        ui.label(egui::RichText::new(&size)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA)
                            .family(egui::FontFamily::Monospace));

                        ui.label(egui::RichText::new(&iq.trigger_type)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        let start = iq.start_time.get(..16).unwrap_or(&iq.start_time);
                        ui.label(egui::RichText::new(start)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

                        ui.end_row();
                    }
                });
        });
}

// ── Auto-IQ Rules (REC-10) ──────────────────────────────

const AUTO_IQ_TRIGGER_TYPES: &[&str] = &["tgid", "frequency", "encryption", "unknown_emitter"];

fn show_auto_iq_rules(ui: &mut egui::Ui, state: &rf_web::AppState, cs: &mut CollectState) {
    rec_section_header(ui, &format!("AUTO-IQ RULES ({})", cs.auto_iq_rules.len()));

    // Existing rules table
    if cs.auto_iq_rules.is_empty() {
        ui.label(egui::RichText::new("No auto-IQ rules configured.")
            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    } else {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(4))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                egui::Grid::new("auto_iq_rules_grid")
                    .min_col_width(40.0)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        // Header
                        ui.label(egui::RichText::new("TRIGGER").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        ui.label(egui::RichText::new("CONFIG").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        ui.label(egui::RichText::new("MAX DUR").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        ui.label(egui::RichText::new("ENABLED").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        ui.label(egui::RichText::new("").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        ui.end_row();

                        let mut toggle_id = None;
                        let mut toggle_val = false;
                        let mut delete_id = None;

                        for rule in &cs.auto_iq_rules {
                            ui.label(egui::RichText::new(&rule.trigger_type)
                                .color(CYAN_P25).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));
                            let config_text = rule.trigger_config_json.as_deref().unwrap_or("—");
                            ui.label(egui::RichText::new(config_text)
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));
                            ui.label(egui::RichText::new(format!("{}s", rule.max_duration_sec))
                                .color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                .family(egui::FontFamily::Monospace));

                            let mut enabled = rule.enabled;
                            if ui.checkbox(&mut enabled, "").changed() {
                                toggle_id = Some(rule.id);
                                toggle_val = enabled;
                            }

                            let del_btn = egui::Button::new(
                                egui::RichText::new("DEL").color(Color32::WHITE).size(FONT_SIZE_HUD)
                            ).fill(Color32::from_rgb(120, 30, 30));
                            if ui.add(del_btn).clicked() {
                                delete_id = Some(rule.id);
                            }
                            ui.end_row();
                        }

                        // Apply mutations after iteration
                        if let Some(id) = toggle_id {
                            let _ = state.db().toggle_auto_iq_rule(id, toggle_val);
                        }
                        if let Some(id) = delete_id {
                            let _ = state.db().delete_auto_iq_rule(id);
                            cs.auto_iq_last_poll = 0.0; // force refresh
                        }
                    });
            });
    }

    ui.add_space(6.0);

    // New rule form
    if !cs.new_rule_creating {
        if ui.add(egui::Button::new(
            egui::RichText::new("+ NEW RULE").color(Color32::WHITE).size(FONT_SIZE_DATA)
        ).fill(CYAN_P25)).clicked() {
            cs.new_rule_creating = true;
            cs.new_rule_trigger = "tgid".to_string();
            cs.new_rule_config.clear();
            cs.new_rule_max_dur = 30;
        }
    } else {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(6))
            .fill(BG_ELEVATED)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("NEW AUTO-IQ RULE")
                    .color(AMBER_WARNING).size(FONT_SIZE_HEADER));

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Trigger:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    egui::ComboBox::from_id_salt("auto_iq_trigger_combo")
                        .selected_text(&cs.new_rule_trigger)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for &t in AUTO_IQ_TRIGGER_TYPES {
                                ui.selectable_value(&mut cs.new_rule_trigger, t.to_string(), t);
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Config:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::TextEdit::singleline(&mut cs.new_rule_config)
                        .desired_width(180.0)
                        .hint_text("e.g. 1234 or 155.5200"));
                });

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Max duration:").color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.add(egui::Slider::new(&mut cs.new_rule_max_dur, 5..=300).suffix("s"));
                });

                ui.horizontal(|ui| {
                    let config_json = if cs.new_rule_config.is_empty() {
                        None
                    } else {
                        Some(cs.new_rule_config.as_str())
                    };

                    if ui.add(egui::Button::new(
                        egui::RichText::new("CREATE").color(Color32::WHITE).size(FONT_SIZE_DATA)
                    ).fill(GREEN_COLLECT)).clicked() {
                        let _ = state.db().create_auto_iq_rule(
                            &cs.new_rule_trigger,
                            config_json,
                            None,
                            cs.new_rule_max_dur,
                        );
                        cs.new_rule_creating = false;
                        cs.auto_iq_last_poll = 0.0; // force refresh
                    }

                    if ui.add(egui::Button::new(
                        egui::RichText::new("CANCEL").color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                    )).clicked() {
                        cs.new_rule_creating = false;
                    }
                });
            });
    }
}

// ── Storage Management (REC-12) ─────────────────────────

fn show_storage_management(ui: &mut egui::Ui, state: &rf_web::AppState, cs: &mut CollectState) {
    rec_section_header(ui, "STORAGE MANAGEMENT");

    let stats = match &cs.rec_stats {
        Some(s) => s.clone(),
        None => {
            ui.label(egui::RichText::new("No recording data.")
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            return;
        }
    };

    egui::Frame::NONE
        .inner_margin(egui::Margin::same(6))
        .fill(BG_ELEVATED)
        .corner_radius(4.0)
        .show(ui, |ui| {
            // Breakdown: total, audio, IQ, clips
            egui::Grid::new("storage_breakdown_grid")
                .min_col_width(80.0)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("TYPE").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    ui.label(egui::RichText::new("COUNT").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    ui.label(egui::RichText::new("SIZE").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    ui.end_row();

                    ui.label(egui::RichText::new("Total").color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                    ui.label(egui::RichText::new(format!("{}", stats.total_count))
                        .color(TEXT_PRIMARY).size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));
                    ui.label(egui::RichText::new(format_size(stats.total_size_bytes as u64))
                        .color(AMBER_WARNING).size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));
                    ui.end_row();

                    ui.label(egui::RichText::new("Audio").color(GREEN_COLLECT).size(FONT_SIZE_DATA));
                    ui.label(egui::RichText::new(format!("{}", stats.audio_count))
                        .color(GREEN_COLLECT).size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));
                    ui.label(egui::RichText::new("—")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.end_row();

                    ui.label(egui::RichText::new("IQ").color(CYAN_P25).size(FONT_SIZE_DATA));
                    ui.label(egui::RichText::new(format!("{}", stats.iq_count))
                        .color(CYAN_P25).size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));
                    ui.label(egui::RichText::new("—")
                        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                    ui.end_row();

                    if let Some(ref clip_stats) = cs.rec_clip_stats {
                        ui.label(egui::RichText::new("Clips").color(RED_RECORDING).size(FONT_SIZE_DATA));
                        ui.label(egui::RichText::new(format!("{}", clip_stats.total_clips))
                            .color(RED_RECORDING).size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));
                        ui.label(egui::RichText::new(format_size(clip_stats.today_size_bytes as u64))
                            .color(TEXT_SECONDARY).size(FONT_SIZE_DATA).family(egui::FontFamily::Monospace));
                        ui.end_row();
                    }
                });

            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("Total duration: {}", format_duration(stats.total_duration_sec)))
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));

            ui.add_space(8.0);

            // Bulk delete buttons with confirmation
            ui.horizontal(|ui| {
                // DELETE ALL IQ
                if cs.storage_delete_iq_armed {
                    if ui.add(egui::Button::new(
                        egui::RichText::new("CONFIRM DELETE ALL IQ").color(Color32::WHITE).size(FONT_SIZE_DATA)
                    ).fill(Color32::from_rgb(180, 30, 30))).clicked() {
                        delete_recordings_by_type(state, "iq");
                        cs.storage_delete_iq_armed = false;
                        cs.rec_last_poll = 0.0; // force refresh
                    }
                    if ui.add(egui::Button::new(
                        egui::RichText::new("CANCEL").color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                    )).clicked() {
                        cs.storage_delete_iq_armed = false;
                    }
                } else if stats.iq_count > 0 {
                    if ui.add(egui::Button::new(
                        egui::RichText::new("DELETE ALL IQ").color(Color32::WHITE).size(FONT_SIZE_DATA)
                    ).fill(Color32::from_rgb(120, 30, 30))).clicked() {
                        cs.storage_delete_iq_armed = true;
                    }
                }
            });

            ui.horizontal(|ui| {
                // DELETE ALL CLIPS
                if cs.storage_delete_clips_armed {
                    if ui.add(egui::Button::new(
                        egui::RichText::new("CONFIRM DELETE ALL CLIPS").color(Color32::WHITE).size(FONT_SIZE_DATA)
                    ).fill(Color32::from_rgb(180, 30, 30))).clicked() {
                        delete_clips(state);
                        cs.storage_delete_clips_armed = false;
                        cs.rec_last_poll = 0.0; // force refresh
                    }
                    if ui.add(egui::Button::new(
                        egui::RichText::new("CANCEL").color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                    )).clicked() {
                        cs.storage_delete_clips_armed = false;
                    }
                } else if cs.rec_clip_stats.as_ref().is_some_and(|s| s.total_clips > 0) {
                    if ui.add(egui::Button::new(
                        egui::RichText::new("DELETE ALL CLIPS").color(Color32::WHITE).size(FONT_SIZE_DATA)
                    ).fill(Color32::from_rgb(120, 30, 30))).clicked() {
                        cs.storage_delete_clips_armed = true;
                    }
                }
            });
        });
}

fn delete_recordings_by_type(state: &rf_web::AppState, rec_type: &str) {
    if let Ok(recs) = state.db().list_recordings(Some(rec_type), 10_000) {
        for rec in &recs {
            // Remove file from disk
            let _ = std::fs::remove_file(&rec.file_path);
            // Remove DB record
            let _ = state.db().delete_recording(rec.id);
        }
        tracing::info!("Deleted {} {} recordings", recs.len(), rec_type);
    }
}

fn delete_clips(state: &rf_web::AppState) {
    // Clips are recordings with trigger_type LIKE 'auto_%'
    if let Ok(clips) = state.db().list_clips(None, None, 10_000) {
        for clip in &clips {
            let _ = std::fs::remove_file(&clip.file_path);
            let _ = state.db().delete_recording(clip.id);
        }
        tracing::info!("Deleted {} clips", clips.len());
    }
}

/// Open the IQ viewer window and load IQ data from file
fn open_iq_viewer(cs: &mut CollectState, file_path: &str, freq_mhz: f64, sample_rate: f64) {
    cs.iq_viewer_open = true;
    cs.iq_viewer_file = Some(file_path.to_string());
    cs.iq_viewer_freq = freq_mhz;
    cs.iq_viewer_sample_rate = sample_rate;
    cs.iq_viewer_offset = 0;
    cs.iq_viewer_zoom = 4096;
    cs.iq_viewer_tab = IqViewerTab::TimeDomain;
    cs.iq_viewer_fingerprint = None;
    cs.iq_viewer_fp_matches.clear();
    cs.iq_viewer_fp_status = None;

    // Read IQ file: raw interleaved f32 pairs (I, Q, I, Q, ...)
    match std::fs::read(file_path) {
        Ok(bytes) => {
            let float_count = bytes.len() / 4;
            let pair_count = float_count / 2;
            // Cap at 1M samples to avoid memory issues
            let cap = pair_count.min(1_000_000);
            let mut data = Vec::with_capacity(cap);
            for i in 0..cap {
                let i_offset = i * 8;
                let q_offset = i_offset + 4;
                if q_offset + 4 <= bytes.len() {
                    let i_val = f32::from_le_bytes([
                        bytes[i_offset], bytes[i_offset + 1],
                        bytes[i_offset + 2], bytes[i_offset + 3],
                    ]);
                    let q_val = f32::from_le_bytes([
                        bytes[q_offset], bytes[q_offset + 1],
                        bytes[q_offset + 2], bytes[q_offset + 3],
                    ]);
                    data.push((i_val, q_val));
                }
            }
            tracing::info!("IQ viewer: loaded {} samples from {}", data.len(), file_path);
            cs.iq_viewer_data = Some(data);
        }
        Err(e) => {
            tracing::error!("Failed to read IQ file {}: {}", file_path, e);
            cs.iq_viewer_data = None;
        }
    }
}

/// IQ Viewer window — time domain, spectrum, fingerprint tabs
fn show_iq_viewer_window(ctx: &egui::Context, state: &rf_web::AppState, cs: &mut CollectState) {
    if !cs.iq_viewer_open {
        return;
    }

    let mut open = cs.iq_viewer_open;
    egui::Window::new("IQ ANALYZER")
        .open(&mut open)
        .default_size([800.0, 500.0])
        .resizable(true)
        .show(ctx, |ui| {
            // Header
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!(
                    "{:.4} MHz  |  {:.0} Hz",
                    cs.iq_viewer_freq, cs.iq_viewer_sample_rate
                ))
                .color(CYAN_P25).size(FONT_SIZE_DATA).strong()
                .family(egui::FontFamily::Monospace));

                if let Some(ref data) = cs.iq_viewer_data {
                    ui.label(egui::RichText::new(format!("  {} samples", data.len()))
                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                }
            });

            ui.separator();

            // Tab bar
            ui.horizontal(|ui| {
                for tab in [IqViewerTab::TimeDomain, IqViewerTab::Spectrum, IqViewerTab::Fingerprint] {
                    let label = match tab {
                        IqViewerTab::TimeDomain => "TIME DOMAIN",
                        IqViewerTab::Spectrum => "SPECTRUM",
                        IqViewerTab::Fingerprint => "FINGERPRINT",
                    };
                    let color = if cs.iq_viewer_tab == tab { CYAN_P25 } else { TEXT_SECONDARY };
                    if ui.add(egui::Button::new(
                        egui::RichText::new(label).color(color).size(FONT_SIZE_DATA)
                    )).clicked() {
                        cs.iq_viewer_tab = tab;
                    }
                }
            });

            ui.separator();

            match cs.iq_viewer_tab {
                IqViewerTab::TimeDomain => show_iq_time_domain(ui, cs),
                IqViewerTab::Spectrum => show_iq_spectrum(ui, cs),
                IqViewerTab::Fingerprint => show_iq_fingerprint(ui, state, cs),
            }
        });
    cs.iq_viewer_open = open;
}

// ── REC-07: IQ Time Domain View ────────────────────────────

fn show_iq_time_domain(ui: &mut egui::Ui, cs: &mut CollectState) {
    let data = match &cs.iq_viewer_data {
        Some(d) if !d.is_empty() => d,
        _ => {
            ui.label(egui::RichText::new("No IQ data loaded.")
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            return;
        }
    };

    let total = data.len();
    let window = cs.iq_viewer_zoom.min(total);
    let max_offset = total.saturating_sub(window);

    // Controls: zoom + scroll
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Window:").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
        let mut zoom_f = (cs.iq_viewer_zoom as f32).log2();
        if ui.add(egui::Slider::new(&mut zoom_f, 10.0..=16.0)
            .text("samples")
            .custom_formatter(|v, _| format!("{}", 1 << v as u32))
        ).changed() {
            cs.iq_viewer_zoom = 1 << zoom_f as u32;
        }
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Offset:").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
        let mut offset = cs.iq_viewer_offset;
        if ui.add(egui::Slider::new(&mut offset, 0..=max_offset)
            .text("samples")
        ).changed() {
            cs.iq_viewer_offset = offset;
        }
    });

    // Draw I/Q traces using Painter polylines
    let plot_height = ui.available_height().max(200.0);
    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width(), plot_height),
        egui::Sense::hover(),
    );
    let rect = response.rect;

    // Background with border
    painter.rect_filled(rect.expand(1.0), 0.0, BORDER);
    painter.rect_filled(rect, 0.0, BG_PRIMARY);

    // Center line
    let center_y = rect.center().y;
    painter.line_segment(
        [egui::pos2(rect.left(), center_y), egui::pos2(rect.right(), center_y)],
        egui::Stroke::new(0.5, BORDER_BRIGHT),
    );

    let start = cs.iq_viewer_offset.min(max_offset);
    let end = (start + window).min(total);
    let visible = &data[start..end];

    if visible.is_empty() {
        return;
    }

    // Find peak amplitude for auto-scaling
    let mut max_amp: f32 = 0.001;
    for &(i_val, q_val) in visible {
        max_amp = max_amp.max(i_val.abs()).max(q_val.abs());
    }

    let w = rect.width();
    let h = rect.height() / 2.0;
    let step = w / visible.len() as f32;

    // I trace (cyan)
    let i_points: Vec<egui::Pos2> = visible.iter().enumerate().map(|(idx, &(i_val, _))| {
        egui::pos2(
            rect.left() + idx as f32 * step,
            center_y - (i_val / max_amp) * h * 0.9,
        )
    }).collect();

    // Q trace (magenta)
    let q_points: Vec<egui::Pos2> = visible.iter().enumerate().map(|(idx, &(_, q_val))| {
        egui::pos2(
            rect.left() + idx as f32 * step,
            center_y - (q_val / max_amp) * h * 0.9,
        )
    }).collect();

    // Draw with stride to avoid too many points
    let max_points = (w as usize).max(1);
    let stride = (visible.len() / max_points).max(1);

    let i_strided: Vec<egui::Pos2> = i_points.iter().step_by(stride).copied().collect();
    let q_strided: Vec<egui::Pos2> = q_points.iter().step_by(stride).copied().collect();

    if i_strided.len() >= 2 {
        painter.add(egui::Shape::line(i_strided, egui::Stroke::new(1.0, CYAN_P25)));
    }
    if q_strided.len() >= 2 {
        painter.add(egui::Shape::line(q_strided, egui::Stroke::new(1.0, MAGENTA_EXPLOIT)));
    }

    // Legend
    let legend_y = rect.top() + 12.0;
    painter.text(
        egui::pos2(rect.left() + 8.0, legend_y),
        egui::Align2::LEFT_CENTER,
        "I",
        egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace),
        CYAN_P25,
    );
    painter.text(
        egui::pos2(rect.left() + 30.0, legend_y),
        egui::Align2::LEFT_CENTER,
        "Q",
        egui::FontId::new(FONT_SIZE_DATA, egui::FontFamily::Monospace),
        MAGENTA_EXPLOIT,
    );
}

// ── REC-08: IQ Spectrum View (FFT) ────────────────────────

fn show_iq_spectrum(ui: &mut egui::Ui, cs: &mut CollectState) {
    let data = match &cs.iq_viewer_data {
        Some(d) if !d.is_empty() => d,
        _ => {
            ui.label(egui::RichText::new("No IQ data loaded.")
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            return;
        }
    };

    let total = data.len();
    let fft_size = cs.iq_viewer_zoom.min(total).max(64).next_power_of_two();
    let start = cs.iq_viewer_offset.min(total.saturating_sub(fft_size));
    let end = (start + fft_size).min(total);
    let window_data = &data[start..end];

    // Controls
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("FFT Size:").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
        let mut zoom_f = (cs.iq_viewer_zoom as f32).log2();
        if ui.add(egui::Slider::new(&mut zoom_f, 6.0..=16.0)
            .text("bins")
            .custom_formatter(|v, _| format!("{}", 1 << v as u32))
        ).changed() {
            cs.iq_viewer_zoom = 1 << zoom_f as u32;
        }
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Offset:").color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
        let max_offset = total.saturating_sub(fft_size);
        let mut offset = cs.iq_viewer_offset.min(max_offset);
        if ui.add(egui::Slider::new(&mut offset, 0..=max_offset)).changed() {
            cs.iq_viewer_offset = offset;
        }
    });

    // Compute FFT
    let n = window_data.len().next_power_of_two();
    let mut fft_input: Vec<rustfft::num_complex::Complex<f32>> = Vec::with_capacity(n);

    // Apply Hann window and convert to Complex
    for (i, &(i_val, q_val)) in window_data.iter().enumerate() {
        let hann = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos());
        fft_input.push(rustfft::num_complex::Complex::new(i_val * hann, q_val * hann));
    }
    // Zero-pad to power of 2
    while fft_input.len() < n {
        fft_input.push(rustfft::num_complex::Complex::new(0.0, 0.0));
    }

    let mut planner = rustfft::FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut fft_input);

    // Convert to power spectrum (dB), with FFT shift
    let power_db: Vec<f32> = (0..n)
        .map(|i| {
            // FFT shift: put DC in center
            let idx = (i + n / 2) % n;
            let mag_sq = fft_input[idx].re * fft_input[idx].re + fft_input[idx].im * fft_input[idx].im;
            let db = 10.0 * (mag_sq / n as f32 + 1e-20).log10();
            db
        })
        .collect();

    // Draw spectrum
    let plot_height = ui.available_height().max(200.0);
    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width(), plot_height),
        egui::Sense::hover(),
    );
    let rect = response.rect;

    // Background with border
    painter.rect_filled(rect.expand(1.0), 0.0, BORDER);
    painter.rect_filled(rect, 0.0, BG_PRIMARY);

    if power_db.is_empty() {
        return;
    }

    // Find range for scaling
    let min_db = power_db.iter().copied().fold(f32::INFINITY, f32::min).max(-120.0);
    let max_db = power_db.iter().copied().fold(f32::NEG_INFINITY, f32::max).min(20.0);
    let range = (max_db - min_db).max(1.0);

    let w = rect.width();
    let h = rect.height();
    let step = w / power_db.len() as f32;

    let points: Vec<egui::Pos2> = power_db.iter().enumerate().map(|(i, &db)| {
        let x = rect.left() + i as f32 * step;
        let y = rect.bottom() - ((db - min_db) / range) * h * 0.9 - h * 0.05;
        egui::pos2(x, y)
    }).collect();

    // Stride for performance
    let max_points = (w as usize).max(1);
    let stride = (points.len() / max_points).max(1);
    let strided: Vec<egui::Pos2> = points.iter().step_by(stride).copied().collect();

    if strided.len() >= 2 {
        painter.add(egui::Shape::line(strided, egui::Stroke::new(1.5, GREEN_COLLECT)));
    }

    // Axis labels
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.top() + 10.0),
        egui::Align2::LEFT_CENTER,
        format!("{:.0} dB", max_db),
        egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace),
        TEXT_SECONDARY,
    );
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.bottom() - 10.0),
        egui::Align2::LEFT_CENTER,
        format!("{:.0} dB", min_db),
        egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace),
        TEXT_SECONDARY,
    );

    // Frequency axis (center = tuned freq)
    if cs.iq_viewer_sample_rate > 0.0 {
        let bw_mhz = cs.iq_viewer_sample_rate / 1e6;
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 10.0),
            egui::Align2::CENTER_CENTER,
            format!("{:.4} MHz", cs.iq_viewer_freq),
            egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace),
            CYAN_P25,
        );
        painter.text(
            egui::pos2(rect.left() + 4.0, rect.bottom() - 22.0),
            egui::Align2::LEFT_CENTER,
            format!("-{:.3}", bw_mhz / 2.0),
            egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace),
            TEXT_SECONDARY,
        );
        painter.text(
            egui::pos2(rect.right() - 4.0, rect.bottom() - 22.0),
            egui::Align2::RIGHT_CENTER,
            format!("+{:.3}", bw_mhz / 2.0),
            egui::FontId::new(FONT_SIZE_HUD, egui::FontFamily::Monospace),
            TEXT_SECONDARY,
        );
    }
}

// ── REC-09: IQ Fingerprint Extraction ──────────────────────

fn show_iq_fingerprint(ui: &mut egui::Ui, state: &rf_web::AppState, cs: &mut CollectState) {
    let data = match &cs.iq_viewer_data {
        Some(d) if !d.is_empty() => d,
        _ => {
            ui.label(egui::RichText::new("No IQ data loaded.")
                .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
            return;
        }
    };

    // Extract fingerprint if not cached
    if cs.iq_viewer_fingerprint.is_none() {
        let mut accum = rf_dsp::fingerprint::FingerprintAccumulator::new();
        let complex_samples: Vec<Complex32> = data.iter()
            .map(|&(i, q)| Complex32::new(i, q))
            .collect();
        accum.feed_iq(&complex_samples);
        cs.iq_viewer_fingerprint = accum.finalize();
    }

    rec_section_header(ui, "FINGERPRINT EXTRACTION");

    // Clone fingerprint to avoid borrow conflicts with closures
    let fp_clone = cs.iq_viewer_fingerprint.clone();

    match fp_clone {
        Some(fp) => {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(8))
                .fill(BG_ELEVATED)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    egui::Grid::new("fp_metrics")
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            fp_metric(ui, "CFO (Hz)", &format!("{:.2}", fp.cfo_hz));
                            fp_metric(ui, "IQ Amplitude Imbalance", &format!("{:.6}", fp.iq_amplitude_imbal));
                            fp_metric(ui, "IQ Phase Imbalance", &format!("{:.6}", fp.iq_phase_imbal));
                            fp_metric(ui, "Avg Power (dB)", &format!("{:.2}", fp.avg_power_db));
                            fp_metric(ui, "Power Variance", &format!("{:.4}", fp.power_variance));
                            fp_metric(ui, "Sample Count", &format!("{}", fp.sample_count));
                        });
                });

            ui.add_space(8.0);

            // Action buttons
            ui.horizontal(|ui| {
                // Save to DB
                let save_btn = egui::Button::new(
                    egui::RichText::new("SAVE TO DB").color(Color32::WHITE).size(FONT_SIZE_DATA)
                ).fill(Color32::from_rgb(40, 120, 40));
                if ui.add(save_btn).clicked() {
                    match state.db().upsert_radio_fingerprint(
                        fp.cfo_hz,
                        fp.iq_amplitude_imbal,
                        fp.iq_phase_imbal,
                        fp.avg_power_db,
                        fp.power_variance,
                        fp.sample_count as i32,
                        cs.iq_viewer_freq,
                        50.0,  // cfo_bucket_hz
                        0.001, // iq_resolution
                    ) {
                        Ok((id, count)) => {
                            cs.iq_viewer_fp_status = Some(
                                format!("Saved: id={}, captures={}", id, count)
                            );
                        }
                        Err(e) => {
                            cs.iq_viewer_fp_status = Some(format!("Error: {}", e));
                        }
                    }
                }

                // Compare with DB
                let compare_btn = egui::Button::new(
                    egui::RichText::new("COMPARE").color(Color32::WHITE).size(FONT_SIZE_DATA)
                ).fill(CYAN_P25);
                if ui.add(compare_btn).clicked() {
                    match state.db().match_fingerprint_typed(
                        fp.cfo_hz,
                        fp.iq_amplitude_imbal,
                        200.0, // tolerance
                    ) {
                        Ok(matches) => {
                            cs.iq_viewer_fp_matches = matches;
                        }
                        Err(e) => {
                            cs.iq_viewer_fp_status = Some(format!("Match error: {}", e));
                        }
                    }
                }

                // Re-extract (clear cache)
                if ui.button(egui::RichText::new("RE-EXTRACT").color(TEXT_SECONDARY).size(FONT_SIZE_DATA)).clicked() {
                    cs.iq_viewer_fingerprint = None;
                }
            });

            // Status message
            if let Some(ref status) = cs.iq_viewer_fp_status {
                ui.add_space(4.0);
                let color = if status.starts_with("Error") { RED_WATCHDOG } else { GREEN_COLLECT };
                ui.label(egui::RichText::new(status).color(color).size(FONT_SIZE_DATA));
            }

            // Match results
            if !cs.iq_viewer_fp_matches.is_empty() {
                ui.add_space(8.0);
                rec_section_header(ui, &format!("MATCHES ({})", cs.iq_viewer_fp_matches.len()));

                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(4))
                    .fill(BG_SURFACE)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        egui::Grid::new("fp_matches")
                            .num_columns(7)
                            .spacing([8.0, 2.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for h in ["FP ID", "CFO", "IQ IMBAL", "CAPTURES", "FREQ", "FIRST SEEN", "LAST SEEN"] {
                                    ui.label(egui::RichText::new(h)
                                        .color(TEXT_SECONDARY).size(FONT_SIZE_HUD).strong());
                                }
                                ui.end_row();

                                for m in &cs.iq_viewer_fp_matches {
                                    ui.label(egui::RichText::new(&m.fingerprint_id)
                                        .color(CYAN_P25).size(FONT_SIZE_DATA)
                                        .family(egui::FontFamily::Monospace));
                                    ui.label(egui::RichText::new(
                                        m.freq_offset_hz.map_or("—".to_string(), |v| format!("{:.1}", v))
                                    ).color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                        .family(egui::FontFamily::Monospace));
                                    ui.label(egui::RichText::new(
                                        m.iq_imbalance.map_or("—".to_string(), |v| format!("{:.6}", v))
                                    ).color(TEXT_PRIMARY).size(FONT_SIZE_DATA)
                                        .family(egui::FontFamily::Monospace));
                                    ui.label(egui::RichText::new(format!("{}", m.capture_count))
                                        .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                                    ui.label(egui::RichText::new(
                                        m.freq_mhz.map_or("—".to_string(), |v| format!("{:.4}", v))
                                    ).color(GREEN_COLLECT).size(FONT_SIZE_DATA)
                                        .family(egui::FontFamily::Monospace));
                                    let first = m.first_seen.get(..16).unwrap_or(&m.first_seen);
                                    ui.label(egui::RichText::new(first)
                                        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                                    let last = m.last_seen.get(..16).unwrap_or(&m.last_seen);
                                    ui.label(egui::RichText::new(last)
                                        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
                                    ui.end_row();
                                }
                            });
                    });
            }
        }
        None => {
            ui.label(egui::RichText::new("Insufficient samples for fingerprint extraction (need >= 480).")
                .color(AMBER_WARNING).size(FONT_SIZE_DATA));
        }
    }
}

fn fp_metric(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label)
        .color(TEXT_SECONDARY).size(FONT_SIZE_DATA));
    ui.label(egui::RichText::new(value)
        .color(GREEN_COLLECT).size(FONT_SIZE_DATA).strong()
        .family(egui::FontFamily::Monospace));
    ui.end_row();
}

// ── Formatting Helpers ──────────────────────────────────

fn format_duration(secs: f64) -> String {
    if secs >= 3600.0 {
        format!("{:.0}h{:.0}m", secs / 3600.0, (secs % 3600.0) / 60.0)
    } else if secs >= 60.0 {
        format!("{:.0}m{:.0}s", secs / 60.0, secs % 60.0)
    } else {
        format!("{:.1}s", secs)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate_str(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

// ── Weather / SAME Alerts ────────────────────────────────

fn show_wx_alert_banner(ui: &mut egui::Ui, cs: &CollectState) {
    if cs.wx_alerts.is_empty() {
        return;
    }

    let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let active: Vec<_> = cs.wx_alerts.iter().filter(|a| {
        a.expires_at.as_ref().map_or(true, |exp| exp.as_str() >= now_str.as_str())
    }).collect();

    if !active.is_empty() {
        // Active alert banner
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(6))
            .fill(egui::Color32::from_rgba_unmultiplied(180, 60, 0, 40))
            .corner_radius(4.0)
            .stroke(egui::Stroke::new(1.0, AMBER_WARNING))
            .show(ui, |ui| {
                for alert in &active {
                    ui.horizontal(|ui| {
                        let severity_color = wx_severity_color(&alert.severity);
                        ui.label(egui::RichText::new("\u{26A0}")
                            .color(severity_color).size(FONT_SIZE_LARGE));
                        ui.label(egui::RichText::new(&alert.event_name)
                            .color(severity_color).size(FONT_SIZE_DATA).strong());
                        ui.label(egui::RichText::new(&alert.locations)
                            .color(TEXT_PRIMARY).size(FONT_SIZE_DATA));
                        if let Some(dur) = alert.duration_mins {
                            ui.label(egui::RichText::new(format!("{}min", dur))
                                .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                        }
                        let time = alert.received_at.get(..16).unwrap_or(&alert.received_at);
                        ui.label(egui::RichText::new(time)
                            .color(TEXT_SECONDARY).size(FONT_SIZE_HUD));
                    });
                }
            });
        ui.add_space(4.0);
    }

    // Full alert history
    section_header(ui, &format!("WEATHER ALERTS ({})", cs.wx_alerts.len()), AMBER_WARNING);

    let alerts = cs.wx_alerts.clone();
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(4))
        .fill(BG_SURFACE)
        .corner_radius(4.0)
        .show(ui, |ui| {
            egui::Grid::new("wx_alerts")
                .num_columns(7)
                .spacing([10.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in &["Severity", "Event", "Locations", "Duration", "Station", "Received", "Expires"] {
                        ui.label(egui::RichText::new(*h).color(AMBER_WARNING).size(FONT_SIZE_HUD));
                    }
                    ui.end_row();

                    for alert in &alerts {
                        let is_active = alert.expires_at.as_ref().map_or(true, |exp| exp.as_str() >= now_str.as_str());
                        let text_color = if is_active { TEXT_PRIMARY } else { TEXT_SECONDARY };
                        let sev_color = wx_severity_color(&alert.severity);

                        ui.label(egui::RichText::new(&alert.severity)
                            .color(sev_color).size(FONT_SIZE_DATA).strong());
                        ui.label(egui::RichText::new(&alert.event_name)
                            .color(text_color).size(FONT_SIZE_DATA));
                        ui.label(egui::RichText::new(truncate_str(&alert.locations, 30))
                            .color(text_color).size(FONT_SIZE_DATA));
                        ui.label(egui::RichText::new(
                            alert.duration_mins.map_or("-".to_string(), |d| format!("{}m", d))
                        ).color(text_color).size(FONT_SIZE_DATA));
                        ui.label(egui::RichText::new(alert.station.as_deref().unwrap_or("-"))
                            .color(text_color).size(FONT_SIZE_DATA));
                        let recv = alert.received_at.get(..16).unwrap_or(&alert.received_at);
                        ui.label(egui::RichText::new(recv)
                            .color(text_color).size(FONT_SIZE_DATA));
                        let expires = alert.expires_at.as_deref()
                            .map(|e| e.get(..16).unwrap_or(e))
                            .unwrap_or("-");
                        ui.label(egui::RichText::new(expires)
                            .color(if is_active { AMBER_WARNING } else { TEXT_SECONDARY })
                            .size(FONT_SIZE_DATA));
                        ui.end_row();
                    }
                });
        });
    ui.add_space(8.0);
}

fn wx_severity_color(severity: &str) -> egui::Color32 {
    match severity {
        "Extreme" => RED_WATCHDOG,
        "Severe" => egui::Color32::from_rgb(255, 100, 40),
        "Moderate" => AMBER_WARNING,
        "Minor" => egui::Color32::from_rgb(200, 200, 60),
        _ => TEXT_SECONDARY,
    }
}

fn section_header(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(text).color(color).size(FONT_SIZE_HEADER).strong());
}
