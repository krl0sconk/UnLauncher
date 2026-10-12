// SPDX-License-Identifier: GPL-3.0-or-later

//! The visual theme, written by hand from `assets/tokens.json`. It is the
//! only place with colors, sizes and spaces: the interface reads them from
//! here. CSS px are egui points.
//!
//! The `tokens_match_design_system` test compares this file against
//! `tokens.json`, so a change to either one without the other fails.

#![allow(
    clippy::unreadable_literal,
    reason = "colors are written as in tokens.json"
)]

use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, RichText, Shadow, Stroke, Style, Theme,
};

mod fonts;
mod icons;

pub use fonts::install_fonts;
pub use icons::Icon;

use crate::settings::ThemeChoice;

/// Every color token, for one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Sidebar and window frame.
    pub bg_000: Color32,
    /// Content area behind every screen.
    pub bg_100: Color32,
    /// Cards, task rows, dialogs, inputs.
    pub bg_200: Color32,
    /// Hover and pressed fill, selected nav item, secondary buttons.
    pub bg_300: Color32,
    /// Hairline dividers and card edges.
    pub line: Color32,
    /// Borders that carry meaning: inputs, secondary buttons.
    pub line_strong: Color32,
    /// Primary text and icons.
    pub ink: Color32,
    /// Secondary text.
    pub ink_muted: Color32,
    /// Tertiary text (not on `bg_300`).
    pub ink_faint: Color32,
    /// The one accent: primary buttons, progress, the Tasks count.
    pub accent: Color32,
    /// Hover fill of primary buttons.
    pub accent_hover: Color32,
    /// Border of accent fills.
    pub accent_edge: Color32,
    /// Text and icons on an accent fill.
    pub on_accent: Color32,
    /// Amber used as text.
    pub accent_ink: Color32,
    /// Tint behind the current nav item and notice banners.
    pub accent_soft: Color32,
    /// Keyboard focus ring.
    pub focus_ring: Color32,
    /// Done, installed.
    pub success: Color32,
    /// Missing engine, low space, paused.
    pub warning: Color32,
    /// Failed, destructive.
    pub danger: Color32,
    /// Fill behind a success badge.
    pub success_soft: Color32,
    /// Fill behind a warning badge.
    pub warning_soft: Color32,
    /// Fill behind a danger badge.
    pub danger_soft: Color32,
    /// Placeholder thumbnail fills, `thumb-1` to `thumb-5`.
    pub thumb: [Color32; 5],
    /// Behind dialogs.
    pub scrim: Color32,
    /// Color of the `shadow-dialog` token.
    pub shadow: Color32,
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "each channel is one byte of `hex`"
)]
const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// The dark theme (the primary one).
pub const DARK: Palette = Palette {
    bg_000: rgb(0x0d0c0b),
    bg_100: rgb(0x141312),
    bg_200: rgb(0x1c1a19),
    bg_300: rgb(0x262322),
    line: rgb(0x3a3532),
    line_strong: rgb(0x6e6761),
    ink: rgb(0xefece8),
    ink_muted: rgb(0xaaa39c),
    ink_faint: rgb(0x8a837c),
    accent: rgb(0xf5a524),
    accent_hover: rgb(0xffb84a),
    accent_edge: rgb(0xf5a524),
    on_accent: rgb(0x1c1204),
    accent_ink: rgb(0xf8b443),
    accent_soft: rgba(245, 165, 36, 36),
    focus_ring: rgb(0xffc35c),
    success: rgb(0x4fc98e),
    warning: rgb(0xff9a4d),
    danger: rgb(0xff6b6b),
    success_soft: rgba(79, 201, 142, 36),
    warning_soft: rgba(255, 154, 77, 36),
    danger_soft: rgba(255, 107, 107, 36),
    thumb: [
        rgb(0x3b3027),
        rgb(0x2f3a33),
        rgb(0x2e3340),
        rgb(0x3d2c33),
        rgb(0x33302a),
    ],
    scrim: rgba(8, 7, 6, 184),
    shadow: rgba(0, 0, 0, 140),
};

