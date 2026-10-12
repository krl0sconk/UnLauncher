// SPDX-License-Identifier: GPL-3.0-or-later

//! Button (primary, secondary, ghost, danger, icon only) and the menu items
//! of the `⋮` card menus. One primary button per surface, at most.

use egui::{Color32, CursorIcon, Popup, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

use super::{colors, faded, focus_ring, paint_icon};
use crate::theme::{self, Icon, Palette, Text, radius, size, space};

/// Which button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `accent` fill: Launch, Install, Connect, Download.
    Primary,
    /// `bg-300` fill with a strong border: Add folder, Cancel.
    Secondary,
    /// No fill until hover: Refresh, Later, the menu trigger.
    Ghost,
    /// `danger` outline: Uninstall, Delete. Always followed by a confirmation.
    Danger,
}

/// The colors of a button in one state: fill, border and label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// Background.
    pub fill: Color32,
    /// Border (transparent for none).
    pub edge: Color32,
    /// Label and icon.
    pub ink: Color32,
}

impl Kind {
    /// How a button of this kind looks when `hovered` (or pressed).
    pub fn look(self, p: &Palette, hovered: bool) -> Look {
        match (self, hovered) {
            (Self::Primary, false) => Look {
                fill: p.accent,
                edge: p.accent_edge,
                ink: p.on_accent,
            },
            (Self::Primary, true) => Look {
                fill: p.accent_hover,
                edge: p.accent_edge,
                ink: p.on_accent,
            },
            (Self::Secondary, false) => Look {
                fill: p.bg_300,
                edge: p.line_strong,
                ink: p.ink,
            },
            (Self::Secondary, true) => Look {
                fill: p.line,
                edge: p.line_strong,
                ink: p.ink,
            },
            (Self::Ghost, false) => Look {
                fill: Color32::TRANSPARENT,
                edge: Color32::TRANSPARENT,
                ink: p.ink_muted,
            },
            (Self::Ghost, true) => Look {
                fill: p.bg_300,
                edge: Color32::TRANSPARENT,
                ink: p.ink,
            },
            (Self::Danger, false) => Look {
                fill: Color32::TRANSPARENT,
                edge: p.danger,
                ink: p.danger,
            },
            (Self::Danger, true) => Look {
                fill: p.danger_soft,
                edge: p.danger,
                ink: p.danger,
            },
        }
    }
}

/// A button with a label and an optional icon before it.
#[must_use = "add it with `ui.add(...)`"]
pub struct Button<'a> {
    kind: Kind,
    label: &'a str,
    icon: Option<Icon>,
    disabled_because: Option<&'a str>,
    fill_width: bool,
}

impl<'a> Button<'a> {
    /// A button of `kind`.
    pub fn new(kind: Kind, label: &'a str) -> Self {
        Self {
            kind,
            label,
            icon: None,
            disabled_because: None,
            fill_width: false,
        }
    }

    /// The one amber button of a surface.
    pub fn primary(label: &'a str) -> Self {
        Self::new(Kind::Primary, label)
    }

    /// The default button.
    pub fn secondary(label: &'a str) -> Self {
        Self::new(Kind::Secondary, label)
    }

    /// A button without fill.
    pub fn ghost(label: &'a str) -> Self {
        Self::new(Kind::Ghost, label)
    }

    /// A destructive button.
    pub fn danger(label: &'a str) -> Self {
        Self::new(Kind::Danger, label)
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Disables the button when `enabled` is false; hovering says
    /// `why_not` ("Sign in to Epic first").
    pub fn enabled(mut self, enabled: bool, why_not: &'a str) -> Self {
        self.disabled_because = (!enabled).then_some(why_not);
        self
    }

    /// Makes the button as wide as the space left in the row.
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = colors(ui);
        let enabled = self.disabled_because.is_none();
        let galley =
            ui.painter()
                .layout_no_wrap(self.label.to_owned(), Text::BodyStrong.font(), p.ink);
        let icon_width = if self.icon.is_some() {
            size::ICON + space::S1
        } else {
            0.0
        };
        let content = icon_width + galley.size().x;
        let width = if self.fill_width {
            ui.available_width()
        } else {
            content + 2.0 * space::S3
        };
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, size::CONTROL_HEIGHT), sense);
        if ui.is_rect_visible(rect) {
            let hovered = enabled && (response.hovered() || response.is_pointer_button_down_on());
            let look = self.kind.look(p, hovered);
            ui.painter().rect(
                rect,
                theme::corner(radius::MD),
                faded(look.fill, enabled),
                Stroke::new(size::STROKE, faded(look.edge, enabled)),
                StrokeKind::Inside,
            );
            let start = rect.center().x - content / 2.0;
            let mut x = start;
            if let Some(icon) = self.icon {
                paint_icon(
                    ui,
                    icon,
                    egui::pos2(x + size::ICON / 2.0, rect.center().y),
                    size::ICON,
                    faded(look.ink, enabled),
                );
                x += icon_width;
            }
            ui.painter().galley(
                egui::pos2(x, rect.center().y - galley.size().y / 2.0),
                galley,
                faded(look.ink, enabled),
            );
            if response.has_focus() {
                focus_ring(ui, rect, radius::MD);
            }
        }
        finish(response, enabled, self.disabled_because)
    }
}

