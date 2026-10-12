// SPDX-License-Identifier: GPL-3.0-or-later

//! Library: the engines (one row, scrolling sideways) and the projects (a
//! grid of cards).

use egui::{Id, ScrollArea, Ui};
use ue_core::{Engine, EngineAssociation, EngineKind, Project};

use super::widgets::banner::{self, banner};
use super::widgets::button::{Button, MenuItem};
use super::widgets::engine_card::{self, EngineCard, engine_card, install_card};
use super::widgets::gap;
use super::widgets::project_card::{self, ProjectCard, project_card};
use super::widgets::section_header::section_header;
use super::widgets::{card_grid, colors, empty_state::empty_state};
use super::{error_box, page_header, short_path};
use crate::app::{Action, Intent, LauncherApp};
use crate::backend::Snapshot;
use crate::images::ImageKey;
use crate::text;
use crate::theme::{Icon, Text, size, space};

pub fn draw(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    page_header(ui, app, text::PAGE_LIBRARY, |ui| {
        if ui
            .add(Button::secondary(text::INSTALL_ENGINE_BUTTON).icon(Icon::Plus))
            .clicked()
        {
            intents.push(Intent::InstallEngine);
        }
    });
    let Some(snapshot) = &app.snapshot else {
        ui.spinner();
        return;
    };
    if let Err(error) = &snapshot.registry {
        error_box(ui, error);
        gap(ui, space::S6);
    }

    new_version_banner(app, ui, intents);
    engines(app, snapshot, ui, intents);
    gap(ui, space::S6);
    unregistered(app, snapshot, ui, intents);
    projects(app, snapshot, ui, intents);
}

/// Tells the user when Fab lists a newer engine than every installed one.
fn new_version_banner(app: &LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let Some(version) = app.new_version_banner() else {
        return;
    };
    let newest = app
        .snapshot
        .as_ref()
        .and_then(|s| s.engines().iter().map(|e| e.version).max());
    let detail = newest.map_or_else(
        || text::NEW_VERSION_DETAIL_NONE.to_owned(),
        |v| {
            format!(
                "{} {v}. {}",
                text::NEW_VERSION_DETAIL,
                text::NEW_VERSION_NOTE
            )
        },
    );
    banner(
        ui,
        banner::Tone::Notice,
        Icon::Download,
        &format!("Unreal Engine {version} {}", text::NEW_VERSION_TITLE_SUFFIX),
        &detail,
        |ui| {
            if ui
                .add(Button::primary(text::DOWNLOAD_NEW_ENGINE).icon(Icon::Download))
                .clicked()
            {
                intents.push(Intent::DownloadNewEngine);
            }
            if ui.add(Button::ghost(text::LATER)).clicked() {
                intents.push(Intent::DismissNewVersion);
            }
        },
    );
    gap(ui, space::S6);
}

fn engines(app: &LauncherApp, snapshot: &Snapshot, ui: &mut Ui, intents: &mut Vec<Intent>) {
    section_header(ui, text::ENGINE_VERSIONS, |ui| {
        if ui
            .add(Button::ghost(text::ADD_EXISTING_ENGINE).icon(Icon::FolderOpen))
            .on_hover_text(text::ADD_EXISTING_ENGINE_HINT)
            .clicked()
        {
            intents.push(Intent::PickEngineFolder);
        }
    });
    let engines = snapshot.engines();
    if engines.is_empty() {
        empty_state(
            ui,
            Icon::Box,
            text::EMPTY_ENGINES_TITLE,
            text::EMPTY_ENGINES_BODY,
            |ui| {
                if ui
                    .add(Button::primary(text::INSTALL_ENGINE_BUTTON).icon(Icon::Plus))
                    .clicked()
                {
                    intents.push(Intent::InstallEngine);
                }
            },
        );
        return;
    }
    let selected = app.selected_engine();
    ScrollArea::horizontal()
        .id_salt("engine_row")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S3;
                for engine in engines {
                    engine_item(ui, engine, selected.as_ref() == Some(&engine.id), intents);
                }
                if install_card(ui, Id::new("install_engine_card")) {
                    intents.push(Intent::InstallEngine);
                }
            });
            ui.add_space(space::S1);
        });
}

