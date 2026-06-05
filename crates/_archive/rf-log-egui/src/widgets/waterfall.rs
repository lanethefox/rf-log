use std::collections::VecDeque;
use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

use crate::theme::*;

/// GPU-backed waterfall display using texture updates
pub struct WaterfallState {
    texture: Option<TextureHandle>,
    /// Ring buffer of pixel rows (newest at front)
    history: VecDeque<Vec<Color32>>,
    width: usize,
    max_rows: usize,
    /// Track if we need a full texture rebuild
    dirty: bool,
}

impl WaterfallState {
    pub fn new(max_rows: usize) -> Self {
        Self {
            texture: None,
            history: VecDeque::new(),
            width: 0,
            max_rows,
            dirty: true,
        }
    }

    /// Push a new row of power data. Call once per spectrum frame.
    pub fn push_row(&mut self, powers: &[f64]) {
        let w = powers.len();
        if w == 0 {
            return;
        }

        // If width changed, reset
        if w != self.width {
            self.width = w;
            self.history.clear();
            self.texture = None;
        }

        let row: Vec<Color32> = powers.iter().map(|&p| power_to_color(p)).collect();
        self.history.push_front(row);
        while self.history.len() > self.max_rows {
            self.history.pop_back();
        }
        self.dirty = true;
    }

    /// Render the waterfall into the UI.
    /// If freq_range is provided, clicking returns the clicked frequency in MHz.
    #[allow(dead_code)] // Convenience wrapper; show_with_freq used directly
    pub fn show(&mut self, ui: &mut egui::Ui, height: f32) -> Option<f64> {
        self.show_with_freq(ui, height, None)
    }

    /// Render with optional frequency range for click-to-tune.
    pub fn show_with_freq(&mut self, ui: &mut egui::Ui, height: f32, freq_range: Option<(f64, f64)>) -> Option<f64> {
        let available_width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(available_width, height),
            egui::Sense::click(),
        );

        if self.history.is_empty() || self.width == 0 {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, egui::CornerRadius::ZERO, BG_PRIMARY);
            return None;
        }

        // Rebuild texture when dirty
        if self.dirty {
            let h = self.history.len();
            let w = self.width;
            let mut pixels = Vec::with_capacity(w * h);
            for row in &self.history {
                if row.len() == w {
                    pixels.extend_from_slice(row);
                } else {
                    // Pad/truncate to width
                    for i in 0..w {
                        pixels.push(row.get(i).copied().unwrap_or(Color32::BLACK));
                    }
                }
            }

            let image = ColorImage::from_rgba_premultiplied(
                [w, h],
                &pixels
                    .iter()
                    .flat_map(|c| c.to_array())
                    .collect::<Vec<u8>>(),
            );

            match &mut self.texture {
                Some(tex) => tex.set(image, TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ui.ctx().load_texture(
                        "waterfall",
                        image,
                        TextureOptions::NEAREST,
                    ));
                }
            }
            self.dirty = false;
        }

        if let Some(tex) = &self.texture {
            // Draw the texture stretched to fill the rect
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
        }

        // Click-to-tune: convert X position to frequency
        if response.clicked() {
            if let Some((fmin, fmax)) = freq_range {
                if let Some(pos) = response.interact_pointer_pos() {
                    let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                    let freq = fmin + t * (fmax - fmin);
                    return Some(freq);
                }
            }
        }

        None
    }
}

/// Map power (dBFS) to waterfall color
/// Range: -120 dBFS (dark blue) to -20 dBFS (bright red)
fn power_to_color(power_db: f64) -> Color32 {
    // Normalize to 0.0..1.0
    let t = ((power_db + 120.0) / 100.0).clamp(0.0, 1.0) as f32;

    // 4-segment gradient: dark blue -> cyan -> green -> yellow -> red
    if t < 0.25 {
        let s = t / 0.25;
        Color32::from_rgb(
            0,
            (s * 80.0) as u8,
            (40.0 + s * 120.0) as u8,
        )
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        Color32::from_rgb(
            0,
            (80.0 + s * 175.0) as u8,
            (160.0 - s * 60.0) as u8,
        )
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        Color32::from_rgb(
            (s * 255.0) as u8,
            (255.0 - s * 55.0) as u8,
            0,
        )
    } else {
        let s = (t - 0.75) / 0.25;
        Color32::from_rgb(
            255,
            (200.0 - s * 200.0) as u8,
            0,
        )
    }
}
