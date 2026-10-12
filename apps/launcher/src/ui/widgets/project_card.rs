// SPDX-License-Identifier: GPL-3.0-or-later

//! `ProjectCard`: one Unreal project. Double-click opens it; the card menu
//! holds Change engine, Show in folder and Delete project.

use egui::{Id, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2};

use super::button::overflow_menu;
use super::thumbnail::{self, Mark};
use super::{colors, faded, focus_ring, one_line, paint_icon, top_corners};
use crate::images::Picture;
use crate::theme::{self, Icon, Text, radius, size, space};

/// Height of the text area under the picture: padding, a title row, the
/// 2 px gap and a meta row.
const BODY_HEIGHT: f32 = 2.0 * space::S3 + 20.0 + 2.0 + 16.0;

/// The card's height for a given width.
pub fn height_for(width: f32) -> f32 {
    width / size::THUMB_RATIO + BODY_HEIGHT
}

/// What a project card shows.
pub struct ProjectCard<'a> {
    /// The project's name.
    pub name: &'a str,
    /// Engine version and folder (`UE 5.7 · ~/Documents/Unreal Projects`).
    pub meta: &'a str,
    /// The thumbnail, if it has loaded.
    pub picture: Option<Picture>,
    /// If the project's engine is not installed: what to say on the strip
    /// (`No 5.5 engine installed`).
    pub missing_engine: Option<&'a str>,
}

/// Draws the card in `rect`. Returns true when the user opened it (double
/// click, or Enter while it has focus). `menu_items` fills the `⋮` menu.
pub fn project_card(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    card: &ProjectCard<'_>,
    menu_items: impl FnOnce(&mut Ui),
) -> bool {
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
        card.name,
        Mark::Initials,
    );
    if let Some(text) = card.missing_engine {
        strip(ui, thumb, text);
    }

    let inner = rect.width() - 2.0 * space::S3;
    let title = one_line(ui, card.name, Text::CardTitle, p.ink, inner);
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

    let button = Rect::from_min_size(
        pos2(
            thumb.right() - space::S2 - size::CONTROL_HEIGHT,
            thumb.top() + space::S2,
        ),
        Vec2::splat(size::CONTROL_HEIGHT),
    );
    overflow_menu(
        ui,
        button,
        id.with("menu"),
        response.contains_pointer(),
        crate::text::MORE,
        menu_items,
    );

    response.double_clicked()
        || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
}

/// The warning strip across the bottom of the thumbnail: a solid `bg-200`
/// band, so text never sits on the picture.
fn strip(ui: &Ui, thumb: Rect, text: &str) {
    let p = colors(ui);
    let height = 16.0 + 2.0 * space::S1;
    let band = Rect::from_min_max(
        pos2(thumb.left(), thumb.bottom() - height),
        thumb.right_bottom(),
    );
    ui.painter().rect_filled(band, 0.0, p.bg_200);
    paint_icon(
        ui,
        Icon::TriangleAlert,
        pos2(
            band.left() + space::S2 + size::ICON_BADGE / 2.0,
            band.center().y,
        ),
        size::ICON_BADGE,
        p.warning,
    );
    let galley = one_line(
        ui,
        text,
        Text::Strip,
        faded(p.warning, true),
        band.width() - 2.0 * space::S2 - size::ICON_BADGE - space::S1,
    );
    ui.painter().galley(
        pos2(
            band.left() + space::S2 + size::ICON_BADGE + space::S1,
            band.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.warning,
    );
}
