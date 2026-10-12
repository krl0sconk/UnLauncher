// SPDX-License-Identifier: GPL-3.0-or-later

//! `SearchField`: a text field with a leading search icon. Filters as you
//! type; Escape clears it.

use egui::{Frame, Key, Layout, Sense, TextEdit, Ui, UiBuilder, Vec2};

use super::{colors, focus_ring, paint_icon};
use crate::theme::{self, Icon, Text, radius, size, space};

/// Draws the field, `width` points wide, over `text`. Returns whether the
/// text changed.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> bool {
    let p = colors(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, size::CONTROL_HEIGHT), Sense::hover());
    ui.painter().rect(
        rect,
        theme::corner(radius::MD),
        p.bg_200,
        egui::Stroke::new(size::STROKE, p.line_strong),
        egui::StrokeKind::Inside,
    );
    paint_icon(
        ui,
        Icon::Search,
        egui::pos2(rect.left() + space::S3 + size::ICON / 2.0, rect.center().y),
        size::ICON,
        p.ink_muted,
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + space::S3 + size::ICON + space::S2, rect.top()),
        egui::pos2(rect.right() - space::S3, rect.bottom()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(egui::Align::Center)),
    );
    let response = child.add(
        TextEdit::singleline(text)
            .frame(Frame::NONE)
            .font(Text::Body.font())
            .text_color(p.ink)
            .hint_text(Text::Body.colored(hint, p.ink_faint))
            .desired_width(inner.width()),
    );
    let mut changed = response.changed();
    if response.has_focus() {
        if ui.input(|input| input.key_pressed(Key::Escape)) && !text.is_empty() {
            text.clear();
            changed = true;
        }
        focus_ring(ui, rect, radius::MD);
    }
    changed
}
