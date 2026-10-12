// SPDX-License-Identifier: GPL-3.0-or-later

//! A single-line text field in the size of the other controls, and the row
//! that shows a path next to the button that changes it.

use egui::{Align, Layout, Response, TextEdit, Ui, Vec2};

use super::button::Button;
use super::colors;
use crate::theme::{Text, size, space};

/// A single-line field as wide as the row and `control-height` tall.
pub fn text_field(ui: &mut Ui, text: &mut String) -> Response {
    field(ui, text, false)
}

/// Like [`text_field`], but the text is shown as dots (tokens, codes).
pub fn secret_field(ui: &mut Ui, text: &mut String) -> Response {
    field(ui, text, true)
}

fn field(ui: &mut Ui, text: &mut String, secret: bool) -> Response {
    let p = colors(ui);
    ui.add(
        TextEdit::singleline(text)
            .password(secret)
            .font(Text::Body.font())
            .text_color(p.ink)
            .margin(crate::theme::margin_xy(space::S3, space::S2))
            .min_size(Vec2::new(0.0, size::CONTROL_HEIGHT))
            .desired_width(f32::INFINITY),
    )
}

/// A path in mono, cut with an ellipsis if the row is too narrow.
pub fn path_label(ui: &mut Ui, path: &str) {
    let p = colors(ui);
    ui.add(egui::Label::new(Text::Mono.colored(path, p.ink_muted)).truncate());
}

/// `path` on the left (cut if long) and a secondary button on the right.
/// Returns whether the button was clicked.
pub fn path_row(ui: &mut Ui, path: &str, button: &str) -> bool {
    let mut clicked = false;
    let row = Vec2::new(ui.available_width(), size::CONTROL_HEIGHT);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
        clicked = ui.add(Button::secondary(button)).clicked();
        path_label(ui, path);
    });
    clicked
}