/// Cursor and hover text shared by every button.
fn finish(response: Response, enabled: bool, why_not: Option<&str>) -> Response {
    if enabled {
        return response.on_hover_cursor(CursorIcon::PointingHand);
    }
    let response = response.on_hover_cursor(CursorIcon::NotAllowed);
    match why_not {
        Some(reason) if !reason.is_empty() => response.on_hover_text(reason),
        _ => response,
    }
}

/// A square button with only an icon. The hover text names the action (it
/// is the control's accessible label).
#[must_use = "add it with `ui.add(...)`"]
pub struct IconButton<'a> {
    kind: Kind,
    icon: Icon,
    label: &'a str,
    disabled_because: Option<&'a str>,
}

impl<'a> IconButton<'a> {
    /// A ghost icon button; `label` is shown on hover.
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            kind: Kind::Ghost,
            icon,
            label,
            disabled_because: None,
        }
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = colors(ui);
        let enabled = self.disabled_because.is_none();
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size::CONTROL_HEIGHT), sense);
        if ui.is_rect_visible(rect) {
            let hovered = enabled && (response.hovered() || response.is_pointer_button_down_on());
            let look = self.kind.look(p, hovered);
            ui.painter().rect(
                rect,
                theme::corner(radius::MD),
                faded(look.fill, enabled),
                Stroke::new(size::STROKE, faded(look.edge, enabled)),
                StrokeKind::Inside,
            );
            paint_icon(
                ui,
                self.icon,
                rect.center(),
                size::ICON,
                faded(look.ink, enabled),
            );
            if response.has_focus() {
                focus_ring(ui, rect, radius::MD);
            }
        }
        let response = finish(response, enabled, self.disabled_because);
        if enabled {
            response.on_hover_text(self.label)
        } else {
            response
        }
    }
}

/// One row of a `⋮` menu.
#[must_use = "add it with `ui.add(...)`"]
pub struct MenuItem<'a> {
    label: &'a str,
    icon: Option<Icon>,
    danger: bool,
}

impl<'a> MenuItem<'a> {
    /// A menu row.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            danger: false,
        }
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A destructive row (`danger` text; the action asks for confirmation).
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

impl Widget for MenuItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = colors(ui);
        let ink = if self.danger { p.danger } else { p.ink };
        let galley = ui
            .painter()
            .layout_no_wrap(self.label.to_owned(), Text::Body.font(), ink);
        let width = ui.available_width().max(size::MENU_WIDTH);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, size::CONTROL_HEIGHT), Sense::click());
        if ui.is_rect_visible(rect) {
            if response.hovered() {
                ui.painter()
                    .rect_filled(rect, theme::corner(radius::MD), p.bg_300);
            }
            let mut x = rect.left() + space::S2;
            if let Some(icon) = self.icon {
                paint_icon(
                    ui,
                    icon,
                    egui::pos2(x + size::ICON / 2.0, rect.center().y),
                    size::ICON,
                    ink,
                );
                x += size::ICON + space::S2;
            }
            ui.painter().galley(
                egui::pos2(x, rect.center().y - galley.size().y / 2.0),
                galley,
                ink,
            );
            if response.has_focus() {
                focus_ring(ui, rect, radius::MD);
            }
        }
        response.on_hover_cursor(CursorIcon::PointingHand)
    }
}

/// Shows a menu under `trigger` (an icon button) when it is clicked. Items
/// are `MenuItem`s; a click on one closes the menu.
pub fn menu(trigger: &Response, add_items: impl FnOnce(&mut Ui)) {
    Popup::menu(trigger).width(size::MENU_WIDTH).show(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        add_items(ui);
    });
}

/// The `⋮` button of a card, `rect` big, with its menu. It always takes
/// part in the frame (so an open menu stays anchored) but is only drawn when
/// `visible`, the menu is open or it has keyboard focus.
pub fn overflow_menu(
    ui: &mut Ui,
    rect: egui::Rect,
    id: egui::Id,
    visible: bool,
    label: &str,
    add_items: impl FnOnce(&mut Ui),
) {
    let p = colors(ui);
    let response = ui.interact(rect, id, Sense::click());
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&response));
    if visible || open || response.has_focus() || response.hovered() {
        let look = Kind::Ghost.look(p, response.hovered() || open);
        ui.painter().rect_filled(
            rect,
            theme::corner(radius::MD),
            if look.fill == Color32::TRANSPARENT {
                p.bg_200
            } else {
                look.fill
            },
        );
        paint_icon(
            ui,
            Icon::EllipsisVertical,
            rect.center(),
            size::ICON,
            look.ink,
        );
        if response.has_focus() {
            focus_ring(ui, rect, radius::MD);
        }
    }
    let response = response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(label);
    menu(&response, add_items);
}
