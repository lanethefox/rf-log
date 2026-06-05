use eframe::egui::{self, Color32};

use crate::bridge::UiBridge;
use crate::state::UiState;
use crate::theme::*;

use crate::views::watchdog::WatchdogState;

/// 32px top status bar — operation, SDR allocation, REC, ALERT, SWEEP, WX, GPS, UTC
pub fn show(ui: &mut egui::Ui, _ui_state: &UiState, bridge: &UiBridge, active_wx_count: i64, ws: &WatchdogState) {
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);

    // Check for active alert highlights
    let alert_active = has_active_alerts(bridge);

    // Background — tint with alert color when active
    if alert_active {
        let t = ui.ctx().input(|i| i.time);
        let pulse = ((t * 4.0).sin() * 0.5 + 0.5) as f32;
        let alert_color = get_alert_color(bridge);
        let bg = Color32::from_rgba_unmultiplied(
            (BG_SURFACE.r() as f32 + (alert_color.r() as f32 - BG_SURFACE.r() as f32) * pulse * 0.15) as u8,
            (BG_SURFACE.g() as f32 + (alert_color.g() as f32 - BG_SURFACE.g() as f32) * pulse * 0.15) as u8,
            (BG_SURFACE.b() as f32 + (alert_color.b() as f32 - BG_SURFACE.b() as f32) * pulse * 0.15) as u8,
            255,
        );
        painter.rect_filled(rect, egui::CornerRadius::ZERO, bg);
        // Top accent line in alert color
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(2.0, alert_color),
        );
        ui.ctx().request_repaint();
    } else {
        painter.rect_filled(rect, egui::CornerRadius::ZERO, BG_SURFACE);
    }

    // Bottom border
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, BORDER),
    );

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        // Operation name
        let op = bridge.hb_str("operation_name");
        status_label(ui, "OP", op, GREEN_COLLECT);

        ui.separator();

        // SDR device allocation — show each device with its role and bands
        show_sdr_allocation(ui, bridge);

        ui.separator();

        // Scanning
        let scanning = bridge.hb_bool("scanning");
        let scan_color = if scanning { GREEN_COLLECT } else { TEXT_SECONDARY };
        status_label(ui, "SCAN", if scanning { "ON" } else { "OFF" }, scan_color);

        // Active site
        let site_id = bridge.hb_u64("active_site_id");
        if site_id > 0 {
            let site_name = bridge.hb_str("active_site_name");
            if site_name != "--" && !site_name.is_empty() {
                status_label(ui, "SITE", site_name, BLUE_PLAN);
            } else {
                let id_label = format!("#{}", site_id);
                status_label(ui, "SITE", &id_label, BLUE_PLAN);
            }
            ui.separator();
        }

        // Recording
        let recording = bridge.hb_u64("recording_active_count") > 0;
        if recording {
            let t = ui.ctx().input(|i| i.time);
            let pulse = ((t * 2.0).sin() * 0.5 + 0.5) as u8;
            let rec_color = Color32::from_rgb(255, 68 + pulse * 40, 68);
            status_label(ui, "REC", "ACTIVE", rec_color);
        }

        // Sweep indicator
        if ws.sweep_active {
            ui.separator();
            let now = ui.ctx().input(|i| i.time);
            let elapsed = (now - ws.sweep_start_time).max(0.0);
            let elapsed_m = (elapsed as u32) / 60;
            let elapsed_s = (elapsed as u32) % 60;
            let total_m = (ws.sweep_duration_sec as u32) / 60;
            let total_s = (ws.sweep_duration_sec as u32) % 60;
            let sweep_label = format!("SWEEP:{}  {}:{:02}/{}:{:02}",
                ws.sweep_protocol, elapsed_m, elapsed_s, total_m, total_s);
            // Pulsing amber
            let t = ui.ctx().input(|i| i.time);
            let pulse = ((t * 2.0).sin() * 0.5 + 0.5) as u8;
            let color = Color32::from_rgb(255, 170 + pulse * 30, 0);
            status_label(ui, "", &sweep_label, color);
            ui.ctx().request_repaint();
        }

        // Alert indicator (pulsing when active)
        if alert_active {
            ui.separator();
            let t = ui.ctx().input(|i| i.time);
            let flash = (t * 3.0) as u32 % 2 == 0;
            let alert_color = get_alert_color(bridge);
            let alpha = if flash { 255u8 } else { 140 };
            let color = Color32::from_rgba_unmultiplied(
                alert_color.r(), alert_color.g(), alert_color.b(), alpha,
            );
            let alert_name = get_alert_name(bridge);
            status_label(ui, "ALERT", &alert_name.to_uppercase(), color);
        }

        // Weather alert badge
        if active_wx_count > 0 {
            ui.separator();
            let wx_label = format!("WX:{}", active_wx_count);
            status_label(ui, "", &wx_label, AMBER_WARNING);
        }

        // Right-aligned: GPS + startup phase + UTC clock
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // UTC clock
            let now = chrono::Utc::now();
            let utc = now.format("%H:%M:%S").to_string();
            ui.label(
                egui::RichText::new(format!("UTC {utc}"))
                    .size(FONT_SIZE_HUD)
                    .color(TEXT_SECONDARY)
                    .family(egui::FontFamily::Monospace),
            );

            // GPS — GPS-05: quality indicator with fix type + satellite count
            let gps_source = bridge.hb_str("gps_source");
            let gps_fix = bridge.hb_str("gps_fix");
            let gps_sats = bridge.hb_u64("gps_sats");
            let gps_hdop = bridge.hb_f64("gps_hdop");

            let (gps_label, gps_color) = if gps_source == "--" || gps_source == "none" {
                ("GPS:NONE".to_string(), TEXT_SECONDARY)
            } else if gps_fix == "none" || gps_fix == "--" {
                (format!("GPS:{}:NOFIX", gps_source.to_uppercase()), AMBER_WARNING)
            } else {
                // Have a fix — show fix type + sat count
                let quality_color = if gps_hdop > 0.0 && gps_hdop < 2.0 {
                    GREEN_COLLECT
                } else if gps_hdop > 0.0 && gps_hdop < 5.0 {
                    AMBER_WARNING
                } else if gps_sats >= 4 {
                    GREEN_COLLECT
                } else {
                    AMBER_WARNING
                };
                (format!("GPS:{}({}sat)", gps_fix.to_uppercase(), gps_sats), quality_color)
            };
            ui.label(
                egui::RichText::new(&gps_label)
                    .size(FONT_SIZE_HUD)
                    .color(gps_color)
                    .family(egui::FontFamily::Monospace),
            );

            // Startup phase (flashing amber while loading)
            let phase = bridge.hb_str("startup_phase");
            if !phase.is_empty() && phase != "--" {
                let t = ui.ctx().input(|i| i.time);
                let flash = (t * 3.0) as u32 % 2 == 0;
                let alpha = if flash { 255u8 } else { 120 };
                let color = Color32::from_rgba_unmultiplied(
                    AMBER_WARNING.r(), AMBER_WARNING.g(), AMBER_WARNING.b(), alpha,
                );
                ui.label(
                    egui::RichText::new(phase.to_uppercase())
                        .size(FONT_SIZE_HUD)
                        .color(color)
                        .family(egui::FontFamily::Monospace),
                );
                ui.ctx().request_repaint();
            }
        });
    });
}

