use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::theme::*;

/// Band color mapping
fn band_color(band: &str) -> Color32 {
    match band {
        "AM" => Color32::from_rgb(200, 160, 0),
        "HF" => Color32::from_rgb(204, 102, 255),
        "FM" => Color32::from_rgb(255, 0, 204),
        "VHF" => Color32::from_rgb(51, 204, 51),
        "FEDV" => Color32::from_rgb(255, 51, 51),
        "BIII" => Color32::from_rgb(0, 204, 204),
        "UHF" => Color32::from_rgb(102, 153, 255),
        "GMRS" => Color32::from_rgb(204, 102, 255),
        "P25" => Color32::from_rgb(0, 204, 255),
        _ => TEXT_SECONDARY,
    }
}

/// Render a horizontal band selector bar. Returns list of currently active bands.
pub fn show(
    ui: &mut egui::Ui,
    active_bands: &[String],
    all_bands: &[&str],
) -> Option<Vec<String>> {
    let mut changed = false;
    let mut bands: Vec<String> = active_bands.to_vec();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;

        ui.label(
            RichText::new("BANDS")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY),
        );

        ui.add_space(4.0);

        for &band_key in all_bands {
            let is_active = bands.iter().any(|b| b == band_key);
            let color = band_color(band_key);
            let text_color = if is_active { color } else { TEXT_SECONDARY };
            let bg = if is_active { BG_ELEVATED } else { BG_SURFACE };
            let stroke_color = if is_active { color } else { BORDER };

            let btn = egui::Button::new(
                RichText::new(band_key)
                    .size(FONT_SIZE_HUD)
                    .color(text_color)
                    .family(egui::FontFamily::Monospace),
            )
            .fill(bg)
            .stroke(Stroke::new(1.0, stroke_color))
            .corner_radius(CornerRadius::ZERO)
            .min_size(egui::vec2(32.0, 20.0));

            if ui.add(btn).clicked() {
                if is_active {
                    bands.retain(|b| b != band_key);
                } else {
                    bands.push(band_key.to_string());
                }
                changed = true;
            }
        }
    });

    if changed { Some(bands) } else { None }
}