/// The light theme.
pub const LIGHT: Palette = Palette {
    bg_000: rgb(0xe8e4df),
    bg_100: rgb(0xf4f1ed),
    bg_200: rgb(0xffffff),
    bg_300: rgb(0xece8e3),
    line: rgb(0xcfc8c0),
    line_strong: rgb(0x8c847c),
    ink: rgb(0x1b1816),
    ink_muted: rgb(0x544e49),
    ink_faint: rgb(0x6b645e),
    accent: rgb(0xe5940f),
    accent_hover: rgb(0xf0a530),
    accent_edge: rgb(0x9a5a00),
    on_accent: rgb(0x1c1204),
    accent_ink: rgb(0x8f5400),
    accent_soft: rgba(229, 148, 15, 41),
    focus_ring: rgb(0x9a5a00),
    success: rgb(0x1f7a4d),
    warning: rgb(0xb04f00),
    danger: rgb(0xc42b2b),
    success_soft: rgba(31, 122, 77, 31),
    warning_soft: rgba(176, 79, 0, 31),
    danger_soft: rgba(196, 43, 43, 31),
    thumb: [
        rgb(0xe9dccb),
        rgb(0xd6e3d9),
        rgb(0xd7dce8),
        rgb(0xead6dc),
        rgb(0xe4e0d6),
    ],
    scrim: rgba(27, 24, 22, 115),
    shadow: rgba(27, 24, 22, 46),
};

/// The palette that matches the theme `ui` is drawn in.
pub fn palette(ctx: &Context) -> &'static Palette {
    if ctx.global_style().visuals.dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

/// Spacing tokens.
pub mod space {
    /// Icon-to-label gap inside buttons and badges.
    pub const S1: f32 = 4.0;
    /// Gap between related controls; badge padding.
    pub const S2: f32 = 8.0;
    /// Card padding; gap between cards in a grid.
    pub const S3: f32 = 12.0;
    /// Gap between a section label and its grid; dialog padding.
    pub const S4: f32 = 16.0;
    /// Screen padding; gap between sections.
    pub const S6: f32 = 24.0;
    /// Top margin of a screen title; empty-state padding.
    pub const S8: f32 = 32.0;
}

/// Radius tokens.
pub mod radius {
    /// Badges, progress tracks, checkboxes.
    pub const SM: u8 = 4;
    /// Buttons, inputs, nav items, task rows.
    pub const MD: u8 = 6;
    /// Cards, thumbnails, dialogs, banners.
    pub const LG: u8 = 10;
    /// Avatars and count bubbles.
    pub const FULL: u8 = 255;
}

