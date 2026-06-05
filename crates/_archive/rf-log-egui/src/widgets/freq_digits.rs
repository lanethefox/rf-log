use eframe::egui::{self, Color32, CornerRadius, Stroke};

use crate::theme::*;

/// Tuning step sizes for each digit position (MHz values).
/// Format: XXX.XXXX MHz → digits[0]=100, [1]=10, [2]=1, [3]=0.1, [4]=0.01, [5]=0.001, [6]=0.0001
const DIGIT_STEPS: [f64; 7] = [100.0, 10.0, 1.0, 0.1, 0.01, 0.001, 0.0001];

/// Render a clickable frequency display. Each digit is a button:
/// - Left-click increments, right-click decrements
/// - Scroll wheel up/down adjusts that digit's place value
/// Returns the new frequency if changed, or None.
pub fn show(ui: &mut egui::Ui, freq_mhz: f64, accent: Color32) -> Option<f64> {
    let mut new_freq = None;

    let clamped = freq_mhz.clamp(0.0, 999.9999);
    let digits = freq_to_digits(clamped);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        for (i, &digit) in digits.iter().enumerate() {
            // Decimal point before position 3
            if i == 3 {
                ui.label(
                    egui::RichText::new(".")
                        .size(FONT_SIZE_LARGE)
                        .color(accent)
                        .family(egui::FontFamily::Monospace),
                );
            }

            let step = DIGIT_STEPS[i];

            // Each digit is a clickable button
            let resp = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{digit}"))
                        .size(FONT_SIZE_LARGE)
                        .color(accent)
                        .family(egui::FontFamily::Monospace),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::ZERO)
                .min_size(egui::vec2(14.0, 24.0)),
            );

            // Left-click = increment, right-click = decrement
            if resp.clicked() {
                new_freq = Some((clamped + step).min(999.9999));
            }
            if resp.secondary_clicked() {
                new_freq = Some((clamped - step).max(0.0));
            }

            // Scroll wheel on digit
            if resp.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll > 0.0 {
                    new_freq = Some((clamped + step).min(999.9999));
                } else if scroll < 0.0 {
                    new_freq = Some((clamped - step).max(0.0));
                }
            }

            // Underline on hover to show it's interactive
            if resp.hovered() {
                let rect = resp.rect;
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left(), rect.bottom()),
                        egui::pos2(rect.right(), rect.bottom()),
                    ],
                    Stroke::new(1.0, accent),
                );
            }
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("MHz")
                .size(FONT_SIZE_HUD)
                .color(TEXT_SECONDARY)
                .family(egui::FontFamily::Monospace),
        );
    });

    new_freq
}

/// Convert frequency in MHz to 7 individual digits: [100s, 10s, 1s, .1s, .01s, .001s, .0001s]
fn freq_to_digits(freq_mhz: f64) -> [u8; 7] {
    let val = (freq_mhz * 10000.0).round() as u64;
    [
        ((val / 1_000_000) % 10) as u8,
        ((val / 100_000) % 10) as u8,
        ((val / 10_000) % 10) as u8,
        ((val / 1_000) % 10) as u8,
        ((val / 100) % 10) as u8,
        ((val / 10) % 10) as u8,
        (val % 10) as u8,
    ]
}

/// Nudge frequency by a tuning step. Used for keyboard arrow tuning.
/// `direction`: positive = up, negative = down.
/// `large`: if true, use 1 MHz steps; if false, use channel steps (12.5 kHz).
pub fn tune_step(current_freq: f64, direction: i32, large: bool) -> f64 {
    let step = if large { 1.0 } else { 0.0125 };
    let new_freq = current_freq + step * direction as f64;
    new_freq.clamp(0.1, 999.9999)
}
