// SPDX-License-Identifier: GPL-3.0-or-later

//! The components of `docs/design/components`, drawn with egui. Sizes, states
//! and colors come from `theme` and follow `reference/components.css`.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, Galley, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{self, Icon, Palette, Text, size};

pub mod asset_card;
pub mod badge;
pub mod banner;
pub mod button;
pub mod dialog;
pub mod empty_state;
pub mod engine_card;
pub mod project_card;
pub mod search_field;
pub mod section_header;
pub mod sidebar;
pub mod task_row;
pub mod text_field;
pub mod thumbnail;

/// Leaves `amount` points between the widget above and the one below.
/// (`add_space` alone adds the item spacing on both sides.)
pub fn gap(ui: &mut Ui, amount: f32) {
    let spacing = ui.spacing().item_spacing.y;
    ui.add_space((amount - 2.0 * spacing).max(0.0));
}

/// The palette of the theme `ui` is drawn in.
pub fn colors(ui: &Ui) -> &'static Palette {
    theme::palette(ui.ctx())
}

/// `text` on one row, cut with an ellipsis if it is wider than `max_width`.
pub fn one_line(ui: &Ui, text: &str, style: Text, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), style.font(), color, max_width.max(0.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

/// Draws `icon` centered at `center`.
pub fn paint_icon(ui: &Ui, icon: Icon, center: Pos2, icon_size: f32, color: Color32) {
    icon.image(icon_size, color)
        .paint_at(ui, Rect::from_center_size(center, Vec2::splat(icon_size)));
}

/// The keyboard focus ring: 2 px solid, 2 px outside `rect`.
pub fn focus_ring(ui: &Ui, rect: Rect, corner: u8) {
    let ring = colors(ui).focus_ring;
    ui.painter().rect_stroke(
        rect.expand(size::FOCUS),
        theme::corner(corner.saturating_add(size::FOCUS_RADIUS)),
        Stroke::new(size::FOCUS, ring),
        StrokeKind::Outside,
    );
}

/// `color` at the opacity of a disabled control when `enabled` is false.
pub fn faded(color: Color32, enabled: bool) -> Color32 {
    if enabled {
        color
    } else {
        color.gamma_multiply(size::DISABLED_OPACITY)
    }
}

/// A rounded rectangle outline drawn as dashes (empty states, the "Install
/// engine" card).
pub fn dashed_round_rect(painter: &Painter, rect: Rect, corner: u8, stroke: Stroke) {
    const DASH: f32 = 4.0;
    const GAP: f32 = 4.0;
    const ARC_STEPS: u8 = 6;
    let r = f32::from(corner)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    let mut points: Vec<Pos2> = Vec::new();
    // Corners clockwise from the top left, each as a quarter arc.
    for (center, start) in [
        (Pos2::new(rect.left() + r, rect.top() + r), 180.0_f32),
        (Pos2::new(rect.right() - r, rect.top() + r), 270.0),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 90.0),
    ] {
        for step in 0..=ARC_STEPS {
            let angle = (start + 90.0 * f32::from(step) / f32::from(ARC_STEPS)).to_radians();
            points.push(center + Vec2::new(angle.cos(), angle.sin()) * r);
        }
    }
    if let Some(first) = points.first().copied() {
        points.push(first);
    }
    painter.extend(Shape::dashed_line(&points, stroke, DASH, GAP));
}

/// The corner radius with only the top corners rounded (a card's thumbnail).
pub fn top_corners(corner: u8) -> CornerRadius {
    CornerRadius {
        nw: corner,
        ne: corner,
        sw: 0,
        se: 0,
    }
}

/// Lays `count` cards of at least `min_width` in a grid that fills the row,
/// `gap` apart. `height_for` gives a card's height from its width; `draw`
/// is called with the card's index and rectangle, only for cards that are on
/// screen (so pictures are only requested for those).
pub fn card_grid(
    ui: &mut Ui,
    count: usize,
    min_width: f32,
    gap: f32,
    height_for: impl Fn(f32) -> f32,
    mut draw: impl FnMut(&mut Ui, usize, Rect),
) {
    let available = ui.available_width();
    let (columns, width) = grid_columns(available, min_width, gap);
    let height = height_for(width);
    let rows = count.div_ceil(columns);
    if rows == 0 {
        return;
    }
    #[allow(clippy::cast_precision_loss, reason = "a screenful of rows")]
    let total = rows as f32 * height + (rows - 1) as f32 * gap;
    let (area, _) = ui.allocate_exact_size(Vec2::new(available, total), egui::Sense::hover());
    for index in 0..count {
        #[allow(clippy::cast_precision_loss, reason = "a screenful of columns")]
        let (column, row) = ((index % columns) as f32, (index / columns) as f32);
        let origin = area.min + Vec2::new(column * (width + gap), row * (height + gap));
        let cell = Rect::from_min_size(origin, Vec2::new(width, height));
        if ui.is_rect_visible(cell) {
            draw(ui, index, cell);
        }
    }
}

/// How many columns of at least `min_width` fit in `available`, and how wide
/// each one is when they share the row.
pub fn grid_columns(available: f32, min_width: f32, gap: f32) -> (usize, f32) {
    let fit = ((available + gap) / (min_width + gap)).floor();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small positive count"
    )]
    let columns = (fit as usize).max(1);
    #[allow(clippy::cast_precision_loss, reason = "a small column count")]
    let width = (available - gap * (columns - 1) as f32) / columns as f32;
    (columns, width.max(min_width.min(available)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fills_the_row_with_as_many_cards_as_fit() {
        // 200 px cards, 12 px apart.
        let (columns, width) = grid_columns(1000.0, 200.0, 12.0);
        assert_eq!(columns, 4);
        assert!((width - 241.0).abs() < 0.01, "{width}");
        let (columns, _) = grid_columns(412.0, 200.0, 12.0);
        assert_eq!(columns, 2);
        let (columns, _) = grid_columns(411.0, 200.0, 12.0);
        assert_eq!(columns, 1);
    }

    #[test]
    fn a_narrow_window_still_gets_one_column() {
        let (columns, width) = grid_columns(150.0, 200.0, 12.0);
        assert_eq!(columns, 1);
        assert!((width - 150.0).abs() < 0.01, "{width}");
    }
}