fn engine_item(ui: &mut Ui, engine: &Engine, selected: bool, intents: &mut Vec<Intent>) {
    let numeral = format!("{}.{}", engine.version.major, engine.version.minor);
    let (install, detail) = match &engine.kind {
        EngineKind::Prebuilt => (engine_card::Install::Prebuilt, engine.version.to_string()),
        EngineKind::Source { git_ref, .. } => (
            engine_card::Install::Source,
            format!("{} · {git_ref}", engine.version),
        ),
        EngineKind::Unknown => (engine_card::Install::External, engine.version.to_string()),
    };
    let card = EngineCard {
        numeral: &numeral,
        detail: &detail,
        install,
        selected,
    };
    let launched = engine_card(ui, Id::new(("engine", engine.id.as_str())), &card, |ui| {
        engine_menu(ui, engine, intents);
    });
    if launched {
        intents.push(Intent::LaunchEngine(engine.clone()));
    }
}

/// Update (source), Open folder, Add to app menu, Clean caches, Remove from
/// the list and Uninstall.
fn engine_menu(ui: &mut Ui, engine: &Engine, intents: &mut Vec<Intent>) {
    if let EngineKind::Source { git_ref, .. } = &engine.kind
        && ui
            .add(MenuItem::new(text::MENU_UPDATE).icon(Icon::RefreshCw))
            .clicked()
    {
        intents.push(Intent::Ask {
            title: text::UPDATE.to_owned(),
            body: format!(
                "Fetch the latest {git_ref} for {} and rebuild it?\n{}",
                engine.id,
                text::UPDATE_NOTE
            ),
            action: Action::Update {
                engine_root: engine.path.clone(),
                git_ref: git_ref.clone(),
            },
        });
    }
    if ui
        .add(MenuItem::new(text::MENU_OPEN_FOLDER).icon(Icon::FolderOpen))
        .clicked()
    {
        intents.push(Intent::OpenFile(engine.path.clone()));
    }
    if ui
        .add(MenuItem::new(text::MENU_ADD_TO_APP_MENU).icon(Icon::Plus))
        .clicked()
    {
        intents.push(Intent::AddShortcut(engine.clone()));
    }
    if ui
        .add(MenuItem::new(text::MENU_CLEAN_CACHES).icon(Icon::HardDrive))
        .clicked()
    {
        intents.push(Intent::Maintenance(engine.clone()));
    }
    if ui
        .add(MenuItem::new(text::MENU_REMOVE_FROM_LIST).icon(Icon::X))
        .clicked()
    {
        intents.push(ask_unregister(&engine.id, &engine.path));
    }
    if ui
        .add(
            MenuItem::new(text::MENU_UNINSTALL)
                .icon(Icon::Trash2)
                .danger(),
        )
        .clicked()
    {
        intents.push(Intent::AskTyped {
            title: format!("Uninstall Unreal Engine {}?", engine.version),
            body: format!(
                "Its folder at {} is deleted and the engine leaves the list. \
                 Projects that use it stay where they are.",
                engine.path.display()
            ),
            expected: engine.id.to_string(),
            action: Action::Uninstall(engine.id.clone()),
        });
    }
}

fn ask_unregister(id: &ue_core::EngineId, path: &std::path::Path) -> Intent {
    Intent::Ask {
        title: text::UNREGISTER.to_owned(),
        body: format!(
            "Remove {id} ({}) from the engine list?\nThe engine files are not deleted. {}",
            path.display(),
            text::BACKUP_NOTE
        ),
        action: Action::Unregister(id.clone()),
    }
}

