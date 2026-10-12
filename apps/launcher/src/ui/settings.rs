// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings: appearance, project folders, accounts (Epic Games and GitHub,
//! with Connect/Disconnect) and About.

use egui::{Align, Frame, Layout, Stroke, Ui};
use github_auth::Access;

use super::widgets::badge::{Badge, Tone, filter_chip};
use super::widgets::button::{Button, IconButton};
use super::widgets::colors;
use super::widgets::gap;
use super::widgets::section_header::section_header;
use super::{error_box, page_header, short_path};
use crate::app::{EpicState, Intent, LauncherApp, LoginWindow, Screen};
use crate::settings::ThemeChoice;
use crate::text;
use crate::theme::{self, Icon, Text, radius, size, space};

pub fn draw(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    page_header(ui, app, text::SETTINGS_TITLE, |_| {});
    appearance(app, ui, intents);
    folders(app, ui, intents);
    section_header(ui, text::ACCOUNTS, |_| {});
    epic_card(app, ui, intents);
    ui.add_space(space::S2);
    github_card(app, ui, intents);
    gap(ui, space::S6);
    about(ui, intents);
}

/// A card on `bg-200`: a title row with its actions on the right, then the
/// body.
fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let p = colors(ui);
    Frame::new()
        .fill(p.bg_200)
        .stroke(Stroke::new(size::STROKE, p.line))
        .corner_radius(theme::corner(radius::LG))
        .inner_margin(theme::margin(space::S3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

fn appearance(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    section_header(ui, text::SETTINGS_APPEARANCE, |_| {});
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(Text::BodyStrong.colored(text::THEME, p.ink));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (choice, label) in [
                    (ThemeChoice::Light, text::THEME_LIGHT),
                    (ThemeChoice::Dark, text::THEME_DARK),
                    (ThemeChoice::System, text::THEME_SYSTEM),
                ] {
                    if filter_chip(ui, label, app.theme() == choice).clicked() {
                        intents.push(Intent::SetTheme(choice));
                    }
                }
            });
        });
    });
    gap(ui, space::S6);
}

fn folders(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    section_header(ui, text::SETTINGS_FOLDERS, |ui| {
        if ui
            .add(Button::secondary(text::ADD_FOLDER_BUTTON).icon(Icon::FolderOpen))
            .clicked()
        {
            intents.push(Intent::PickProjectFolder);
        }
    });
    card(ui, |ui| {
        ui.label(Text::Caption.colored(text::SETTINGS_FOLDERS_HINT, p.ink_muted));
        ui.add_space(space::S2);
        for folder in system::paths::default_project_search_paths(app.home()) {
            ui.horizontal(|ui| {
                ui.label(Text::Mono.colored(short_path(&folder, app.home()), p.ink_muted));
                ui.add(Badge::new(Tone::Neutral, text::DEFAULT_FOLDER));
            });
        }
        for folder in app.project_folders() {
            ui.horizontal(|ui| {
                ui.label(Text::Mono.colored(short_path(folder, app.home()), p.ink));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(IconButton::new(Icon::X, text::REMOVE_FOLDER))
                        .clicked()
                    {
                        intents.push(Intent::RemoveProjectFolder(folder.clone()));
                    }
                });
            });
        }
    });
    gap(ui, space::S6);
}

/// An account: its name, its state, the actions and a note.
fn account_card(
    ui: &mut Ui,
    title: &str,
    state: &str,
    connected: bool,
    note: &str,
    actions: impl FnOnce(&mut Ui),
) {
    let p = colors(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(Text::CardTitle.colored(title, p.ink));
                    if connected {
                        ui.add(Badge::new(Tone::Success, text::CONNECTED).icon(Icon::CircleCheck));
                    }
                });
                ui.label(Text::Caption.colored(state, p.ink_muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), actions);
        });
        ui.add_space(space::S2);
        ui.label(Text::Caption.colored(note, p.ink_faint));
    });
}

