// SPDX-License-Identifier: GPL-3.0-or-later

//! Sidebar: the app's only navigation. Three destinations (Library, Fab,
//! Tasks), then Settings and the account at the foot.

use egui::{Align, CursorIcon, Frame, Layout, Panel, Response, Sense, Ui, Vec2, Widget, pos2};

use super::{colors, focus_ring, paint_icon};
use crate::app::Screen;
use crate::images::{Picture, placeholder};
use crate::text;
use crate::theme::{self, Icon, Text, radius, size, space};

/// What the sidebar shows besides the destinations.
pub struct SidebarView<'a> {
    /// The screen being shown.
    pub current: Screen,
    /// Tasks queued or running; the count bubble is hidden at zero.
    pub active_tasks: usize,
    /// A task failed: the bubble turns `danger`.
    pub tasks_failed: bool,
    /// The UnLauncher mark, once decoded.
    pub brand: Option<Picture>,
    /// The signed-in name (or the app's own prompt when nobody is).
    pub account_name: &'a str,
    /// Which accounts are connected (`Epic · GitHub`).
    pub account_detail: &'a str,
}

/// A navigation row: icon, label and an optional count bubble.
#[must_use = "add it with `ui.add(...)`"]
struct NavItem<'a> {
    icon: Icon,
    label: &'a str,
    selected: bool,
    /// Bubble text and whether it is a failure.
    count: Option<(usize, bool)>,
}

impl Widget for NavItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = colors(ui);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), size::CONTROL_HEIGHT),
            Sense::click(),
        );
        if ui.is_rect_visible(rect) {
            let hovered = response.hovered();
            let (fill, label, icon) = if self.selected {
                (p.accent_soft, p.ink, p.accent_ink)
            } else if hovered {
                (p.bg_300, p.ink, p.ink)
            } else {
                (egui::Color32::TRANSPARENT, p.ink_muted, p.ink_muted)
            };
            ui.painter()
                .rect_filled(rect, theme::corner(radius::MD), fill);
            paint_icon(
                ui,
                self.icon,
                pos2(rect.left() + space::S3 + size::ICON / 2.0, rect.center().y),
                size::ICON,
                icon,
            );
            ui.painter().text(
                pos2(
                    rect.left() + space::S3 + size::ICON + space::S2,
                    rect.center().y,
                ),
                egui::Align2::LEFT_CENTER,
                self.label,
                Text::BodyStrong.font(),
                label,
            );
            if let Some((count, failed)) = self.count {
                bubble(ui, rect, count, failed);
            }
            if response.has_focus() {
                focus_ring(ui, rect, radius::MD);
            }
        }
        response.on_hover_cursor(CursorIcon::PointingHand)
    }
}

/// The count bubble at the right end of a nav row.
fn bubble(ui: &Ui, row: egui::Rect, count: usize, failed: bool) {
    let p = colors(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(count.to_string(), Text::NavCount.font(), p.on_accent);
    let width = (galley.size().x + 2.0 * size::NAV_COUNT_PADDING).max(size::NAV_COUNT);
    let rect = egui::Rect::from_center_size(
        pos2(row.right() - space::S3 - width / 2.0, row.center().y),
        Vec2::new(width, size::NAV_COUNT),
    );
    let fill = if failed { p.danger } else { p.accent };
    ui.painter()
        .rect_filled(rect, theme::corner(radius::FULL), fill);
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.on_accent,
    );
}

/// Draws the sidebar. Returns the screen the user chose, if any.
pub fn sidebar(ui: &mut Ui, view: &SidebarView<'_>) -> Option<Screen> {
    let p = colors(ui);
    let mut chosen = None;
    let frame = Frame::new()
        .fill(p.bg_000)
        .inner_margin(theme::margin_xy(space::S3, space::S4));
    Panel::left("nav")
        .resizable(false)
        .exact_size(size::SIDEBAR_WIDTH)
        .show_separator_line(false)
        .frame(frame)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::S1;
            brand(ui, view.brand);
            let tasks_count =
                (view.active_tasks > 0).then_some((view.active_tasks, view.tasks_failed));
            for (screen, icon, label, count) in [
                (Screen::Library, Icon::Layers, text::NAV_LIBRARY, None),
                (Screen::Fab, Icon::Store, text::NAV_FAB, None),
                (
                    Screen::Tasks,
                    Icon::ListChecks,
                    text::NAV_TASKS,
                    tasks_count,
                ),
            ] {
                let item = NavItem {
                    icon,
                    label,
                    selected: view.current == screen,
                    count,
                };
                if ui.add(item).clicked() {
                    chosen = Some(screen);
                }
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                if account(ui, view).clicked() {
                    chosen = Some(Screen::Settings);
                }
                let item = NavItem {
                    icon: Icon::Settings,
                    label: text::NAV_SETTINGS,
                    selected: view.current == Screen::Settings,
                    count: None,
                };
                if ui.add(item).clicked() {
                    chosen = Some(Screen::Settings);
                }
            });
        });
    chosen
}

/// The mark and the name.
fn brand(ui: &mut Ui, mark: Option<Picture>) {
    let p = colors(ui);
    ui.horizontal(|ui| {
        ui.add_space(space::S2);
        ui.spacing_mut().item_spacing.x = space::S2;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size::BRAND_MARK), Sense::hover());
        if let Some(mark) = mark {
            ui.painter().image(
                mark.id,
                rect,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        ui.label(Text::Brand.colored(text::APP_NAME, p.ink));
    });
    ui.add_space(space::S4);
}

/// The account row: initials in an avatar and which accounts are connected.
fn account(ui: &mut Ui, view: &SidebarView<'_>) -> Response {
    let p = colors(ui);
    let height = size::BRAND_MARK + 2.0 * space::S2;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::corner(radius::MD), p.bg_300);
    }
    let avatar = egui::Rect::from_min_size(
        pos2(
            rect.left() + space::S2,
            rect.center().y - size::BRAND_MARK / 2.0,
        ),
        Vec2::splat(size::BRAND_MARK),
    );
    ui.painter()
        .rect_filled(avatar, theme::corner(radius::FULL), p.thumb[0]);
    ui.painter().text(
        avatar.center(),
        egui::Align2::CENTER_CENTER,
        placeholder::initials(view.account_name),
        Text::Avatar.font(),
        p.ink,
    );
    let left = avatar.right() + space::S2;
    let width = rect.right() - left - space::S2;
    let name = super::one_line(ui, view.account_name, Text::BodyStrong, p.ink, width);
    let detail = super::one_line(ui, view.account_detail, Text::Caption, p.ink_muted, width);
    let block = name.size().y + detail.size().y;
    let top = rect.center().y - block / 2.0;
    let detail_top = top + name.size().y;
    ui.painter().galley(pos2(left, top), name, p.ink);
    ui.painter()
        .galley(pos2(left, detail_top), detail, p.ink_muted);
    if response.has_focus() {
        focus_ring(ui, rect, radius::MD);
    }
    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(text::ACCOUNT_HOVER)
}
