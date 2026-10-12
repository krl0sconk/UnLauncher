// SPDX-License-Identifier: GPL-3.0-or-later

//! The Install engine dialog.

#[allow(clippy::wildcard_imports, reason = "part of the dialogs module")]
use super::*;
use crate::app::{InstallForm, InstallMode};
use crate::backend::format_bytes;
use crate::ui::widgets::badge::filter_chip;
use crate::ui::widgets::text_field::{path_row, secret_field, text_field};

pub(super) fn install_engine(
    ui: &mut Ui,
    form: &mut InstallForm,
    context: &Context<'_>,
    intents: &mut Vec<Intent>,
) {
    body(ui, |ui| {
        ui.horizontal(|ui| {
            for (mode, label) in [
                (InstallMode::Prebuilt, text::INSTALL_PREBUILT),
                (InstallMode::Source, text::INSTALL_SOURCE),
            ] {
                if filter_chip(ui, label, form.mode == mode).clicked() {
                    intents.push(Intent::SetInstallMode(mode));
                }
            }
        });
        match form.mode {
            InstallMode::Prebuilt => {
                install_prebuilt(ui, form, intents);
                epic_downloads(ui, form, context.library, intents);
            }
            InstallMode::Source => {
                egui::ScrollArea::vertical()
                    .max_height(ui.ctx().content_rect().height() * 0.6)
                    .show(ui, |ui| install_source(ui, form, context, intents));
            }
        }
    });
    match form.mode {
        InstallMode::Prebuilt => {
            let free = crate::tasks::is_free_destination(&form.destination);
            let why = if form.zip.is_none() {
                Some(text::CHOOSE_A_ZIP)
            } else if !free {
                Some(text::DESTINATION_NOT_EMPTY)
            } else {
                None
            };
            if ask_footer(ui, text::IMPORT, Kind::Primary, why, intents)
                && let Some(zip) = form.zip.clone()
            {
                intents.push(Intent::StartImport {
                    zip,
                    destination: form.destination.clone(),
                });
            }
        }
        InstallMode::Source => {
            // Warnings (little RAM, disk on the edge) never block: only
            // failures do.
            let status = form.preflight.as_deref().map(ue_build::overall_status);
            let why = if form.git_ref.trim().is_empty() {
                Some(text::GIT_REF_NEEDED)
            } else if !context.github.is_ready() {
                Some(text::CONNECT_GITHUB_FIRST)
            } else if !matches!(
                status,
                Some(ue_build::CheckStatus::Ok | ue_build::CheckStatus::Warn)
            ) {
                Some(text::CHECK_REQUIREMENTS_FIRST)
            } else {
                None
            };
            if ask_footer(ui, text::BUILD, Kind::Primary, why, intents) {
                intents.push(Intent::StartBuild);
            }
        }
    }
}

fn install_prebuilt(ui: &mut Ui, form: &InstallForm, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    ui.label(Text::Body.colored(text::INSTALL_PREBUILT_HELP, p.ink_muted));
    let zip = form
        .zip
        .as_ref()
        .map_or_else(String::new, |zip| zip.display().to_string());
    if path_row(ui, &zip, text::CHOOSE_ZIP) {
        intents.push(Intent::PickZip);
    }
    ui.label(Text::BodyStrong.colored(text::DESTINATION, p.ink));
    if path_row(ui, &form.destination.display().to_string(), text::BROWSE) {
        intents.push(Intent::PickInstallDestination);
    }
    if !crate::tasks::is_free_destination(&form.destination) {
        ui.label(Text::Body.colored(text::DESTINATION_NOT_EMPTY, p.warning));
    }
    ui.label(Text::Caption.colored(text::BACKUP_NOTE, p.ink_faint));
}