fn epic_card(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let library = &app.library;
    let (state, connected) = match (&library.epic, library.epic.display_name()) {
        (_, Some(name)) => (format!("{} {name}", text::CONNECTED_AS), true),
        (EpicState::Restoring | EpicState::SigningIn, None) => (text::LOADING.to_owned(), false),
        _ => (text::NOT_CONNECTED.to_owned(), false),
    };
    account_card(
        ui,
        text::ACCOUNT_EPIC,
        &state,
        connected,
        text::EPIC_ACCOUNT_HELP,
        |ui| match &library.epic {
            EpicState::SignedIn(_) | EpicState::Demo(_) => {
                if ui.add(Button::danger(text::DISCONNECT)).clicked() {
                    intents.push(Intent::SignOut);
                }
            }
            EpicState::SignedOut => {
                let busy = !matches!(library.login_window, LoginWindow::Closed);
                if ui
                    .add(
                        Button::primary(text::CONNECT)
                            .icon(Icon::LogIn)
                            .enabled(!busy, text::SIGN_IN_WAITING),
                    )
                    .clicked()
                {
                    // The Fab screen shows the sign-in progress and the
                    // browser fallback.
                    intents.push(Intent::Navigate(Screen::Fab));
                    intents.push(Intent::SignInWithWindow);
                }
                if busy {
                    ui.spinner();
                }
            }
            EpicState::Restoring | EpicState::SigningIn => {
                ui.spinner();
            }
        },
    );
    if let Some(error) = &library.error {
        error_box(ui, error);
    }
}

fn github_card(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    let github = &app.github;
    let connected = github.saved == Some(true);
    let state = match (github.saved, github.access()) {
        (Some(false), _) => text::NOT_CONNECTED.to_owned(),
        (Some(true), Some(Access::Ready { login })) => format!("{} @{login}", text::CONNECTED_AS),
        (Some(true), Some(access)) => common::UserFacing::summary(access),
        (None, _) | (Some(true), None) => text::LOADING.to_owned(),
    };
    account_card(
        ui,
        text::ACCOUNT_GITHUB,
        &state,
        connected && github.is_ready(),
        text::GITHUB_ACCOUNT_HELP,
        |ui| {
            if connected {
                if ui.add(Button::danger(text::DISCONNECT)).clicked() {
                    intents.push(Intent::GithubDisconnect);
                }
            } else if github.device.is_none()
                && ui
                    .add(Button::primary(text::CONNECT).icon(Icon::LogIn))
                    .clicked()
            {
                intents.push(Intent::GithubConnect);
            }
            if github.checking || github.detecting {
                ui.spinner();
            }
        },
    );
    let blocked = matches!(github.access(), Some(Access::Blocked { .. }));
    ui.add_space(space::S2);
    if !super::dialogs::code_in_progress(ui, github, intents) && (!connected || blocked) {
        if blocked {
            ui.label(Text::Body.colored(text::GITHUB_BLOCKED, p.warning));
        }
        for (index, offer) in github
            .existing
            .iter()
            .enumerate()
            .filter(|(_, o)| o.is_ready)
        {
            let label = format!(
                "{} (@{}, {})",
                text::USE_THIS_COMPUTER,
                offer.login,
                offer.source.label()
            );
            if ui.add(Button::secondary(&label)).clicked() {
                intents.push(Intent::GithubUseExisting(index));
            }
        }
        super::dialogs::gh_option(ui, github, intents);
    }
    if ui.link(text::REVOKE_ON_GITHUB).clicked() {
        intents.push(Intent::OpenUrl(text::GITHUB_APPLICATIONS_URL.to_owned()));
    }
    if let Some(error) = &github.error {
        error_box(ui, error);
    }
}

fn about(ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    section_header(ui, text::SETTINGS_ABOUT, |_| {});
    card(ui, |ui| {
        ui.label(Text::CardTitle.colored(
            format!(
                "{} {} {}",
                text::APP_NAME,
                text::ABOUT_VERSION,
                env!("CARGO_PKG_VERSION")
            ),
            p.ink,
        ));
        ui.add_space(space::S2);
        for line in [
            text::ABOUT_LICENSE,
            text::ABOUT_FONTS,
            text::ABOUT_ICONS,
            text::ABOUT_LICENSES_FOLDER,
            text::ABOUT_TRADEMARKS,
        ] {
            ui.label(Text::Caption.colored(line, p.ink_muted));
        }
        ui.add_space(space::S2);
        if ui
            .add(Button::ghost(text::SOURCE_CODE).icon(Icon::ExternalLink))
            .clicked()
        {
            intents.push(Intent::OpenUrl(text::SOURCE_URL.to_owned()));
        }
    });
}