/// Size tokens and the measures `reference/components.css` gives each
/// component.
pub mod size {
    /// Fixed width of the sidebar.
    pub const SIDEBAR_WIDTH: f32 = 200.0;
    /// Engine card width.
    pub const CARD_ENGINE_WIDTH: f32 = 176.0;
    /// Minimum project and asset card width.
    pub const CARD_MIN_WIDTH: f32 = 200.0;
    /// Buttons, inputs and nav items.
    pub const CONTROL_HEIGHT: f32 = 32.0;
    /// Icons in buttons, nav and badges.
    pub const ICON: f32 = 16.0;
    /// Icons in thumbnails and empty states.
    pub const ICON_LG: f32 = 24.0;
    /// Icons inside a badge.
    pub const ICON_BADGE: f32 = 12.0;
    /// Badge height.
    pub const BADGE_HEIGHT: f32 = 20.0;
    /// The brand mark and the account avatar in the sidebar.
    pub const BRAND_MARK: f32 = 28.0;
    /// Count bubble on the Tasks nav item.
    pub const NAV_COUNT: f32 = 20.0;
    /// Horizontal padding of the count bubble.
    pub const NAV_COUNT_PADDING: f32 = 6.0;
    /// Kind icon tile on a task row.
    pub const TASK_ICON: f32 = 40.0;
    /// Progress bar height.
    pub const PROGRESS: f32 = 6.0;
    /// Icon tile on an empty state.
    pub const EMPTY_MARK: f32 = 56.0;
    /// Widest text in an empty state, in points (42 characters).
    pub const EMPTY_TEXT_WIDTH: f32 = 300.0;
    /// Maximum dialog width.
    pub const DIALOG_WIDTH: f32 = 480.0;
    /// Share of the track an unknown progress segment covers.
    pub const PROGRESS_SEGMENT: f32 = 0.3;
    /// Gap between a card's title line and its meta line.
    pub const CARD_TEXT_GAP: f32 = 2.0;
    /// Height of the template list in the New project dialog.
    pub const TEMPLATE_LIST_HEIGHT: f32 = 180.0;
    /// Width of the template picture in the New project dialog.
    pub const TEMPLATE_PREVIEW_WIDTH: f32 = 240.0;
    /// Width of the Fab search field.
    pub const SEARCH_WIDTH: f32 = 360.0;
    /// Width of a card menu.
    pub const MENU_WIDTH: f32 = 200.0;
    /// Height of the "Install engine" card, as in the reference.
    pub const ENGINE_ADD_MIN_HEIGHT: f32 = 148.0;
    /// Width of the thumbnail ratio (16:9).
    pub const THUMB_RATIO: f32 = 16.0 / 9.0;
    /// Width of a step dot in a dialog's step list.
    pub const STEP_DOT: f32 = 24.0;
    /// Border width of cards, inputs and buttons.
    pub const STROKE: f32 = 1.0;
    /// Focus ring width and gap.
    pub const FOCUS: f32 = 2.0;
    /// The same, as a corner radius step.
    pub const FOCUS_RADIUS: u8 = 2;
    /// Disabled controls keep their layout at this opacity.
    pub const DISABLED_OPACITY: f32 = 0.45;
    /// Widest the content of a screen grows.
    pub const CONTENT_MAX_WIDTH: f32 = 1400.0;
    /// Default window size.
    pub const WINDOW: [f32; 2] = [1100.0, 700.0];
    /// Smallest window size.
    pub const WINDOW_MIN: [f32; 2] = [760.0, 480.0];
}

/// A margin of `points` on every side.
#[allow(
    clippy::cast_possible_truncation,
    reason = "margins are small whole numbers"
)]
pub fn margin(points: f32) -> egui::Margin {
    egui::Margin::same(points as i8)
}

/// A margin of `x` points left and right and `y` points above and below.
#[allow(
    clippy::cast_possible_truncation,
    reason = "margins are small whole numbers"
)]
pub fn margin_xy(x: f32, y: f32) -> egui::Margin {
    egui::Margin::symmetric(x as i8, y as i8)
}

/// A corner radius for [`radius`] values.
pub fn corner(value: u8) -> CornerRadius {
    CornerRadius::same(value)
}

/// The text styles of `tokens.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    /// The engine version on an engine card (40 px, 800).
    VersionNumeral,
    /// Screen title (24 px, 700).
    PageTitle,
    /// Section header (12 px, 700, uppercase).
    SectionLabel,
    /// Names on cards, dialog titles, task titles (15 px, 600).
    CardTitle,
    /// Running text (14 px, 400).
    Body,
    /// Button and nav labels (14 px, 600).
    BodyStrong,
    /// Metadata (12 px, 500).
    Caption,
    /// Paths, git refs and log lines (12 px mono).
    Mono,
    /// Live numbers (13 px mono, 500).
    MonoFigures,
    /// Badge label (11 px, 600, uppercase; `components.css` `.ul-badge`).
    Badge,
    /// Count bubble on the Tasks nav item (11 px, 700).
    NavCount,
    /// Initials on a thumbnail placeholder (28 px, 800).
    Initials,
    /// Initials in the account avatar (12 px, 700).
    Avatar,
    /// A code the user must type (22 px mono, 500).
    Code,
    /// The name next to the mark in the sidebar (16 px, 800).
    Brand,
    /// The warning strip on a project card (12 px, 600).
    Strip,
}

