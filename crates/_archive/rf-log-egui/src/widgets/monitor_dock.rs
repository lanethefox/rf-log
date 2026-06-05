use eframe::egui::{self, Color32, CornerRadius, Stroke};

use crate::bridge::UiBridge;
use crate::state::UiState;
use crate::theme::*;

/// Bottom monitor/demod control bar with "who's talking" display
pub fn show(ui: &mut egui::Ui, ui_state: &mut UiState, bridge: &UiBridge, state: &rf_web::AppState) {
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, CornerRadius::ZERO, BG_SURFACE);
    // Top border
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, BORDER),
    );

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;

        // --- Who's Talking bar ---
        show_whos_talking(ui, bridge);

        // --- Controls row ---
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Frequency digit display with clickable up/down arrows
            let freq = bridge.hb_f64("monitor_freq");
            if let Some(new_freq) = crate::widgets::freq_digits::show(ui, freq, GREEN_COLLECT) {
                crate::commands::monitor_signal(state, new_freq);
            }

            ui.separator();

            // Scan/Stop toggle
            let scanning = bridge.hb_bool("scanning");
            let scan_label = if scanning { "STOP" } else { "SCAN" };
            let scan_color = if scanning { RED_WATCHDOG } else { GREEN_COLLECT };
            let scan_btn = egui::Button::new(
                egui::RichText::new(scan_label)
                    .size(FONT_SIZE_DATA)
                    .color(scan_color)
                    .family(egui::FontFamily::Monospace),
            )
            .fill(BG_ELEVATED)
            .stroke(Stroke::new(1.0, scan_color))
            .corner_radius(CornerRadius::ZERO)
            .min_size(egui::vec2(44.0, 28.0));

            if ui.add(scan_btn).clicked() {
                crate::commands::set_scanning(state, !scanning);
            }

            ui.separator();

            // Demod mode buttons
            let mode = bridge.hb_str("modulation").to_uppercase();
            for m in &["NFM", "WFM", "AM", "USB", "LSB", "P25"] {
                let is_active = mode == *m;
                let btn_color = if is_active { GREEN_COLLECT } else { TEXT_SECONDARY };
                let btn = egui::Button::new(
                    egui::RichText::new(*m)
                        .size(FONT_SIZE_DATA)
                        .color(btn_color)
                        .family(egui::FontFamily::Monospace),
                )
                .fill(if is_active { BG_ELEVATED } else { BG_SURFACE })
                .stroke(Stroke::new(1.0, if is_active { btn_color } else { BORDER }))
                .corner_radius(CornerRadius::ZERO)
                .min_size(egui::vec2(36.0, 28.0));

                if ui.add(btn).clicked() {
                    crate::commands::set_modulation(state, m);
                }
            }

            ui.separator();

            // Squelch
            ui.label(
                egui::RichText::new("SQ:")
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_SECONDARY),
            );
            let squelch_slider = egui::Slider::new(&mut ui_state.squelch, -80.0..=0.0)
                .show_value(false)
                .custom_formatter(|v, _| format!("{:.0}", v));
            if ui.add(squelch_slider).changed() {
                crate::commands::set_squelch(state, ui_state.squelch as f64);
            }
            ui.label(
                egui::RichText::new(format!("{:.0}dB", ui_state.squelch))
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_PRIMARY),
            );

            ui.separator();

            // Mute + Volume
            let vol_icon = if ui_state.muted { "\u{1F507}" } else { "\u{1F50A}" };
            if ui.button(vol_icon).clicked() {
                ui_state.muted = !ui_state.muted;
            }
            ui.add(egui::Slider::new(&mut ui_state.volume, 0.0..=1.0).show_value(false));

            // Right side: P25 metadata + SQ OPEN
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sq_open = bridge.hb_bool("squelch_open");
                if sq_open {
                    ui.label(
                        egui::RichText::new("SQ OPEN")
                            .size(FONT_SIZE_HUD)
                            .color(GREEN_COLLECT),
                    );
                    ui.add_space(8.0);
                }

                let nac = bridge.hb_str("p25_nac");
                let tg = bridge.hb_str("p25_talkgroup");
                if nac != "--" {
                    ui.label(
                        egui::RichText::new(format!("NAC:{nac} TG:{tg}"))
                            .size(FONT_SIZE_HUD)
                            .color(CYAN_P25),
                    );
                }
            });
        });
    });
}