/// Engines found on disk but not registered, and registered entries that
/// are not an engine any more.
fn unregistered(app: &LauncherApp, snapshot: &Snapshot, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    let invalid = snapshot
        .registry
        .as_ref()
        .map_or(&[][..], |registry| registry.invalid_entries());
    if snapshot.unregistered.is_empty() && invalid.is_empty() {
        return;
    }
    section_header(ui, text::FOUND_ON_DISK, |_| {});
    ui.label(Text::Caption.colored(text::FOUND_ON_DISK_HINT, p.ink_muted));
    ui.add_space(space::S2);
    for found in &snapshot.unregistered {
        note_row(
            ui,
            |ui| {
                ui.label(
                    Text::BodyStrong.colored(format!("Unreal Engine {}", found.version), p.ink),
                );
                ui.label(Text::Mono.colored(short_path(&found.path, app.home()), p.ink_muted));
            },
            |ui| {
                if ui.add(Button::secondary(text::REGISTER)).clicked() {
                    intents.push(Intent::Ask {
                        title: text::REGISTER.to_owned(),
                        body: format!(
                            "Register Unreal Engine {} at {}?\n{}",
                            found.version,
                            found.path.display(),
                            text::BACKUP_NOTE
                        ),
                        action: Action::Register(vec![found.path.clone()]),
                    });
                }
            },
        );
    }
    for entry in invalid {
        note_row(
            ui,
            |ui| {
                ui.label(
                    Text::BodyStrong
                        .colored(format!("{} · {}", text::NOT_AVAILABLE, entry.id), p.ink),
                );
                ui.label(Text::Mono.colored(short_path(&entry.path, app.home()), p.ink_muted));
                if let Some(hint) = sandbox_hint(app, &entry.path) {
                    ui.label(Text::Caption.colored(text::OUTSIDE_SANDBOX, p.warning));
                    ui.label(Text::Mono.colored(hint, p.ink_muted));
                } else {
                    ui.label(
                        Text::Caption.colored(common::UserFacing::summary(&entry.error), p.warning),
                    );
                }
            },
            |ui| {
                if ui.add(Button::secondary(text::UNREGISTER)).clicked() {
                    intents.push(ask_unregister(&entry.id, &entry.path));
                }
            },
        );
    }
    gap(ui, space::S6);
}

/// A row on `bg-200` with some text on the left and an action on the right.
fn note_row(ui: &mut Ui, text: impl FnOnce(&mut Ui), action: impl FnOnce(&mut Ui)) {
    let p = colors(ui);
    egui::Frame::new()
        .fill(p.bg_200)
        .stroke(egui::Stroke::new(size::STROKE, p.line))
        .corner_radius(crate::theme::corner(crate::theme::radius::MD))
        .inner_margin(crate::theme::margin(space::S3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    text(ui);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), action);
            });
        });
    ui.add_space(space::S2);
}

/// Inside Flatpak, an engine outside the home folder is invisible until the
/// user grants access. Returns the command that does it.
fn sandbox_hint(app: &LauncherApp, path: &std::path::Path) -> Option<String> {
    if !app.is_flatpak() || path.starts_with(app.home()) || path.exists() {
        return None;
    }
    // Grant the top two levels (e.g. /mnt/disk), where the engines folder lives.
    let root: std::path::PathBuf = path.components().take(3).collect();
    Some(format!(
        "flatpak override --user --filesystem={} {}",
        root.display(),
        system::APP_ID
    ))
}

fn projects(app: &LauncherApp, snapshot: &Snapshot, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let has_engines = !snapshot.engines().is_empty();
    section_header(ui, text::MY_PROJECTS, |ui| {
        // Added right to left: Add folder is the rightmost.
        if ui
            .add(Button::ghost(text::ADD_FOLDER_BUTTON).icon(Icon::FolderOpen))
            .clicked()
        {
            intents.push(Intent::PickProjectFolder);
        }
        if ui
            .add(
                Button::secondary(text::NEW_PROJECT_BUTTON)
                    .icon(Icon::Plus)
                    .enabled(has_engines, text::NEEDS_AN_ENGINE),
            )
            .clicked()
        {
            intents.push(Intent::NewProject);
        }
    });
    let mut loaded: Vec<&Project> = Vec::new();
    for project in &snapshot.projects {
        match project {
            Ok(project) => loaded.push(project),
            Err(error) => error_box(ui, error),
        }
    }
    if loaded.is_empty() {
        empty_state(
            ui,
            Icon::FolderSearch,
            text::EMPTY_PROJECTS_TITLE,
            text::EMPTY_PROJECTS_BODY,
            |ui| {
                if ui
                    .add(
                        Button::primary(text::NEW_PROJECT_BUTTON)
                            .icon(Icon::Plus)
                            .enabled(has_engines, text::NEEDS_AN_ENGINE),
                    )
                    .clicked()
                {
                    intents.push(Intent::NewProject);
                }
            },
        );
        return;
    }
    card_grid(
        ui,
        loaded.len(),
        size::CARD_MIN_WIDTH,
        space::S3,
        project_card::height_for,
        |ui, index, rect| {
            let project = loaded[index];
            project_item(app, snapshot, project, ui, rect, intents);
        },
    );
}