impl Text {
    /// Font size in points.
    pub const fn size(self) -> f32 {
        match self {
            Self::VersionNumeral => 40.0,
            Self::PageTitle => 24.0,
            Self::SectionLabel | Self::Caption | Self::Mono | Self::Avatar | Self::Strip => 12.0,
            Self::CardTitle => 15.0,
            Self::Brand => 16.0,
            Self::Badge | Self::NavCount => 11.0,
            Self::Initials => 28.0,
            Self::Code => 22.0,
            Self::Body | Self::BodyStrong => 14.0,
            Self::MonoFigures => 13.0,
        }
    }

    /// Line height in points.
    pub const fn line_height(self) -> f32 {
        match self {
            Self::VersionNumeral => 40.0,
            Self::Initials | Self::Code => 28.0,
            Self::PageTitle => 30.0,
            Self::SectionLabel | Self::Caption | Self::Mono | Self::Strip | Self::Badge => 16.0,
            Self::NavCount | Self::CardTitle | Self::Body | Self::BodyStrong | Self::Brand => 20.0,
            Self::Avatar => 12.0,
            Self::MonoFigures => 18.0,
        }
    }

    /// The font family: one per weight.
    pub fn family(self) -> FontFamily {
        FontFamily::Name(
            match self {
                Self::VersionNumeral | Self::Initials | Self::Brand => "archivo-800",
                Self::PageTitle | Self::SectionLabel | Self::NavCount | Self::Avatar => {
                    "archivo-700"
                }
                Self::CardTitle | Self::BodyStrong | Self::Badge | Self::Strip => "archivo-600",
                Self::Body => "archivo-400",
                Self::Caption => "archivo-500",
                Self::Mono => "mono-400",
                Self::MonoFigures | Self::Code => "mono-500",
            }
            .into(),
        )
    }

    /// The font to give a widget.
    pub fn font(self) -> FontId {
        FontId::new(self.size(), self.family())
    }

    /// `text` in this style, without a color (the widget's text color
    /// applies). Section labels are uppercased.
    pub fn rich(self, text: impl Into<String>) -> RichText {
        let text = text.into();
        let text = if matches!(self, Self::SectionLabel | Self::Badge) {
            text.to_uppercase()
        } else {
            text
        };
        RichText::new(text)
            .font(self.font())
            .line_height(Some(self.line_height()))
    }

    /// `text` in this style and color.
    pub fn colored(self, text: impl Into<String>, color: Color32) -> RichText {
        self.rich(text).color(color)
    }
}

/// Applies the theme to `ctx`: fonts, the dark and light styles, and which
/// one is used.
pub fn apply(ctx: &Context, choice: ThemeChoice) {
    install_fonts(ctx);
    ctx.set_style_of(Theme::Dark, style(&DARK, true));
    ctx.set_style_of(Theme::Light, style(&LIGHT, false));
    set_choice(ctx, choice);
}

/// Changes only which theme is used.
pub fn set_choice(ctx: &Context, choice: ThemeChoice) {
    ctx.set_theme(match choice {
        ThemeChoice::System => egui::ThemePreference::System,
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
    });
}