/// "Who's Talking" — shows active P25 voice slot info in a compact bar
fn show_whos_talking(ui: &mut egui::Ui, bridge: &UiBridge) {
    let vs_active = bridge.hb_nested("network_scan_voice_slot", "active")
        .and_then(|v| v.as_bool()).unwrap_or(false);
    let vs_tgid = bridge.hb_nested("network_scan_voice_slot", "current_tgid")
        .and_then(|v| v.as_u64()).map(|v| v as u32);
    let vs_uid = bridge.hb_nested("network_scan_voice_slot", "current_uid")
        .and_then(|v| v.as_u64()).map(|v| v as u32);
    let vs_freq = bridge.hb_nested("network_scan_voice_slot", "current_freq")
        .and_then(|v| v.as_f64());

    // Network scan CC info
    let ns_active = bridge.hb_bool("network_scan_active");
    let cc_freq = bridge.hb_f64("network_scan_cc_freq");

    // Only show if network scanner is active or voice is active
    if !ns_active && !vs_active {
        return;
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        if vs_active && vs_tgid.is_some() {
            // Active voice — pulsing green TX indicator
            let t = ui.ctx().input(|i| i.time);
            let pulse = ((t * 3.0).sin() * 0.5 + 0.5) as f32;
            let green = Color32::from_rgba_unmultiplied(
                0, (180.0 + 75.0 * pulse) as u8, (40.0 + 30.0 * pulse) as u8, 255,
            );

            ui.label(
                egui::RichText::new("\u{25CF} TX") // ● TX
                    .size(FONT_SIZE_HUD)
                    .color(green)
                    .family(egui::FontFamily::Monospace),
            );
            ui.ctx().request_repaint();

            if let Some(tgid) = vs_tgid {
                // TG name from heartbeat's talkgroup DB
                let tg_name = bridge.heartbeat.as_ref()
                    .and_then(|hb| hb.get("network_scan_voice_slot"))
                    .and_then(|vs| vs.get("tg_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let tg_dept = bridge.heartbeat.as_ref()
                    .and_then(|hb| hb.get("network_scan_voice_slot"))
                    .and_then(|vs| vs.get("tg_department"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                ui.label(
                    egui::RichText::new(format!("TG:{tgid}"))
                        .size(FONT_SIZE_DATA)
                        .color(CYAN_P25)
                        .family(egui::FontFamily::Monospace),
                );
                if !tg_name.is_empty() {
                    ui.label(
                        egui::RichText::new(tg_name)
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_PRIMARY)
                            .family(egui::FontFamily::Monospace),
                    );
                }
                if !tg_dept.is_empty() {
                    ui.label(
                        egui::RichText::new(tg_dept)
                            .size(FONT_SIZE_HUD)
                            .color(TEXT_SECONDARY)
                            .family(egui::FontFamily::Monospace),
                    );
                }
            }

            if let Some(uid) = vs_uid {
                ui.label(
                    egui::RichText::new(format!("UID:{uid}"))
                        .size(FONT_SIZE_HUD)
                        .color(Color32::from_rgb(180, 140, 60))
                        .family(egui::FontFamily::Monospace),
                );
            }

            if let Some(freq) = vs_freq {
                ui.label(
                    egui::RichText::new(format!("{:.4}MHz", freq))
                        .size(FONT_SIZE_HUD)
                        .color(TEXT_SECONDARY)
                        .family(egui::FontFamily::Monospace),
                );
            }

            // Encryption status
            let enc = bridge.hb_nested("network_scan_voice_slot", "encrypted")
                .and_then(|v| v.as_bool()).unwrap_or(false);
            if enc {
                ui.label(
                    egui::RichText::new("ENC")
                        .size(FONT_SIZE_HUD)
                        .color(RED_RECORDING)
                        .family(egui::FontFamily::Monospace),
                );
            } else {
                ui.label(
                    egui::RichText::new("CLR")
                        .size(FONT_SIZE_HUD)
                        .color(GREEN_COLLECT)
                        .family(egui::FontFamily::Monospace),
                );
            }
        } else {
            // CC active but no voice — show CC status
            ui.label(
                egui::RichText::new("\u{25CB} IDLE") // ○ IDLE
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );
            if cc_freq > 0.0 {
                ui.label(
                    egui::RichText::new(format!("CC:{:.4}", cc_freq))
                        .size(FONT_SIZE_HUD)
                        .color(TEXT_SECONDARY)
                        .family(egui::FontFamily::Monospace),
                );
            }
        }
    });
}
