// SPDX-License-Identifier: GPL-3.0-or-later

//! Fab: the user's Fab library as a grid of cards, and the sign-in that
//! comes before it.

use egui::{Id, Ui};
use fab::{AssetKind, LibraryAsset};

use super::widgets::asset_card::{self, AssetCard, asset_card};
use super::widgets::badge::filter_chip;
use super::widgets::button::Button;
use super::widgets::empty_state::empty_state;
use super::widgets::gap;
use super::widgets::search_field::search_field;
use super::widgets::text_field::secret_field;
use super::widgets::{card_grid, colors};
use super::{error_box, page_header};
use crate::app::{
    EpicState, FabWindow, Intent, KindFilter, LauncherApp, LibraryState, LoginMode, LoginWindow,
};
use crate::images::ImageKey;
use crate::text;
use crate::theme::{Icon, Text, size, space};

pub fn draw(app: &mut LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let signed_in = app.library.epic.is_signed_in();
    let fab_window = app.library.fab_window;
    page_header(ui, app, text::PAGE_FAB, |ui| {
        if signed_in {
            if ui
                .add(
                    Button::primary(text::GET_MORE_ON_FAB)
                        .icon(Icon::ExternalLink)
                        .enabled(
                            !matches!(fab_window, FabWindow::Opening | FabWindow::Open),
                            text::FAB_WINDOW_OPEN,
                        ),
                )
                .clicked()
            {
                intents.push(Intent::GetMoreOnFab);
            }
            if ui
                .add(Button::ghost(text::REFRESH).icon(Icon::RefreshCw))
                .clicked()
            {
                intents.push(Intent::RefreshLibrary);
            }
        }
    });
    let library = &mut app.library;
    if let Some(error) = &library.error {
        error_box(ui, error);
        ui.add_space(space::S3);
    }
    match &library.epic {
        EpicState::Restoring | EpicState::SigningIn => {
            ui.spinner();
        }
        EpicState::SignedOut => sign_in(library, ui, intents),
        EpicState::SignedIn(_) | EpicState::Demo(_) => assets(&mut *app, ui, intents),
    }
}

fn sign_in(library: &mut LibraryState, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    if let Some(notice) = &library.notice {
        ui.label(Text::Body.colored(notice, p.warning));
        ui.add_space(space::S3);
    }
    if library.login_mode == LoginMode::Window {
        window_sign_in(library, ui, intents);
    } else {
        browser_sign_in(library, ui, intents);
    }
    gap(ui, space::S4);
    ui.label(Text::Caption.colored(text::UNOFFICIAL_API, p.ink_faint));
}

/// Sign in with the built-in window (the default).
fn window_sign_in(library: &LibraryState, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    match &library.login_window {
        LoginWindow::Closed => {
            empty_state(
                ui,
                Icon::Store,
                text::EMPTY_FAB_SIGNED_OUT_TITLE,
                text::EMPTY_FAB_SIGNED_OUT_BODY,
                |ui| {
                    if ui
                        .add(Button::primary(text::SIGN_IN_WITH_EPIC).icon(Icon::LogIn))
                        .clicked()
                    {
                        intents.push(Intent::SignInWithWindow);
                    }
                    ui.add_space(space::S2);
                    ui.label(Text::Caption.colored(text::SIGN_IN_WINDOW_HELP, p.ink_faint));
                },
            );
        }
        LoginWindow::Starting => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(Text::Body.colored(text::OPENING_WINDOW, p.ink_muted));
            });
        }
        LoginWindow::Open(_) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(Text::Body.colored(text::SIGN_IN_WAITING, p.ink_muted));
            });
            ui.add_space(space::S2);
            if ui.add(Button::secondary(text::CANCEL)).clicked() {
                intents.push(Intent::CancelWindowLogin);
            }
        }
    }
    ui.add_space(space::S3);
    if ui.link(text::USE_BROWSER_LOGIN).clicked() {
        intents.push(Intent::UseBrowserLogin);
    }
}

