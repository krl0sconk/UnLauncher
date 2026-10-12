// SPDX-License-Identifier: GPL-3.0-or-later

//! Dialog: a focused task over the current screen. `bg-200`, `radius-lg`,
//! the only shadow of the system, over the scrim. A header with the title
//! and a close button, a body, and a footer with right-aligned actions.

use egui::{Align, Context, Frame, Id, Layout, Modal, Sense, Stroke, Ui, Vec2};

use super::button::IconButton;
use super::{colors, paint_icon};
use crate::text;
use crate::theme::{self, Icon, Text, radius, size, space};

/// Shows a dialog titled `title` with `content`, and returns true when the
/// user asked to close it (the X button, Escape or a click on the scrim).
/// `content` calls [`body`] and [`footer`].
pub fn show(
    ctx: &Context,
    id: Id,
    title: &str,
    closable: bool,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let p = theme::palette(ctx);
    let frame = Frame::new()
        .fill(p.bg_200)
        .stroke(Stroke::new(size::STROKE, p.line))
        .corner_radius(theme::corner(radius::LG))
        .shadow(ctx.global_style().visuals.popup_shadow);
    let mut close = false;
    let response = Modal::new(id)
        .frame(frame)
        .backdrop_color(p.scrim)
        .show(ctx, |ui| {
            ui.set_width(size::DIALOG_WIDTH);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            Frame::new()
                .inner_margin(egui::Margin {
                    left: theme::margin(space::S4).left,
                    right: theme::margin(space::S4).right,
                    top: theme::margin(space::S4).top,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(Text::CardTitle.colored(title, p.ink));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if closable && ui.add(IconButton::new(Icon::X, text::CLOSE)).clicked() {
                                close = true;
                            }
                        });
                    });
                });
            content(ui);
        });
    close || (closable && response.should_close())
}

/// The padded body of a dialog.
pub fn body(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .inner_margin(theme::margin(space::S4))
        .show(ui, |ui| {
            ui.set_width(size::DIALOG_WIDTH - 2.0 * space::S4);
            ui.spacing_mut().item_spacing = Vec2::splat(space::S3);
            add_contents(ui);
        });
}

/// The footer: a `line` rule, then actions right-aligned. `add_actions`
/// adds the primary action first (it ends up rightmost).
pub fn footer(ui: &mut Ui, add_actions: impl FnOnce(&mut Ui)) {
    let p = colors(ui);
    let (rule, _) =
        ui.allocate_exact_size(Vec2::new(size::DIALOG_WIDTH, size::STROKE), Sense::hover());
    ui.painter().rect_filled(rule, 0.0, p.line);
    Frame::new()
        .inner_margin(theme::margin_xy(space::S4, space::S3))
        .show(ui, |ui| {
            ui.set_width(size::DIALOG_WIDTH - 2.0 * space::S4);
            ui.spacing_mut().item_spacing.x = space::S2;
            ui.with_layout(Layout::right_to_left(Align::Center), add_actions);
        });
}

/// Where a step of a multi-step flow stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// Finished: a `success` check.
    Done,
    /// The one in progress: its number on `accent`.
    Current,
    /// Still to do: its number on `bg-300`.
    Todo,
}

/// One entry of a step list.
pub struct Step<'a> {
    /// Its state.
    pub state: StepState,
    /// Its number, from 1.
    pub number: usize,
    /// What it is.
    pub title: &'a str,
    /// What the user needs to know or do.
    pub detail: Option<&'a str>,
}

/// The step list of a multi-step flow. `extra` draws anything under a
/// step's text (a code, a button); it gets the step's index.
pub fn steps(ui: &mut Ui, items: &[Step<'_>], mut extra: impl FnMut(&mut Ui, usize)) {
    let p = colors(ui);
    for (index, step) in items.iter().enumerate() {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(size::STEP_DOT), Sense::hover());
            let (fill, ink) = match step.state {
                StepState::Done => (p.success_soft, p.success),
                StepState::Current => (p.accent, p.on_accent),
                StepState::Todo => (p.bg_300, p.ink_muted),
            };
            ui.painter()
                .rect_filled(dot, theme::corner(radius::FULL), fill);
            if step.state == StepState::Done {
                paint_icon(ui, Icon::Check, dot.center(), size::ICON, ink);
            } else {
                ui.painter().text(
                    dot.center(),
                    egui::Align2::CENTER_CENTER,
                    step.number.to_string(),
                    Text::Avatar.font(),
                    ink,
                );
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                ui.label(Text::BodyStrong.colored(step.title, p.ink));
                if let Some(detail) = step.detail {
                    ui.label(Text::Caption.colored(detail, p.ink_muted));
                }
                extra(ui, index);
            });
        });
    }
}

/// A code the user must type, in mono on `bg-300`.
pub fn code(ui: &mut Ui, code: &str) {
    let p = colors(ui);
    Frame::new()
        .fill(p.bg_300)
        .corner_radius(theme::corner(radius::MD))
        .inner_margin(theme::margin_xy(space::S3, space::S2))
        .show(ui, |ui| {
            ui.label(Text::Code.colored(code, p.ink));
        });
}
