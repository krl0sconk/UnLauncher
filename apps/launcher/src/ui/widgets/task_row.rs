// SPDX-License-Identifier: GPL-3.0-or-later

//! `TaskRow`: one long-running job on the Tasks screen. A kind icon in a
//! tile tinted by state, the title, live figures, a progress bar, the
//! current step and the actions.

use egui::{Align, Color32, Layout, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2};

use super::{colors, one_line, paint_icon};
use crate::theme::{self, Icon, Palette, Text, radius, size, space};

/// The state a row is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Queued or running.
    Running,
    /// Paused.
    Paused,
    /// Finished.
    Done,
    /// Failed or interrupted.
    Failed,
    /// Cancelled.
    Stopped,
}

impl Tone {
    /// Tile fill, tile icon and bar colors.
    fn colors(self, p: &Palette) -> (Color32, Color32, Color32) {
        match self {
            Self::Running => (p.accent_soft, p.accent_ink, p.accent),
            Self::Paused => (p.warning_soft, p.warning, p.warning),
            Self::Done => (p.success_soft, p.success, p.success),
            Self::Failed => (p.danger_soft, p.danger, p.danger),
            Self::Stopped => (p.bg_300, p.ink_muted, p.ink_faint),
        }
    }
}

/// What a task row shows.
pub struct TaskRow<'a> {
    /// `download`, `hammer`, `package` or `folder-open`.
    pub icon: Icon,
    /// The state.
    pub tone: Tone,
    /// What is happening to what.
    pub title: &'a str,
    /// Steps, size, percentage, speed and time left.
    pub figures: &'a str,
    /// Progress from 0 to 1; `None` when unknown.
    pub fraction: Option<f32>,
    /// The current file or the last log line; for a failure, the error.
    pub step: &'a str,
    /// Whether to animate an unknown progress (the task is running).
    pub animate: bool,
}

/// The row's height: padding, the title line, the bar and the step line.
pub fn height() -> f32 {
    2.0 * space::S3
        + Text::CardTitle.line_height()
        + 2.0 * space::S1
        + size::PROGRESS
        + Text::Mono.line_height()
}

/// Draws the row. `actions` adds the buttons on the right (the first one
/// added is the rightmost): Pause and Cancel while running, Resume when
/// paused, Show when done, Details when failed.
pub fn task_row(ui: &mut Ui, row: &TaskRow<'_>, actions: impl FnOnce(&mut Ui)) {
    let p = colors(ui);
    let (tile_fill, tile_ink, bar) = row.tone.colors(p);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height()), Sense::hover());
    ui.painter().rect(
        rect,
        theme::corner(radius::MD),
        p.bg_200,
        Stroke::new(size::STROKE, p.line),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(space::S3);

    // Actions first: they decide how much room the text has.
    let mut buttons = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
    );
    actions(&mut buttons);
    let used = buttons.min_rect();
    let right = if used.width() > 0.0 {
        used.left() - space::S3
    } else {
        inner.right()
    };

    let tile = Rect::from_center_size(
        pos2(inner.left() + size::TASK_ICON / 2.0, inner.center().y),
        Vec2::splat(size::TASK_ICON),
    );
    ui.painter()
        .rect_filled(tile, theme::corner(radius::MD), tile_fill);
    paint_icon(ui, row.icon, tile.center(), size::ICON, tile_ink);

    let left = tile.right() + space::S3;
    let width = (right - left).max(0.0);
    let top = inner.top();
    let figures = one_line(ui, row.figures, Text::MonoFigures, p.ink_muted, width / 2.0);
    let figures_width = if row.figures.is_empty() {
        0.0
    } else {
        figures.size().x
    };
    let room = width - figures_width - if figures_width > 0.0 { space::S2 } else { 0.0 };
    let heading = one_line(ui, row.title, Text::CardTitle, p.ink, room);
    ui.painter().galley(pos2(left, top), heading, p.ink);
    if figures_width > 0.0 {
        ui.painter().galley(
            pos2(
                right - figures_width,
                top + (Text::CardTitle.line_height() - figures.size().y) / 2.0,
            ),
            figures,
            p.ink_muted,
        );
    }

    let track = Rect::from_min_size(
        pos2(left, top + Text::CardTitle.line_height() + space::S1),
        Vec2::new(width, size::PROGRESS),
    );
    progress_bar(ui, track, row.fraction, row.animate, bar);

    let step = one_line(ui, row.step, Text::Mono, p.ink_faint, width);
    ui.painter()
        .galley(pos2(left, track.bottom() + space::S1), step, p.ink_faint);
}

/// A 6 px bar: a `bg-300` track and a fill in the state color. Without a
/// fraction, a segment slides along the track while `animate` is set.
fn progress_bar(ui: &Ui, track: Rect, fraction: Option<f32>, animate: bool, color: Color32) {
    let p = colors(ui);
    ui.painter()
        .rect_filled(track, theme::corner(radius::SM), p.bg_300);
    let fill = match fraction {
        Some(fraction) => {
            let width = track.width() * fraction.clamp(0.0, 1.0);
            Rect::from_min_size(track.min, Vec2::new(width, track.height()))
        }
        None if animate => {
            let segment = track.width() * size::PROGRESS_SEGMENT;
            // 0 to 1 and back, once every two seconds.
            let time = ui.input(|input| input.time);
            let phase = ((time / 2.0).fract() * 2.0 - 1.0).abs();
            #[allow(clippy::cast_possible_truncation, reason = "a pixel offset")]
            let offset = (track.width() - segment) * phase as f32;
            ui.ctx().request_repaint();
            Rect::from_min_size(
                pos2(track.left() + offset, track.top()),
                Vec2::new(segment, track.height()),
            )
        }
        None => return,
    };
    if fill.width() > 0.0 {
        ui.painter()
            .rect_filled(fill, theme::corner(radius::SM), color);
    }
}
