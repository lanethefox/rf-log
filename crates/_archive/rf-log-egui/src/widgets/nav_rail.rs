use eframe::egui::{self, Color32, Rect, CornerRadius, Sense, Stroke, Vec2};

use crate::state::{UiState, Workflow};
use crate::theme::*;

/// 56px vertical navigation rail — 4 workflow buttons
pub fn show(ui: &mut egui::Ui, ui_state: &mut UiState) {
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, CornerRadius::ZERO, BG_SURFACE);
    // Right border
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, BORDER),
    );

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(4.0);

        for workflow in &[Workflow::Collect, Workflow::Exploit, Workflow::Plan, Workflow::Watchdog] {
            let is_active = ui_state.active_workflow == *workflow;
            let accent = workflow_color(workflow);

            let btn_w = NAV_RAIL_WIDTH - 1.0;
            let btn_h = NAV_BUTTON_SIZE;
            let (response, painter) = ui.allocate_painter(
                Vec2::new(btn_w, btn_h),
                Sense::click(),
            );
            let rect = response.rect;

            // Background fill
            if is_active {
                // Active: tinted background in workflow color
                painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_premultiplied(
                    accent.r() / 4, accent.g() / 4, accent.b() / 4, 50,
                ));
                // Left accent bar (3px)
                painter.rect_filled(
                    Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
                    CornerRadius::ZERO,
                    accent,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, CornerRadius::ZERO, BG_ELEVATED);
            }

            // Label text (centered, no emoji icons — use text labels for reliability)
            let label = workflow.label();
            let label_color = if is_active { accent } else { TEXT_SECONDARY };

            // Two-line layout: short icon char + label
            let icon_char = match workflow {
                Workflow::Collect => "C",
                Workflow::Exploit => "E",
                Workflow::Plan => "P",
                Workflow::Watchdog => "W",
            };

            // Icon letter (large, centered)
            let icon_galley = painter.layout_no_wrap(
                icon_char.to_string(),
                egui::FontId::new(20.0, egui::FontFamily::Monospace),
                label_color,
            );
            let icon_pos = egui::pos2(
                rect.center().x - icon_galley.size().x / 2.0,
                rect.center().y - icon_galley.size().y / 2.0 - 6.0,
            );
            painter.galley(icon_pos, icon_galley, label_color);

            // Label (below icon, small)
            let label_galley = painter.layout_no_wrap(
                label.to_string(),
                egui::FontId::new(7.0, egui::FontFamily::Monospace),
                label_color,
            );
            let label_pos = egui::pos2(
                rect.center().x - label_galley.size().x / 2.0,
                rect.center().y + 8.0,
            );
            painter.galley(label_pos, label_galley, label_color);

            if response.clicked() {
                ui_state.active_workflow = *workflow;
            }

            // Tooltip
            response.on_hover_text(format!("{} view (Ctrl+{})", workflow.label(), match workflow {
                Workflow::Collect => "1",
                Workflow::Exploit => "2",
                Workflow::Plan => "3",
                Workflow::Watchdog => "4",
            }));
        }
    });
}