/// The egui style for one palette.
#[allow(
    clippy::cast_possible_truncation,
    reason = "margins are small whole numbers"
)]
fn style(p: &Palette, dark: bool) -> Style {
    let mut style = Style::default();

    style.text_styles = [
        (egui::TextStyle::Heading, Text::PageTitle),
        (egui::TextStyle::Body, Text::Body),
        (egui::TextStyle::Button, Text::BodyStrong),
        (egui::TextStyle::Small, Text::Caption),
        (egui::TextStyle::Monospace, Text::Mono),
    ]
    .into_iter()
    .map(|(style, text)| (style, text.font()))
    .collect();

    style.spacing.item_spacing = egui::vec2(space::S2, space::S2);
    style.spacing.button_padding = egui::vec2(space::S3, 0.0);
    style.spacing.interact_size.y = size::CONTROL_HEIGHT;
    style.spacing.window_margin = egui::Margin::same(space::S4 as i8);
    style.spacing.menu_margin = egui::Margin::same(space::S1 as i8);
    style.spacing.icon_width = size::ICON;
    style.spacing.icon_width_inner = size::ICON_BADGE;
    style.spacing.scroll.floating = true;

    let v = &mut style.visuals;
    v.dark_mode = dark;
    v.override_text_color = None;
    v.panel_fill = p.bg_100;
    v.window_fill = p.bg_200;
    v.window_stroke = Stroke::new(size::STROKE, p.line);
    v.window_corner_radius = corner(radius::LG);
    v.menu_corner_radius = corner(radius::MD);
    v.extreme_bg_color = p.bg_200;
    v.text_edit_bg_color = Some(p.bg_200);
    v.faint_bg_color = p.bg_200;
    v.code_bg_color = p.bg_300;
    v.hyperlink_color = p.accent_ink;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(size::STROKE, p.accent_ink);
    v.striped = false;
    v.image_loading_spinners = false;
    v.disabled_alpha = size::DISABLED_OPACITY;
    // Only dialogs and menus cast a shadow.
    let dialog_shadow = Shadow {
        offset: [0, 16],
        blur: 48,
        spread: 0,
        color: p.shadow,
    };
    v.window_shadow = dialog_shadow;
    v.popup_shadow = dialog_shadow;

    let widget = |fill: Color32, edge: Stroke, text: Color32| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: edge,
        corner_radius: corner(radius::MD),
        fg_stroke: Stroke::new(size::STROKE, text),
        expansion: 0.0,
    };
    v.widgets.noninteractive = widget(p.bg_100, Stroke::new(size::STROKE, p.line), p.ink);
    v.widgets.inactive = widget(p.bg_300, Stroke::new(size::STROKE, p.line_strong), p.ink);
    v.widgets.hovered = widget(p.line, Stroke::new(size::STROKE, p.line_strong), p.ink);
    v.widgets.active = widget(p.line, Stroke::new(size::STROKE, p.line_strong), p.ink);
    v.widgets.open = widget(p.bg_300, Stroke::new(size::STROKE, p.line_strong), p.ink);
    style
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    const TOKENS: &str = include_str!("../assets/tokens.json");

    fn hex_or_rgba(value: &str) -> Color32 {
        if let Some(hex) = value.strip_prefix('#') {
            return rgb(u32::from_str_radix(hex, 16).unwrap());
        }
        let inner = value
            .strip_prefix("rgba(")
            .and_then(|v| v.strip_suffix(')'))
            .unwrap();
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let alpha = (parts[3].parse::<f32>().unwrap() * 255.0).round() as u8;
        rgba(
            parts[0].parse().unwrap(),
            parts[1].parse().unwrap(),
            parts[2].parse().unwrap(),
            alpha,
        )
    }

    fn field(p: &Palette, name: &str) -> Color32 {
        match name {
            "bg-000" => p.bg_000,
            "bg-100" => p.bg_100,
            "bg-200" => p.bg_200,
            "bg-300" => p.bg_300,
            "line" => p.line,
            "line-strong" => p.line_strong,
            "ink" => p.ink,
            "ink-muted" => p.ink_muted,
            "ink-faint" => p.ink_faint,
            "accent" => p.accent,
            "accent-hover" => p.accent_hover,
            "accent-edge" => p.accent_edge,
            "on-accent" => p.on_accent,
            "accent-ink" => p.accent_ink,
            "accent-soft" => p.accent_soft,
            "focus-ring" => p.focus_ring,
            "success" => p.success,
            "warning" => p.warning,
            "danger" => p.danger,
            "success-soft" => p.success_soft,
            "warning-soft" => p.warning_soft,
            "danger-soft" => p.danger_soft,
            "thumb-1" => p.thumb[0],
            "thumb-2" => p.thumb[1],
            "thumb-3" => p.thumb[2],
            "thumb-4" => p.thumb[3],
            "thumb-5" => p.thumb[4],
            "scrim" => p.scrim,
            other => panic!("token {other} has no field in Palette"),
        }
    }

    #[test]
    fn tokens_match_design_system() {
        let tokens: serde_json::Value = serde_json::from_str(TOKENS).unwrap();
        let colors = tokens["color"]["tokens"].as_array().unwrap();
        assert_eq!(colors.len(), 28, "a color token was added or removed");
        for token in colors {
            let name = token["name"].as_str().unwrap();
            for (theme, palette) in [("dark", &DARK), ("light", &LIGHT)] {
                let expected = hex_or_rgba(token["value"][theme].as_str().unwrap());
                assert_eq!(field(palette, name), expected, "{name} in {theme}");
            }
        }
    }

    #[test]
    fn metrics_match_design_system() {
        let tokens: serde_json::Value = serde_json::from_str(TOKENS).unwrap();
        let px = |group: &str, name: &str| -> f32 {
            tokens[group]["tokens"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("no token {name}"))["value"]
                .as_str()
                .unwrap()
                .trim_end_matches("px")
                .parse()
                .unwrap()
        };
        assert_eq!(px("spacing", "space-1"), space::S1);
        assert_eq!(px("spacing", "space-2"), space::S2);
        assert_eq!(px("spacing", "space-3"), space::S3);
        assert_eq!(px("spacing", "space-4"), space::S4);
        assert_eq!(px("spacing", "space-6"), space::S6);
        assert_eq!(px("spacing", "space-8"), space::S8);
        assert_eq!(px("radius", "radius-sm"), f32::from(radius::SM));
        assert_eq!(px("radius", "radius-md"), f32::from(radius::MD));
        assert_eq!(px("radius", "radius-lg"), f32::from(radius::LG));
        assert_eq!(px("size", "sidebar-width"), size::SIDEBAR_WIDTH);
        assert_eq!(px("size", "card-engine-width"), size::CARD_ENGINE_WIDTH);
        assert_eq!(px("size", "card-min-width"), size::CARD_MIN_WIDTH);
        assert_eq!(px("size", "control-height"), size::CONTROL_HEIGHT);
        assert_eq!(px("size", "icon-size"), size::ICON);
    }

    #[test]
    fn text_styles_match_design_system() {
        let tokens: serde_json::Value = serde_json::from_str(TOKENS).unwrap();
        let styles: Vec<&serde_json::Value> = tokens["type"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["styles"].as_array().unwrap())
            .collect();
        assert_eq!(styles.len(), 9);
        for (name, text, weight) in [
            ("version-numeral", Text::VersionNumeral, 800),
            ("page-title", Text::PageTitle, 700),
            ("section-label", Text::SectionLabel, 700),
            ("card-title", Text::CardTitle, 600),
            ("body", Text::Body, 400),
            ("body-strong", Text::BodyStrong, 600),
            ("caption", Text::Caption, 500),
            ("mono", Text::Mono, 400),
            ("mono-figures", Text::MonoFigures, 500),
        ] {
            let style = styles.iter().find(|s| s["name"] == name).unwrap();
            let px = |key: &str| -> f32 {
                style[key]
                    .as_str()
                    .unwrap()
                    .trim_end_matches("px")
                    .parse()
                    .unwrap()
            };
            assert_eq!(px("fontSize"), text.size(), "{name} size");
            assert_eq!(px("lineHeight"), text.line_height(), "{name} line height");
            assert_eq!(style["fontWeight"], weight, "{name} weight");
            let family = match text.family() {
                FontFamily::Name(family) => family.to_string(),
                other => panic!("{other:?}"),
            };
            assert!(family.ends_with(&weight.to_string()), "{name}: {family}");
        }
    }

    #[test]
    fn dark_style_uses_the_palette() {
        let style = style(&DARK, true);
        assert!(style.visuals.dark_mode);
        assert_eq!(style.visuals.panel_fill, DARK.bg_100);
        assert_eq!(style.visuals.selection.bg_fill, DARK.accent_soft);
        assert_eq!(style.visuals.window_shadow.blur, 48);
    }
}
