use eframe::egui::{self, Color32, CornerRadius, FontDefinitions, FontFamily, FontId, Stroke, Style, TextStyle, Visuals};

// --- Background ---
pub const BG_PRIMARY: Color32 = Color32::from_rgb(10, 10, 15);
pub const BG_SURFACE: Color32 = Color32::from_rgb(18, 20, 28);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(22, 26, 38);
pub const BORDER: Color32 = Color32::from_rgb(26, 30, 46);
pub const BORDER_BRIGHT: Color32 = Color32::from_rgb(60, 70, 100);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(224, 232, 240);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(90, 106, 122);

// --- Accent neon ---
pub const GREEN_COLLECT: Color32 = Color32::from_rgb(0, 255, 102);
pub const MAGENTA_EXPLOIT: Color32 = Color32::from_rgb(255, 0, 204);
pub const BLUE_PLAN: Color32 = Color32::from_rgb(68, 136, 255);
pub const RED_WATCHDOG: Color32 = Color32::from_rgb(255, 51, 51);
pub const AMBER_WARNING: Color32 = Color32::from_rgb(255, 170, 0);
pub const CYAN_P25: Color32 = Color32::from_rgb(0, 204, 255);
pub const RED_RECORDING: Color32 = Color32::from_rgb(255, 68, 68);

// --- Layout constants ---
pub const STATUS_HUD_HEIGHT: f32 = 32.0;
pub const NAV_RAIL_WIDTH: f32 = 56.0;
pub const NAV_BUTTON_SIZE: f32 = 56.0;

// --- Font sizes ---
pub const FONT_SIZE_HUD: f32 = 10.0;
pub const FONT_SIZE_DATA: f32 = 12.0;
pub const FONT_SIZE_HEADER: f32 = 11.0;
pub const FONT_SIZE_LARGE: f32 = 16.0;
pub const FONT_SIZE_FREQ: f32 = 20.0;

/// Bundled JetBrains Mono font bytes
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/JetBrainsMono-Regular.ttf");
const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/JetBrainsMono-Bold.ttf");

/// Get accent color for a workflow view
pub fn workflow_color(workflow: &crate::state::Workflow) -> Color32 {
    match workflow {
        crate::state::Workflow::Collect => GREEN_COLLECT,
        crate::state::Workflow::Exploit => MAGENTA_EXPLOIT,
        crate::state::Workflow::Plan => BLUE_PLAN,
        crate::state::Workflow::Watchdog => RED_WATCHDOG,
    }
}

/// Red border color for invalid/empty required form fields
pub const RED_INVALID: Color32 = Color32::from_rgb(255, 60, 60);

/// Add a required text input field. Shows red border when the field is empty and has been touched.
/// Returns the response from the text edit widget.
pub fn required_text_field(
    ui: &mut egui::Ui,
    value: &mut String,
    width: f32,
    hint: &str,
) -> egui::Response {
    let is_empty = value.trim().is_empty();
    let te = egui::TextEdit::singleline(value)
        .desired_width(width)
        .font(egui::TextStyle::Monospace)
        .hint_text(hint);

    if is_empty {
        // Temporarily override the stroke for this widget
        let resp = ui.scope(|ui| {
            ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, RED_INVALID);
            ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, RED_INVALID);
            ui.add(te)
        });
        resp.inner
    } else {
        ui.add(te)
    }
}

pub fn setup_tactical_theme(ctx: &egui::Context) {
    // --- Fonts ---
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "JetBrainsMono".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(JETBRAINS_MONO)),
    );
    fonts.font_data.insert(
        "JetBrainsMono-Bold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(JETBRAINS_MONO_BOLD)),
    );
    // Set monospace as the primary font
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "JetBrainsMono".to_owned());
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "JetBrainsMono".to_owned());

    ctx.set_fonts(fonts);

    // --- Text styles ---
    let mut style = Style::default();
    style.text_styles = [
        (TextStyle::Small, FontId::new(FONT_SIZE_HUD, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(FONT_SIZE_DATA, FontFamily::Monospace)),
        (TextStyle::Heading, FontId::new(FONT_SIZE_LARGE, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(FONT_SIZE_DATA, FontFamily::Monospace)),
        (TextStyle::Monospace, FontId::new(FONT_SIZE_DATA, FontFamily::Monospace)),
    ]
    .into();

    // --- Visuals ---
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG_PRIMARY;
    visuals.window_fill = BG_SURFACE;
    visuals.extreme_bg_color = BG_PRIMARY;
    visuals.faint_bg_color = BG_SURFACE;
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.window_stroke = Stroke::new(1.0, BORDER);

    // Widget styles
    visuals.widgets.noninteractive.bg_fill = BG_SURFACE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);

    visuals.widgets.inactive.bg_fill = BG_ELEVATED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(30, 35, 50);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, TEXT_SECONDARY);

    visuals.widgets.active.bg_fill = Color32::from_rgb(35, 40, 55);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::ZERO;

    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(0, 255, 102, 30);
    visuals.selection.stroke = Stroke::new(1.0, GREEN_COLLECT);

    style.visuals = visuals;

    // Spacing
    style.spacing.item_spacing = egui::vec2(4.0, 2.0);
    style.spacing.window_margin = egui::Margin::same(4);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);

    ctx.set_style(style);
}