fn project_item(
    app: &LauncherApp,
    snapshot: &Snapshot,
    project: &Project,
    ui: &mut Ui,
    rect: egui::Rect,
    intents: &mut Vec<Intent>,
) {
    let engine = snapshot.engine_for(project);
    let folder = project
        .path
        .parent()
        .and_then(std::path::Path::parent)
        .map_or_else(String::new, |dir| short_path(dir, app.home()));
    let (meta, missing) = match engine {
        Some(engine) => (
            format!(
                "UE {}.{} · {folder}",
                engine.version.major, engine.version.minor
            ),
            None,
        ),
        None => match &project.association {
            EngineAssociation::None => (
                text::CHOOSE_AN_ENGINE.to_owned(),
                Some(text::MISSING_ENGINE_NONE.to_owned()),
            ),
            EngineAssociation::Version { major, minor } => (
                format!("{} {major}.{minor}", text::MADE_WITH),
                Some(format!(
                    "No {major}.{minor} {}",
                    text::MISSING_ENGINE_VERSION
                )),
            ),
            EngineAssociation::Id(id) => (
                format!("{} {id}", text::MADE_WITH),
                Some(format!("Engine {id} is not installed")),
            ),
        },
    };
    let picture = snapshot
        .thumbnails
        .get(&project.path)
        .and_then(|path| app.images.texture(&ImageKey::File(path.clone())));
    let card = ProjectCard {
        name: &project.name,
        meta: &meta,
        picture,
        missing_engine: missing.as_deref(),
    };
    let id = Id::new(("project", project.path.as_path()));
    let opened = project_card(ui, id, rect, &card, |ui| {
        project_menu(ui, project, engine, intents);
    });
    if opened {
        if engine.is_some() {
            intents.push(Intent::OpenProject(project.path.clone()));
        } else {
            intents.push(change_engine(project, None));
        }
    }
}

fn change_engine(project: &Project, current: Option<&Engine>) -> Intent {
    Intent::ChangeEngine {
        project: project.path.clone(),
        name: project.name.clone(),
        current: current.map(|engine| engine.id.clone()),
    }
}

/// Change engine, Show in folder and Delete project.
fn project_menu(
    ui: &mut Ui,
    project: &Project,
    engine: Option<&Engine>,
    intents: &mut Vec<Intent>,
) {
    if ui
        .add(MenuItem::new(text::MENU_CHANGE_ENGINE).icon(Icon::Box))
        .clicked()
    {
        intents.push(change_engine(project, engine));
    }
    let folder = project.path.parent().unwrap_or(&project.path).to_path_buf();
    if ui
        .add(MenuItem::new(text::MENU_SHOW_IN_FOLDER).icon(Icon::FolderOpen))
        .clicked()
    {
        intents.push(Intent::OpenFile(folder.clone()));
    }
    if ui
        .add(
            MenuItem::new(text::MENU_DELETE_PROJECT)
                .icon(Icon::Trash2)
                .danger(),
        )
        .clicked()
    {
        intents.push(Intent::AskTyped {
            title: format!("Delete {}?", project.name),
            body: format!(
                "The project folder {} is deleted with everything in it. This cannot be undone.",
                folder.display()
            ),
            expected: project.name.clone(),
            action: Action::DeleteProject(project.path.clone()),
        });
    }
}
