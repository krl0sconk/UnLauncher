// SPDX-License-Identifier: GPL-3.0-or-later

//! The 16:9 picture of a card: the real image cropped to cover, or a
//! `thumb-*` placeholder until it is there.

use egui::epaint::RectShape;
use egui::{Color32, CornerRadius, Rect, Ui, Vec2, pos2};

use super::colors;
use crate::images::{Picture, placeholder};
use crate::theme::{Icon, Text, size};

/// What a placeholder shows.
#[derive(Debug, Clone, Copy)]
pub enum Mark {
    /// Two initials of the name (projects).
    Initials,
    /// An icon for the kind (Fab assets).
    Icon(Icon),
}

/// The part of a picture of `image` size that fills a box of `target`'s
/// proportions without stretching (`object-fit: cover`), as a UV rectangle.
pub fn cover_uv(image: Vec2, target: Vec2) -> Rect {
    let image_aspect = image.x / image.y;
    let target_aspect = target.x / target.y;
    if !image_aspect.is_finite() || !target_aspect.is_finite() {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    if image_aspect > target_aspect {
        // Too wide: crop the sides.
        let shown = target_aspect / image_aspect;
        Rect::from_min_max(pos2(0.5 - shown / 2.0, 0.0), pos2(0.5 + shown / 2.0, 1.0))
    } else {
        // Too tall: crop top and bottom.
        let shown = image_aspect / target_aspect;
        Rect::from_min_max(pos2(0.0, 0.5 - shown / 2.0), pos2(1.0, 0.5 + shown / 2.0))
    }
}

/// Draws the picture, or the placeholder of `name`, in `rect`.
pub fn paint(
    ui: &Ui,
    rect: Rect,
    corners: CornerRadius,
    picture: Option<Picture>,
    name: &str,
    mark: Mark,
) {
    let p = colors(ui);
    if let Some(picture) = picture {
        let uv = cover_uv(picture.size, rect.size());
        ui.painter()
            .add(RectShape::filled(rect, corners, Color32::WHITE).with_texture(picture.id, uv));
        return;
    }
    ui.painter()
        .rect_filled(rect, corners, placeholder::color(p, name));
    match mark {
        Mark::Initials => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                placeholder::initials(name),
                Text::Initials.font(),
                p.ink.gamma_multiply(0.85),
            );
        }
        Mark::Icon(icon) => super::paint_icon(ui, icon, rect.center(), size::ICON_LG, p.ink),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_picture_is_cropped_at_the_sides() {
        let uv = cover_uv(Vec2::new(1000.0, 400.0), Vec2::new(160.0, 90.0));
        assert!((uv.min.y - 0.0).abs() < 1e-6 && (uv.max.y - 1.0).abs() < 1e-6);
        let kept = uv.width() * 1000.0 / 400.0; // aspect of the visible part
        assert!((kept - 160.0 / 90.0).abs() < 1e-3, "{kept}");
        assert!((uv.center().x - 0.5).abs() < 1e-6, "centered");
    }

    #[test]
    fn a_tall_picture_is_cropped_top_and_bottom() {
        let uv = cover_uv(Vec2::new(300.0, 600.0), Vec2::new(160.0, 90.0));
        assert!((uv.min.x - 0.0).abs() < 1e-6 && (uv.max.x - 1.0).abs() < 1e-6);
        let kept = 300.0 / (uv.height() * 600.0);
        assert!((kept - 160.0 / 90.0).abs() < 1e-3, "{kept}");
    }

    #[test]
    fn a_picture_of_the_same_proportions_is_shown_whole() {
        let uv = cover_uv(Vec2::new(512.0, 288.0), Vec2::new(160.0, 90.0));
        assert!((uv.width() - 1.0).abs() < 1e-3 && (uv.height() - 1.0).abs() < 1e-3);
    }
}
