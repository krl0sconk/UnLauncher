// SPDX-License-Identifier: GPL-3.0-or-later

//! `AssetCard`: one item from the user's Fab library. Click opens the asset
//! dialog.

use egui::{Id, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2};

use super::badge::{Badge, Tone};
use super::thumbnail::{self, Mark};
use super::{colors, focus_ring, one_line, top_corners};
use crate::images::Picture;
use crate::theme::{self, Icon, Text, radius, size, space};

/// Height of the text area under the picture (same as a project card).
const BODY_HEIGHT: f32 = 2.0 * space::S3 + 20.0 + 2.0 + 16.0;

/// The card's height for a given width.
pub fn height_for(width: f32) -> f32 {
    width / size::THUMB_RATIO + BODY_HEIGHT
}

/// What an asset card shows.
pub struct AssetCard<'a> {
    /// The asset's title.
    pub title: &'a str,
    /// Kind and engine range (`Asset pack · UE 5.0–5.7`).
    pub meta: &'a str,
    /// The icon of its kind (`package` for asset packs, `box` for plugins).
    pub kind_icon: Icon,
    /// The thumbnail, if it has loaded.
    pub picture: Option<Picture>,
    /// It is already in the project the user has selected.
    pub in_project: bool,
}

/// Draws the card in `rect`. Returns true when it was clicked (or Enter was
/// pressed with focus on it).
pub fn asset_card(ui: &mut Ui, id: Id, rect: Rect, card: &AssetCard<'_>) -> bool {
    let p = colors(ui);
    let response = ui.interact(rect, id, Sense::click());
    let fill = if response.hovered() {
        p.bg_300
    } else {
        p.bg_200
    };
    ui.painter()
        .rect_filled(rect, theme::corner(radius::LG), fill);

    let thumb = Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width(), rect.width() / size::THUMB_RATIO),
    );
    thumbnail::paint(
        ui,
        thumb,
        top_corners(radius::LG),
        card.picture,
        card.title,
        Mark::Icon(card.kind_icon),
    );
    if card.in_project {
        let badge = Rect::from_min_size(
            thumb.min + Vec2::splat(space::S2),
            Vec2::new(thumb.width() - 2.0 * space::S2, size::BADGE_HEIGHT),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(badge));
        child.add(Badge::new(Tone::Success, crate::text::IN_PROJECT).icon(Icon::Check));
    }

    let inner = rect.width() - 2.0 * space::S3;
    let title = one_line(ui, card.title, Text::CardTitle, p.ink, inner);
    let meta = one_line(ui, card.meta, Text::Caption, p.ink_muted, inner);
    let top = thumb.bottom() + space::S3;
    ui.painter()
        .galley(pos2(rect.left() + space::S3, top), title, p.ink);
    ui.painter().galley(
        pos2(
            rect.left() + space::S3,
            top + Text::CardTitle.line_height() + size::CARD_TEXT_GAP,
        ),
        meta,
        p.ink_muted,
    );

    ui.painter().rect_stroke(
        rect,
        theme::corner(radius::LG),
        Stroke::new(size::STROKE, p.line),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        focus_ring(ui, rect, radius::LG);
    }
    response.clicked() || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
}
