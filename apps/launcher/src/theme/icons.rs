// SPDX-License-Identifier: GPL-3.0-or-later

//! Lucide icons (ISC) from `docs/design/icons.md`. The SVG files draw with
//! `currentColor`; at load that becomes white and the icon is tinted with the
//! token color, so one rasterized SVG serves every color.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use egui::load::Bytes;
use egui::{Color32, Image, ImageSource, Vec2};

macro_rules! icons {
    ($($variant:ident => $file:literal),+ $(,)?) => {
        /// The 26 icons of the design system.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon { $($variant),+ }

        impl Icon {
            /// Every icon.
            pub const ALL: &'static [Icon] = &[$(Icon::$variant),+];

            /// The file name without extension (Lucide's name).
            pub const fn name(self) -> &'static str {
                match self { $(Icon::$variant => $file),+ }
            }

            const fn svg(self) -> &'static str {
                match self {
                    $(Icon::$variant => include_str!(concat!("../../assets/icons/", $file, ".svg"))),+
                }
            }
        }
    };
}

icons! {
    Layers => "layers",
    Store => "store",
    ListChecks => "list-checks",
    Settings => "settings",
    Box => "box",
    Package => "package",
    Play => "play",
    Pause => "pause",
    Download => "download",
    GitBranch => "git-branch",
    Hammer => "hammer",
    FolderOpen => "folder-open",
    FolderSearch => "folder-search",
    HardDrive => "hard-drive",
    Plus => "plus",
    RefreshCw => "refresh-cw",
    Search => "search",
    EllipsisVertical => "ellipsis-vertical",
    ExternalLink => "external-link",
    LogIn => "log-in",
    User => "user",
    Check => "check",
    CircleCheck => "circle-check",
    TriangleAlert => "triangle-alert",
    X => "x",
    Trash2 => "trash-2",
}

impl Icon {
    /// The SVG with `currentColor` replaced by white.
    fn white_svg(self) -> String {
        self.svg().replace("currentColor", "#ffffff")
    }

    fn bytes(self) -> Arc<[u8]> {
        static CACHE: OnceLock<Vec<Arc<[u8]>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| {
            Icon::ALL
                .iter()
                .map(|icon| Arc::from(icon.white_svg().into_bytes()))
                .collect()
        });
        let index = Icon::ALL.iter().position(|icon| *icon == self).unwrap_or(0);
        Arc::clone(&cache[index])
    }

    /// The image source for egui's loaders.
    fn source(self) -> ImageSource<'static> {
        ImageSource::Bytes {
            uri: Cow::Owned(format!("bytes://icons/{}.svg", self.name())),
            bytes: Bytes::Shared(self.bytes()),
        }
    }

    /// The icon `size` points wide, tinted with `color`.
    pub fn image(self, size: f32, color: Color32) -> Image<'static> {
        Image::new(self.source())
            .fit_to_exact_size(Vec2::splat(size))
            .tint(color)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn there_are_26_icons_and_all_are_in_the_design_list() {
        assert_eq!(Icon::ALL.len(), 26);
        let list = include_str!("../../assets/icons/icons.md");
        for icon in Icon::ALL {
            assert!(
                list.contains(&format!("`{}`", icon.name())),
                "{}",
                icon.name()
            );
        }
    }

    #[test]
    fn svgs_are_drawn_in_white() {
        for icon in Icon::ALL {
            let svg = icon.white_svg();
            assert!(svg.contains("<svg"), "{}", icon.name());
            assert!(!svg.contains("currentColor"), "{}", icon.name());
            assert!(svg.contains("#ffffff"), "{}", icon.name());
        }
    }
}
