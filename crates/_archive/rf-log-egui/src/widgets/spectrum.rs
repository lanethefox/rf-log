use eframe::egui::{self, Color32, Vec2b};
use egui_plot::{HLine, Line, PlotBounds, PlotMemory, PlotPoints, Points, Plot};

use crate::bridge::SpectrumFrame;
use crate::theme::*;

/// Classification color mapping
fn cls_color(cls: &str) -> Color32 {
    match cls {
        "PUBS" => Color32::from_rgb(51, 204, 51),
        "AMAT" => Color32::from_rgb(102, 153, 255),
        "MARN" => Color32::from_rgb(0, 204, 204),
        "WX" => Color32::from_rgb(255, 153, 51),
        "GMRS" => Color32::from_rgb(204, 102, 255),
        "COMM" => Color32::from_rgb(212, 212, 212),
        "FEDL" => Color32::from_rgb(255, 51, 51),
        "BCST" => Color32::from_rgb(200, 160, 0),
        _ => Color32::from_rgb(88, 88, 88), // UNK
    }
}

/// Render a spectrum plot for a single band using egui_plot.
/// Returns the clicked frequency (MHz) if the user clicked on the plot.
///
/// X axis is locked (no auto-fit) so it doesn't jump as data updates.
/// User can drag to pan X, scroll to zoom X, double-click to reset to full band.
/// Clicking centers the view on the clicked frequency and returns it for tune.
pub fn show(
    ui: &mut egui::Ui,
    band: &str,
    frame: &SpectrumFrame,
    threshold: f64,
    height: f32,
) -> Option<f64> {
    let accent = GREEN_COLLECT;

    let (fmin, fmax) = match (frame.freqs.first(), frame.freqs.last()) {
        (Some(&lo), Some(&hi)) if hi > lo => (lo, hi),
        _ => return None,
    };

    let plot_id_str = format!("spectrum_{band}");

    // Compute the persistent plot id (must match egui_plot's internal id computation)
    let plot_id = ui.make_persistent_id(egui::Id::new(&plot_id_str));

    // After the first frame with data, lock X axis so it doesn't auto-fit every frame.
    // Y axis stays auto-fit so power range adjusts to data.
    // Double-click resets auto_bounds to true (re-fits to data).
    if let Some(mut mem) = PlotMemory::load(ui.ctx(), plot_id) {
        if mem.auto_bounds.x && mem.bounds().is_valid_x() {
            mem.auto_bounds.x = false;
            mem.store(ui.ctx(), plot_id);
        }
    }

    let plot_resp = Plot::new(&plot_id_str)
        .height(height)
        .allow_zoom(Vec2b::new(true, false))   // Ctrl+scroll / pinch zoom X
        .allow_drag(Vec2b::new(true, false))    // drag to pan X
        .allow_scroll(Vec2b::new(false, false)) // disable scroll-to-pan (we use scroll for zoom)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(true)
        .show_axes([true, true])
        .show_grid(true)
        .x_axis_label("MHz")
        .y_axis_label("dBFS")
        .x_axis_formatter(|mark, _range| format!("{:.2}", mark.value))
        .y_axis_formatter(|mark, _range| format!("{:.0}", mark.value))
        .show(ui, |plot_ui| {
            // Spectrum trace
            let trace_points: PlotPoints = frame
                .freqs
                .iter()
                .zip(&frame.powers)
                .map(|(&f, &p)| [f, p])
                .collect();
            plot_ui.line(
                Line::new(format!("{band} spectrum"), trace_points)
                    .color(accent)
                    .width(1.5)
                    .fill(-120.0),
            );

            // Noise floor line
            plot_ui.hline(
                HLine::new("Noise Floor", frame.noise_floor)
                    .color(AMBER_WARNING)
                    .width(1.0)
                    .style(egui_plot::LineStyle::dashed_dense()),
            );

            // Threshold line
            plot_ui.hline(
                HLine::new("Threshold", threshold)
                    .color(Color32::from_rgb(180, 180, 180))
                    .width(1.0)
                    .style(egui_plot::LineStyle::dashed_loose()),
            );

            // Signal markers
            if !frame.signals.is_empty() {
                let mut by_cls: std::collections::HashMap<String, Vec<[f64; 2]>> =
                    std::collections::HashMap::new();
                for sig in &frame.signals {
                    by_cls
                        .entry(sig.classification.clone())
                        .or_default()
                        .push([sig.freq_mhz, sig.power_db]);
                }
                for (cls, pts) in by_cls {
                    let color = cls_color(&cls);
                    let plot_pts: PlotPoints = pts.into_iter().collect();
                    plot_ui.points(
                        Points::new(cls, plot_pts)
                            .color(color)
                            .radius(4.0)
                            .filled(true),
                    );
                }
            }
        });

    // Scroll-to-zoom: plain mouse wheel zooms X axis (centered on cursor)
    if plot_resp.response.contains_pointer() {
        let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_y != 0.0 {
            if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                if let Some(mut mem) = PlotMemory::load(ui.ctx(), plot_id) {
                    let mut transform = mem.transform();
                    let zoom_factor = (scroll_y * 0.005).exp();
                    transform.zoom(egui::vec2(zoom_factor, 1.0), hover_pos);
                    mem.set_transform(transform);
                    mem.auto_bounds.x = false;
                    mem.store(ui.ctx(), plot_id);
                }
            }
        }
    }

    // Detect click → center view on clicked frequency + return for tune
    if plot_resp.response.clicked() {
        if let Some(pos) = plot_resp.response.interact_pointer_pos() {
            let plot_point = plot_resp.transform.value_from_position(pos);
            let freq_mhz = plot_point.x;
            if freq_mhz >= fmin && freq_mhz <= fmax {
                // Center the X axis on the clicked frequency (keep current zoom span)
                if let Some(mut mem) = PlotMemory::load(ui.ctx(), plot_id) {
                    let current_bounds = *mem.bounds();
                    let span = current_bounds.width();
                    let new_bounds = PlotBounds::from_min_max(
                        [freq_mhz - span / 2.0, current_bounds.min()[1]],
                        [freq_mhz + span / 2.0, current_bounds.max()[1]],
                    );
                    mem.set_bounds(new_bounds);
                    mem.auto_bounds.x = false;
                    mem.store(ui.ctx(), plot_id);
                }
                return Some(freq_mhz);
            }
        }
    }

    None
}

/// Show a "no data" placeholder for a band
pub fn show_empty(ui: &mut egui::Ui, band: &str, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::ZERO, BG_PRIMARY);

    let text = format!("{band} — AWAITING DATA");
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::new(FONT_SIZE_HEADER, egui::FontFamily::Monospace),
        TEXT_SECONDARY,
    );
    let pos = rect.center() - galley.size() / 2.0;
    painter.galley(pos, galley, TEXT_SECONDARY);
}
