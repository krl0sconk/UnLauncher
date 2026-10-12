// SPDX-License-Identifier: GPL-3.0-or-later

//! Banner: a message about the whole app, at the top of the screen it
//! concerns. At most one at a time.

use egui::{Align, Frame, Layout, Stroke, Ui};

use super::{colors, paint_icon};
use crate::theme::{self, Icon, Text, radius, size, space};

/// What kind of message it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// `accent-soft`: a new engine version, a zip found in Downloads.
    Notice,
    /// `danger-soft`: the editor crashed, a sign-in expired.
    #[allow(
        dead_code,
        reason = "the design system's problem banner, for screens that need it"
    )]
    Problem,
}

/// Draws the banner: `icon`, a one-line `title`, a `detail` line and the
/// buttons `actions` adds on the right (the first one added is the
/// rightmost: one primary at most, then Later or a close button).
pub fn banner(
    ui: &mut Ui,
    tone: Tone,
    icon: Icon,
    title: &str,
    detail: &str,
    actions: impl FnOnce(&mut Ui),
) {
    let p = colors(ui);
    let (fill, icon_color) = match tone {
        Tone::Notice => (p.accent_soft, p.accent_ink),
        Tone::Problem => (p.danger_soft, p.danger),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::NONE)
        .corner_radius(theme::corner(radius::LG))
        .inner_margin(theme::margin_xy(space::S4, space::S3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(size::ICON), egui::Sense::hover());
                paint_icon(ui, icon, icon_rect.center(), size::ICON, icon_color);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(Text::BodyStrong.colored(title, p.ink));
                    ui.label(Text::Caption.colored(detail, p.ink_muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), actions);
            });
        });
}