/// Sign in through the system browser by pasting the code Epic shows.
fn browser_sign_in(library: &mut LibraryState, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    if library.notice.is_none() {
        ui.label(Text::Body.colored(text::SIGN_IN_STEP_1, p.ink));
        if ui
            .add(Button::secondary(text::OPEN_LOGIN_PAGE).icon(Icon::ExternalLink))
            .clicked()
        {
            intents.push(Intent::OpenLoginPage);
        }
        ui.add_space(space::S3);
    }
    ui.label(Text::Body.colored(text::BROWSER_STEP_COPY, p.ink_muted));
    ui.horizontal(|ui| {
        let field = secret_field(ui, &mut library.login_code);
        // Pasting a valid code is enough; no need to press the button.
        let pasted_code =
            field.changed() && epic_auth::parse_authorization_code(&library.login_code).is_ok();
        let clicked = ui
            .add(
                Button::primary(text::SIGN_IN)
                    .icon(Icon::LogIn)
                    .enabled(!library.login_code.trim().is_empty(), text::PASTE_THE_CODE),
            )
            .clicked();
        if pasted_code || clicked {
            intents.push(Intent::SignIn);
        }
    });
    if library.login_mode != LoginMode::NoWindow {
        ui.add_space(space::S3);
        if ui.link(text::USE_WINDOW_LOGIN).clicked() {
            intents.push(Intent::UseWindowLogin);
        }
    }
}

#[allow(clippy::too_many_lines, reason = "one screen, drawn top to bottom")]
fn assets(app: &mut LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    match app.library.fab_window {
        FabWindow::Open | FabWindow::Opening => {
            ui.label(Text::Body.colored(text::FAB_WINDOW_OPEN, p.ink_muted));
            ui.add_space(space::S3);
        }
        FabWindow::InBrowser => {
            ui.label(Text::Body.colored(text::FAB_IN_BROWSER, p.warning));
            ui.add_space(space::S3);
        }
        FabWindow::Closed => {}
    }
    let images = app.images.clone();
    let library = &mut app.library;
    let (all, count) = match &library.assets {
        None => {
            ui.spinner();
            return;
        }
        Some(Err(error)) => {
            error_box(ui, error);
            return;
        }
        Some(Ok(assets)) => {
            let filter = library.filter.trim().to_lowercase();
            let shown = assets
                .iter()
                .filter(|asset| library.kind_filter.accepts(asset.kind))
                .filter(|asset| filter.is_empty() || asset.title.to_lowercase().contains(&filter))
                .count();
            (assets.len(), shown)
        }
    };
    if all == 0 && !library.loading {
        empty_state(
            ui,
            Icon::Store,
            text::EMPTY_FAB_TITLE,
            text::EMPTY_FAB_BODY,
            |ui| {
                if ui
                    .add(Button::primary(text::GET_MORE_ON_FAB).icon(Icon::ExternalLink))
                    .clicked()
                {
                    intents.push(Intent::GetMoreOnFab);
                }
            },
        );
        return;
    }

    ui.horizontal(|ui| {
        let width = ui.available_width().min(size::SEARCH_WIDTH);
        search_field(ui, &mut library.filter, text::FAB_SEARCH_HINT, width);
        ui.add_space(space::S1);
        for (filter, label) in [
            (KindFilter::All, text::FILTER_ALL),
            (KindFilter::AssetPacks, text::FILTER_ASSET_PACKS),
            (KindFilter::Plugins, text::FILTER_PLUGINS),
        ] {
            if filter_chip(ui, label, library.kind_filter == filter).clicked() {
                library.kind_filter = filter;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if library.loading {
                ui.spinner();
            }
            let noun = if count == 1 { text::ITEM } else { text::ITEMS };
            ui.label(Text::Caption.colored(format!("{count} {noun}"), p.ink_muted));
        });
    });
    gap(ui, space::S4);

    let Some(Ok(assets)) = &library.assets else {
        return;
    };
    let filter = library.filter.trim().to_lowercase();
    let shown: Vec<&LibraryAsset> = assets
        .iter()
        .filter(|asset| library.kind_filter.accepts(asset.kind))
        .filter(|asset| filter.is_empty() || asset.title.to_lowercase().contains(&filter))
        .collect();
    if shown.is_empty() {
        ui.label(Text::Body.colored(text::NO_MATCHES, p.ink_muted));
        return;
    }
    card_grid(
        ui,
        shown.len(),
        size::CARD_MIN_WIDTH,
        space::S3,
        asset_card::height_for,
        |ui, index, rect| {
            let asset = shown[index];
            let picture = asset
                .thumbnail_url
                .as_ref()
                .and_then(|url| images.texture(&ImageKey::Remote(url.clone())));
            let meta = meta(asset);
            let card = AssetCard {
                title: &asset.title,
                meta: &meta,
                kind_icon: kind_icon(asset.kind),
                picture,
                in_project: false,
            };
            if asset_card(ui, Id::new(("asset", &asset.id)), rect, &card) {
                intents.push(Intent::InstallAsset(asset.clone()));
            }
        },
    );
}

