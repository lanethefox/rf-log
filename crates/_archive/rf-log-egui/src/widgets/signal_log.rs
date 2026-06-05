use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};

use crate::theme::*;

/// Classification color mapping (matches React frontend)
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
        _ => Color32::from_rgb(88, 88, 88),
    }
}

/// Power-based color: stronger signals = warmer colors
fn power_color(db: f64) -> Color32 {
    if db > -50.0 {
        Color32::from_rgb(255, 68, 68) // strong = red
    } else if db > -65.0 {
        Color32::from_rgb(255, 170, 0) // medium = orange
    } else if db > -80.0 {
        Color32::from_rgb(255, 255, 100) // weak = yellow
    } else {
        TEXT_SECONDARY // very weak = grey
    }
}

/// Aggregated signal for display (collected across all bands)
#[derive(Clone, Debug)]
pub struct SignalRow {
    pub freq_mhz: f64,
    pub power_db: f64,
    pub classification: String,
    pub name: String,
    pub mode: String,
    #[allow(dead_code)] // Available for future band-based filtering
    pub band: String,
}

/// Render the signal log table. Returns the frequency of a newly clicked signal.
pub fn show(
    ui: &mut egui::Ui,
    signals: &[SignalRow],
    selected_idx: &mut Option<usize>,
) -> Option<f64> {
    let mut clicked_freq = None;
    if signals.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("No signals detected")
                    .size(FONT_SIZE_DATA)
                    .color(TEXT_SECONDARY),
            );
        });
        return None;
    }

    let row_height = 18.0;
    let available = ui.available_height();
    let max_rows = ((available / row_height) as usize).max(5);

    TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(80.0).clip(true))  // FREQ
        .column(Column::auto().at_least(50.0))              // PWR
        .column(Column::auto().at_least(45.0))              // CLS
        .column(Column::remainder().at_least(100.0).clip(true)) // NAME
        .column(Column::auto().at_least(35.0))              // MODE
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.label(RichText::new("FREQ").size(FONT_SIZE_HUD).color(GREEN_COLLECT));
            });
            header.col(|ui| {
                ui.label(RichText::new("PWR").size(FONT_SIZE_HUD).color(GREEN_COLLECT));
            });
            header.col(|ui| {
                ui.label(RichText::new("CLS").size(FONT_SIZE_HUD).color(GREEN_COLLECT));
            });
            header.col(|ui| {
                ui.label(RichText::new("NAME").size(FONT_SIZE_HUD).color(GREEN_COLLECT));
            });
            header.col(|ui| {
                ui.label(RichText::new("MODE").size(FONT_SIZE_HUD).color(GREEN_COLLECT));
            });
        })
        .body(|body| {
            let display_count = signals.len().min(max_rows);
            body.rows(row_height, display_count, |mut row| {
                let idx = row.index();
                let sig = &signals[idx];
                let is_selected = *selected_idx == Some(idx);

                row.col(|ui| {
                    let text = format!("{:.4}", sig.freq_mhz);
                    let label = RichText::new(text)
                        .size(FONT_SIZE_DATA)
                        .color(if is_selected { GREEN_COLLECT } else { Color32::from_rgb(255, 255, 100) })
                        .family(egui::FontFamily::Monospace);
                    if ui.selectable_label(is_selected, label).clicked() {
                        *selected_idx = Some(idx);
                        clicked_freq = Some(sig.freq_mhz);
                    }
                });
                row.col(|ui| {
                    let text = format!("{:.0}", sig.power_db);
                    ui.label(
                        RichText::new(text)
                            .size(FONT_SIZE_DATA)
                            .color(power_color(sig.power_db)),
                    );
                });
                row.col(|ui| {
                    let color = cls_color(&sig.classification);
                    ui.label(
                        RichText::new(&sig.classification)
                            .size(FONT_SIZE_HUD)
                            .color(color),
                    );
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(&sig.name)
                            .size(FONT_SIZE_DATA)
                            .color(TEXT_SECONDARY),
                    );
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(&sig.mode)
                            .size(FONT_SIZE_HUD)
                            .color(TEXT_SECONDARY),
                    );
                });
            });
        });

    clicked_freq
}
