// SPDX-License-Identifier: GPL-3.0-or-later

//! `EmptyState`: what a section shows when it has nothing to list. It says
//! what will appear and gives the one action that makes it appear.

use egui::{Align, Layout, Sense, Stroke, Ui, Vec2};

use super::{colors, dashed_round_rect, paint_icon};
use crate::theme::{self, Icon, Text, radius, size, space};

/// Draws the empty state: a tile with `icon`, `title`, up to two sentences
/// of `body` and the action `action` adds (one primary button).
pub fn empty_state(ui: &mut Ui, icon: Icon, title: &str, body: &str, action: impl FnOnce(&mut Ui)) {
    let p = colors(ui);
    let width = ui.available_width();
    let response = ui
        .allocate_ui_with_layout(
            Vec2::new(width, 0.0),
            Layout::top_down(Align::Center),
            |ui| {
                ui.set_width(width);
                ui.add_space(space::S8);
                let (mark_rect, _) =
                    ui.allocate_exact_size(Vec2::splat(size::EMPTY_MARK), Sense::hover());
                ui.painter()
                    .rect_filled(mark_rect, theme::corner(radius::LG), p.bg_300);
                paint_icon(ui, icon, mark_rect.center(), size::ICON_LG, p.ink_muted);
                ui.add_space(space::S3);
                ui.label(Text::CardTitle.colored(title, p.ink));
                ui.add_space(space::S3);
                ui.scope(|ui| {
                    ui.set_max_width(size::EMPTY_TEXT_WIDTH);
                    ui.label(Text::Body.colored(body, p.ink_muted));
                });
                ui.add_space(space::S3);
                action(ui);
                ui.add_space(space::S8);
            },
        )
        .response;
    dashed_round_rect(
        ui.painter(),
        response.rect,
        radius::LG,
        Stroke::new(size::STROKE, p.line_strong),
    );
}
