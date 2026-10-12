// SPDX-License-Identifier: GPL-3.0-or-later

//! Badge: a small uppercase label for the kind or state of a thing. Neutral
//! for kinds; accent only for Source engines; the status tones always carry
//! an icon and a word. A badge is not a button.

use egui::{Color32, Response, Sense, Ui, Vec2, Widget};

use super::{colors, paint_icon};
use crate::theme::{self, Icon, Palette, Text, radius, size, space};

/// The color family of a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// `bg-300` and `ink-muted`: the kind of a thing.
    Neutral,
    /// `accent-soft`: Source engines.
    Accent,
    /// Installed, in project.
    Success,
    /// No engine.
    #[allow(
        dead_code,
        reason = "a status tone of the design system, for screens that need it"
    )]
    Warning,
    /// Failed.
    #[allow(
        dead_code,
        reason = "a status tone of the design system, for screens that need it"
    )]
    Danger,
}

impl Tone {
    /// Fill and text colors.
    pub fn colors(self, p: &Palette) -> (Color32, Color32) {
        match self {
            Self::Neutral => (p.bg_300, p.ink_muted),
            Self::Accent => (p.accent_soft, p.accent_ink),
            Self::Success => (p.success_soft, p.success),
            Self::Warning => (p.warning_soft, p.warning),
            Self::Danger => (p.danger_soft, p.danger),
        }
    }
}

/// A badge with a label and an optional icon.
#[must_use = "add it with `ui.add(...)`"]
pub struct Badge<'a> {
    tone: Tone,
    label: &'a str,
    icon: Option<Icon>,
}

impl<'a> Badge<'a> {
    /// A badge of `tone`.
    pub fn new(tone: Tone, label: &'a str) -> Self {
        Self {
            tone,
            label,
            icon: None,
        }
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl Widget for Badge<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = colors(ui);
        let (fill, ink) = self.tone.colors(p);
        let galley =
            ui.painter()
                .layout_no_wrap(self.label.to_uppercase(), Text::Badge.font(), ink);
        let icon_width = if self.icon.is_some() {
            size::ICON_BADGE + space::S1
        } else {
            0.0
        };
        let width = 2.0 * space::S2 + icon_width + galley.size().x;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, size::BADGE_HEIGHT), Sense::hover());
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, theme::corner(radius::SM), fill);
            let mut x = rect.left() + space::S2;
            if let Some(icon) = self.icon {
                paint_icon(
                    ui,
                    icon,
                    egui::pos2(x + size::ICON_BADGE / 2.0, rect.center().y),
                    size::ICON_BADGE,
                    ink,
                );
                x += icon_width;
            }
            ui.painter().galley(
                egui::pos2(x, rect.center().y - galley.size().y / 2.0),
                galley,
                ink,
            );
        }
        response
    }
}

/// A badge-shaped toggle that filters a list (All, Asset packs, Plugins).
/// The chosen one is accent, the others neutral.
pub fn filter_chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let p = colors(ui);
    let (fill, ink) = if selected {
        Tone::Accent.colors(p)
    } else {
        Tone::Neutral.colors(p)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_uppercase(), Text::Badge.font(), ink);
    let width = 2.0 * space::S2 + galley.size().x;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, size::BADGE_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if !selected && response.hovered() {
            p.line
        } else {
            fill
        };
        ui.painter()
            .rect_filled(rect, theme::corner(radius::SM), fill);
        ui.painter().galley(
            egui::pos2(
                rect.left() + space::S2,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            ink,
        );
        if response.has_focus() {
            super::focus_ring(ui, rect, radius::SM);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