/// The icon of an asset's kind: `box` for plugins, `package` for the rest.
fn kind_icon(kind: AssetKind) -> Icon {
    match kind {
        AssetKind::Plugin => Icon::Box,
        AssetKind::Content | AssetKind::Project | AssetKind::Unknown => Icon::Package,
    }
}

fn kind_label(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Plugin => text::KIND_PLUGIN,
        AssetKind::Project => text::KIND_PROJECT,
        AssetKind::Content | AssetKind::Unknown => text::KIND_ASSET_PACK,
    }
}

/// `Asset pack · UE 5.0–5.7`.
pub fn meta(asset: &LibraryAsset) -> String {
    let range = version_range(asset);
    if range.is_empty() {
        kind_label(asset.kind).to_owned()
    } else {
        format!("{} · UE {range}", kind_label(asset.kind))
    }
}

/// The engine versions an asset supports, as a compact range (`4.26–5.4`).
pub fn version_range(asset: &LibraryAsset) -> String {
    let mut versions: Vec<(u32, u32)> = asset
        .versions
        .iter()
        .flat_map(|v| v.engine_versions.iter().copied())
        .collect();
    versions.sort_unstable();
    versions.dedup();
    match (versions.first(), versions.last()) {
        (Some(first), Some(last)) if first == last => format!("{}.{}", first.0, first.1),
        (Some(first), Some(last)) => format!("{}.{}–{}.{}", first.0, first.1, last.0, last.1),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(kind: AssetKind, versions: &[(u32, u32)]) -> LibraryAsset {
        LibraryAsset {
            id: "a".to_owned(),
            namespace: "n".to_owned(),
            title: "T".to_owned(),
            kind,
            versions: vec![fab::AssetVersion {
                id: "v".to_owned(),
                engine_versions: versions.to_vec(),
                title: None,
            }],
            thumbnail_url: None,
        }
    }

    #[test]
    fn the_meta_line_names_the_kind_and_the_engine_range() {
        let pack = asset(AssetKind::Content, &[(5, 7), (5, 0), (5, 3)]);
        assert_eq!(meta(&pack), "Asset pack · UE 5.0–5.7");
        let plugin = asset(AssetKind::Plugin, &[(5, 6)]);
        assert_eq!(meta(&plugin), "Plugin · UE 5.6");
        assert_eq!(meta(&asset(AssetKind::Unknown, &[])), "Asset pack");
    }

    #[test]
    fn plugins_have_the_box_icon_and_the_rest_the_package() {
        assert_eq!(kind_icon(AssetKind::Plugin), Icon::Box);
        assert_eq!(kind_icon(AssetKind::Content), Icon::Package);
        assert_eq!(kind_icon(AssetKind::Project), Icon::Package);
    }
}