fn install_source(
    ui: &mut Ui,
    form: &mut InstallForm,
    context: &Context<'_>,
    intents: &mut Vec<Intent>,
) {
    use ue_build::CheckStatus;

    let p = colors(ui);
    ui.label(Text::Body.colored(text::INSTALL_SOURCE_HELP, p.ink_muted));
    ui.label(Text::BodyStrong.colored(text::GIT_REF, p.ink));
    text_field(ui, &mut form.git_ref);
    ui.label(Text::BodyStrong.colored(text::DESTINATION, p.ink));
    if path_row(
        ui,
        &form.source_destination.display().to_string(),
        text::BROWSE,
    ) {
        intents.push(Intent::PickSourceDestination);
    }
    ui.label(Text::BodyStrong.colored(text::JOBS, p.ink));
    text_field(ui, &mut form.jobs);

    super::github::connect_github(ui, context.library, context.github, intents);
    advanced_token(ui, form, intents);

    ui.horizontal(|ui| {
        if ui
            .add(
                Button::secondary(text::CHECK)
                    .icon(Icon::ListChecks)
                    .enabled(!form.checking, text::CHECKING),
            )
            .clicked()
        {
            intents.push(Intent::RunPreflight);
        }
        if form.checking {
            ui.spinner();
        }
    });
    if let Some(results) = &form.preflight {
        for check in results {
            let (icon, color) = match check.status {
                CheckStatus::Ok => (Icon::CircleCheck, p.success),
                CheckStatus::Warn => (Icon::TriangleAlert, p.warning),
                CheckStatus::Fail => (Icon::X, p.danger),
            };
            ui.horizontal_top(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(size::ICON), egui::Sense::hover());
                super::super::widgets::paint_icon(ui, icon, rect.center(), size::ICON, color);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::S1;
                    ui.label(
                        Text::Body.colored(format!("{}: {}", check.name, check.message), p.ink),
                    );
                    if let Some(hint) = &check.hint
                        && check.status != CheckStatus::Ok
                    {
                        ui.label(Text::Caption.colored(hint, p.ink_muted));
                    }
                });
            });
        }
        if ue_build::overall_status(results) == CheckStatus::Warn {
            ui.label(Text::Caption.colored(text::WARNINGS_DONT_BLOCK, p.ink_muted));
        }
    }
}

/// "Advanced options → Use a personal token", folded.
fn advanced_token(ui: &mut Ui, form: &mut InstallForm, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    egui::CollapsingHeader::new(Text::BodyStrong.colored(text::ADVANCED_OPTIONS, p.ink_muted))
        .default_open(false)
        .show(ui, |ui| {
            ui.label(Text::BodyStrong.colored(text::PERSONAL_TOKEN, p.ink));
            ui.label(Text::Caption.colored(text::PERSONAL_TOKEN_HELP, p.ink_muted));
            ui.horizontal(|ui| {
                secret_field(ui, &mut form.token_input);
                if ui
                    .add(
                        Button::secondary(text::SAVE_TOKEN)
                            .enabled(!form.token_input.trim().is_empty(), text::PASTE_A_TOKEN),
                    )
                    .clicked()
                {
                    intents.push(Intent::SaveGitHubToken(form.token_input.clone()));
                }
            });
        });
}

fn epic_downloads(
    ui: &mut Ui,
    form: &InstallForm,
    library: &crate::app::LibraryState,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    ui.separator();
    ui.label(Text::BodyStrong.colored(text::DOWNLOAD_FROM_EPIC, p.ink));
    if library.listing_downloads {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(Text::Body.colored(text::LISTING_DOWNLOADS, p.ink_muted));
        });
        return;
    }
    if library.login_mode == crate::app::LoginMode::NoWindow {
        ui.label(Text::Caption.colored(text::DOWNLOADS_IN_BROWSER, p.ink_muted));
    }
    match &library.engine_downloads {
        None => {
            if ui
                .add(Button::secondary(text::LIST_DOWNLOADS).icon(Icon::Download))
                .clicked()
            {
                intents.push(Intent::ListEngineDownloads);
            }
        }
        Some(Err(error)) => {
            error_box(ui, error);
            ui.horizontal(|ui| {
                if library.login_mode != crate::app::LoginMode::NoWindow
                    && ui.add(Button::secondary(text::LIST_DOWNLOADS)).clicked()
                {
                    intents.push(Intent::ListEngineDownloads);
                }
                if ui
                    .add(Button::secondary(text::OPEN_DOWNLOAD_PAGE).icon(Icon::ExternalLink))
                    .clicked()
                {
                    intents.push(Intent::OpenEngineDownloadPage);
                }
            });
        }
        Some(Ok(downloads)) => {
            ui.label(Text::Caption.colored(text::DOWNLOAD_SPACE_NOTE, p.ink_muted));
            let free = crate::tasks::is_free_destination(&form.destination);
            for download in downloads.iter().take(5) {
                ui.horizontal(|ui| {
                    let length = download
                        .size
                        .map_or_else(String::new, |s| format!(" ({})", format_bytes(s)));
                    ui.label(
                        Text::Body
                            .colored(format!("Unreal Engine {}{length}", download.version), p.ink),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                Button::primary(text::DOWNLOAD)
                                    .icon(Icon::Download)
                                    .enabled(free, text::DESTINATION_NOT_EMPTY),
                            )
                            .clicked()
                        {
                            intents.push(Intent::StartEngineDownload(download.version.clone()));
                        }
                    });
                });
            }
        }
    }
}
