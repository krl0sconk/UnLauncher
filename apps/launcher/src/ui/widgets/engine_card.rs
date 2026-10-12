// SPDX-License-Identifier: GPL-3.0-or-later

//! `EngineCard`: one installed Unreal Engine, and the dashed "Install engine"
//! card that ends the row. The version is the hero; Launch is primary only on
//! the engine launched last, so the row shows one amber fill.

use egui::{Align, Frame, Id, Layout, Sense, Stroke, Ui, Vec2};

use super::badge::{Badge, Tone};
use super::button::{Button, Kind, overflow_menu};
use super::{colors, dashed_round_rect, paint_icon};
use crate::text;
use crate::theme::{self, Icon, Text, radius, size, space};

/// How the engine was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// Unzipped from Epic's prebuilt build.
    Prebuilt,
    /// Built from the source.
    Source,
    /// Found on disk; UnLauncher does not know how it was installed.
    External,
}

/// What an engine card shows.
pub struct EngineCard<'a> {
    /// Major and minor version (`5.7`).
    pub numeral: &'a str,
    /// The full version or git ref (`5.7.4`, `4f2c1a9`).
    pub detail: &'a str,
    /// How it was installed.
    pub install: Install,
    /// The engine launched last.
    pub selected: bool,
}

/// Draws the card. Returns true when Launch was clicked. `menu_items` fills
/// the `⋮` menu (Update, Open folder, Add to app menu, Clean caches,
/// Uninstall).
pub fn engine_card(
    ui: &mut Ui,
    id: Id,
    card: &EngineCard<'_>,
    menu_items: impl FnOnce(&mut Ui),
) -> bool {
    let p = colors(ui);
    let edge = if card.selected { p.accent_edge } else { p.line };
    let mut launched = false;
    Frame::new()
        .fill(p.bg_200)
        .stroke(Stroke::new(size::STROKE, edge))
        .corner_radius(theme::corner(radius::LG))
        .inner_margin(theme::margin(space::S3))
        .show(ui, |ui| {
            // A frame inherits its parent's layout (a row of cards is
            // horizontal): the card's content is a column.
            ui.vertical(|ui| {
                ui.set_width(size::CARD_ENGINE_WIDTH - 2.0 * (space::S3 + size::STROKE));
                ui.spacing_mut().item_spacing.y = space::S3;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let numeral_color = if card.selected { p.accent_ink } else { p.ink };
                        ui.label(Text::VersionNumeral.colored(card.numeral, numeral_color));
                        ui.label(Text::Mono.colored(card.detail, p.ink_muted));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        let (rect, _) = ui
                            .allocate_exact_size(Vec2::splat(size::CONTROL_HEIGHT), Sense::hover());
                        overflow_menu(ui, rect, id.with("menu"), true, text::MORE, menu_items);
                    });
                });
                match card.install {
                    Install::Prebuilt => {
                        ui.add(
                            Badge::new(Tone::Neutral, text::BADGE_PREBUILT).icon(Icon::Download),
                        );
                    }
                    Install::Source => {
                        ui.add(Badge::new(Tone::Accent, text::BADGE_SOURCE).icon(Icon::GitBranch));
                    }
                    Install::External => {
                        ui.add(Badge::new(Tone::Neutral, text::BADGE_EXTERNAL).icon(Icon::Box));
                    }
                }
                let kind = if card.selected {
                    Kind::Primary
                } else {
                    Kind::Secondary
                };
                launched = ui
                    .add(
                        Button::new(kind, text::LAUNCH)
                            .icon(Icon::Play)
                            .fill_width(),
                    )
                    .clicked();
            });
        });
    launched
}

/// The dashed card that ends the row. Returns true when it was clicked.
pub fn install_card(ui: &mut Ui, id: Id) -> bool {
    let p = colors(ui);
    let width = size::CARD_ENGINE_WIDTH;
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, size::ENGINE_ADD_MIN_HEIGHT),
        Sense::hover(),
    );
    let response = ui.interact(rect, id, Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::corner(radius::LG), p.bg_200);
    }
    dashed_round_rect(
        ui.painter(),
        rect,
        radius::LG,
        Stroke::new(size::STROKE, p.line_strong),
    );
    let center = rect.center();
    let tile = egui::Rect::from_center_size(
        egui::pos2(center.x, center.y - space::S6),
        Vec2::splat(size::CONTROL_HEIGHT),
    );
    ui.painter()
        .rect_filled(tile, theme::corner(radius::MD), p.bg_300);
    paint_icon(ui, Icon::Plus, tile.center(), size::ICON, p.ink_muted);
    ui.painter().text(
        egui::pos2(center.x, tile.bottom() + space::S3),
        egui::Align2::CENTER_TOP,
        text::INSTALL_ENGINE_CARD,
        Text::BodyStrong.font(),
        p.ink_muted,
    );
    ui.painter().text(
        egui::pos2(
            center.x,
            tile.bottom() + space::S3 + Text::BodyStrong.line_height(),
        ),
        egui::Align2::CENTER_TOP,
        text::INSTALL_ENGINE_CARD_HINT,
        Text::Caption.font(),
        p.ink_faint,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