/// Show SDR device allocation: each device with role color + band assignments
fn show_sdr_allocation(ui: &mut egui::Ui, bridge: &UiBridge) {
    let slots = bridge.heartbeat.as_ref()
        .and_then(|hb| hb.get("sdr_slots"))
        .and_then(|v| v.as_array());

    if let Some(slots) = slots {
        if slots.is_empty() {
            // No devices — simulation mode
            ui.label(
                egui::RichText::new("SDR:SIM")
                    .size(FONT_SIZE_HUD)
                    .color(AMBER_WARNING)
                    .family(egui::FontFamily::Monospace),
            );
            return;
        }

        for slot in slots {
            let name = slot.get("user_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| slot.get("label").and_then(|v| v.as_str()))
                .unwrap_or("SDR");
            let role = slot.get("role").and_then(|v| v.as_str()).unwrap_or("idle");
            let alive = slot.get("alive").and_then(|v| v.as_bool()).unwrap_or(false);
            let quarantined = slot.get("quarantined").and_then(|v| v.as_bool()).unwrap_or(false);
            let bands: Vec<&str> = slot.get("assigned_bands")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            // Color by status
            let color = if quarantined {
                RED_WATCHDOG
            } else if !alive {
                TEXT_SECONDARY
            } else {
                match role {
                    "scan" => GREEN_COLLECT,
                    "monitor" => CYAN_P25,
                    "cc" | "control" => MAGENTA_EXPLOIT,
                    "voice" => CYAN_P25,
                    _ => TEXT_SECONDARY,
                }
            };

            // Compact: "NAME:ROLE[BANDS]"
            let role_short = match role {
                "scan" => "SCN",
                "monitor" => "MON",
                "cc" | "control" => "CC",
                "voice" => "VOX",
                "idle" => "IDL",
                "failed" => "FAIL",
                _ => role,
            };

            // Short device name (truncate to ~8 chars)
            let short_name = if name.len() > 8 { &name[..8] } else { name };

            let bands_str = if bands.is_empty() {
                String::new()
            } else {
                format!("[{}]", bands.join(","))
            };

            ui.label(
                egui::RichText::new(format!("{short_name}:{role_short}{bands_str}"))
                    .size(FONT_SIZE_HUD)
                    .color(color)
                    .family(egui::FontFamily::Monospace),
            );
        }
    } else {
        // No heartbeat yet or no sdr_slots field
        let sdr_alive = bridge.hb_bool("sdr_ok");
        let sdr_label = if sdr_alive { "ONLINE" } else { "SIM" };
        let sdr_color = if sdr_alive { GREEN_COLLECT } else { AMBER_WARNING };
        status_label(ui, "SDR", sdr_label, sdr_color);
    }
}

fn status_label(ui: &mut egui::Ui, prefix: &str, value: &str, color: Color32) {
    ui.label(
        egui::RichText::new(format!("{prefix}:"))
            .size(FONT_SIZE_HUD)
            .color(TEXT_SECONDARY)
            .family(egui::FontFamily::Monospace),
    );
    ui.label(
        egui::RichText::new(value.to_uppercase())
            .size(FONT_SIZE_HUD)
            .color(color)
            .family(egui::FontFamily::Monospace),
    );
}

/// Check if there are active alert highlights in the heartbeat.
fn has_active_alerts(bridge: &UiBridge) -> bool {
    bridge.heartbeat.as_ref()
        .and_then(|hb| hb.get("alert_highlights"))
        .and_then(|v| v.as_array())
        .is_some_and(|arr| !arr.is_empty())
}

/// Get the highest-priority alert color, or default to RED_WATCHDOG.
fn get_alert_color(bridge: &UiBridge) -> Color32 {
    let color_str = bridge.heartbeat.as_ref()
        .and_then(|hb| hb.get("alert_highlights"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|h| h.get("color"))
        .and_then(|c| c.as_str())
        .unwrap_or("#FF3333");

    parse_color(color_str)
}

/// Get the name of the most recent alert highlight.
fn get_alert_name(bridge: &UiBridge) -> String {
    bridge.heartbeat.as_ref()
        .and_then(|hb| hb.get("alert_highlights"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|h| h.get("rule_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("ACTIVE")
        .to_string()
}

/// Parse a CSS-style hex color string to Color32.
fn parse_color(s: &str) -> Color32 {
    if s.starts_with('#') && s.len() == 7 {
        let r = u8::from_str_radix(&s[1..3], 16).unwrap_or(255);
        let g = u8::from_str_radix(&s[3..5], 16).unwrap_or(51);
        let b = u8::from_str_radix(&s[5..7], 16).unwrap_or(51);
        Color32::from_rgb(r, g, b)
    } else {
        match s.to_lowercase().as_str() {
            "red" => RED_WATCHDOG,
            "amber" | "orange" | "yellow" => AMBER_WARNING,
            "green" => GREEN_COLLECT,
            "blue" | "cyan" => CYAN_P25,
            "magenta" | "pink" => MAGENTA_EXPLOIT,
            _ => RED_WATCHDOG,
        }
    }
}
