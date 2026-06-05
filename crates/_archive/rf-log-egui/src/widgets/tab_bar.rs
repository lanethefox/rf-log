use eframe::egui::{self, Color32, CornerRadius, Stroke};

use crate::theme::*;

/// Reusable tab bar widget. Returns true if the tab changed.
pub fn show<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    tabs: &[T],
    active: &mut T,
    label_fn: impl Fn(&T) -> &str,
    accent: Color32,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        for tab in tabs {
            let is_active = *active == *tab;
            let label = label_fn(tab);
            let text_color = if is_active { accent } else { TEXT_SECONDARY };

            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(FONT_SIZE_HEADER)
                    .color(text_color)
                    .family(egui::FontFamily::Monospace),
            )
            .fill(if is_active { BG_ELEVATED } else { BG_SURFACE })
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::ZERO)
            .min_size(egui::vec2(0.0, 28.0));

            let response = ui.add(btn);

            // Active underline
            if is_active {
                let rect = response.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left(), rect.bottom() - 2.0),
                        egui::vec2(rect.width(), 2.0),
                    ),
                    CornerRadius::ZERO,
                    accent,
                );
            }

            if response.clicked() && !is_active {
                *active = *tab;
                changed = true;
            }

            ui.add_space(2.0);
        }
    });

    changed
}
