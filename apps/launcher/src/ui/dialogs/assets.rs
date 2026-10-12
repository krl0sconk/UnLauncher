// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine maintenance and Fab asset dialogs.

#[allow(clippy::wildcard_imports, reason = "part of the dialogs module")]
use super::*;
use crate::app::{FabForm, MaintenanceForm};
use crate::backend::format_bytes;
use crate::epic_tasks::Target;

pub(super) fn maintenance(ui: &mut Ui, form: &mut MaintenanceForm, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    body(ui, |ui| {
        super::path_label(ui, &form.engine.path.display().to_string());
        let Some(usage) = &form.usage else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(Text::Body.colored(text::DISK_USAGE, p.ink_muted));
            });
            return;
        };
        ui.label(Text::Body.colored(
            format!("{}: {}", text::DISK_USAGE, format_bytes(usage.total)),
            p.ink,
        ));
        ui.label(Text::BodyStrong.colored(text::CACHES, p.ink));
        for cache in &usage.caches {
            ui.horizontal(|ui| {
                let scope = if cache.shared {
                    format!(" ({})", text::SHARED)
                } else {
                    String::new()
                };
                ui.label(Text::Body.colored(
                    format!("{}{scope}: {}", cache.name, format_bytes(cache.size)),
                    p.ink_muted,
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            Button::secondary(text::CLEAN)
                                .icon(Icon::HardDrive)
                                .enabled(cache.size > 0, text::CACHE_EMPTY),
                        )
                        .clicked()
                    {
                        intents.push(Intent::Ask {
                            title: format!("{} {}?", text::CLEAN, cache.name),
                            body: format!(
                                "Delete everything in {} ({})? Close every Unreal Editor first.",
                                cache.path.display(),
                                format_bytes(cache.size)
                            ),
                            action: Action::CleanCache(cache.path.clone()),
                        });
                    }
                });
            });
        }
    });
    close_footer(ui, intents);
}

/// One place an asset can go.
struct Choice {
    label: String,
    target: Target,
    version: Option<ue_core::EngineVersion>,
}

#[allow(clippy::too_many_lines, reason = "one dialog, drawn top to bottom")]
pub(super) fn fab_install(
    ui: &mut Ui,
    form: &mut FabForm,
    context: &Context<'_>,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    let Some(snapshot) = context.snapshot else {
        body(ui, |ui| {
            ui.spinner();
        });
        return;
    };
    // A copy, so the form can change while the build is borrowed.
    let asset = form.asset.clone();
    let kind = asset.kind;
    let mut choices = choices(snapshot, &asset);
    // Compatible destinations first; the others are allowed, with a warning.
    for choice in &mut choices {
        if choice
            .version
            .is_none_or(|v| fab::compatible_version(&asset, v).is_none())
        {
            choice.label.push_str(text::OTHER_VERSION_SUFFIX);
        }
    }
    choices.sort_by_key(|choice| choice.label.ends_with(text::OTHER_VERSION_SUFFIX));

    let picked = form
        .target
        .as_ref()
        .and_then(|target| choices.iter().find(|c| same_target(&c.target, target)));
    let engine_version = picked.and_then(|choice| choice.version);
    let version = engine_version.and_then(|v| fab::closest_version(&asset, v));

    body(ui, |ui| {
        asset_header(ui, &asset, context.images);
        ui.label(Text::BodyStrong.colored(text::INSTALL_INTO, p.ink));
        if choices.is_empty() {
            ui.label(Text::Body.colored(text::NO_TARGETS, p.ink_muted));
        }
        let selected_label = picked.map(|choice| choice.label.clone());
        egui::ComboBox::from_id_salt("fab_target")
            .selected_text(selected_label.unwrap_or_default())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for choice in &choices {
                    let is_selected = form
                        .target
                        .as_ref()
                        .is_some_and(|t| same_target(t, &choice.target));
                    if ui.selectable_label(is_selected, &choice.label).clicked() {
                        form.target = Some(choice.target.clone());
                    }
                }
            });
        if let (Some((build, false)), Some(engine)) = (version, engine_version) {
            other_version_warning(ui, build, engine);
        }
        if form.target.is_some() && engine_version.is_none() {
            ui.label(Text::Body.colored(text::NO_COMPATIBLE, p.warning));
        }
        if kind == fab::AssetKind::Project {
            ui.label(Text::BodyStrong.colored(text::LOCATION, p.ink));
            super::text_field(ui, &mut form.location);
        } else {
            ui.label(Text::Caption.colored(text::MISSING_PROJECT_HINT, p.ink_faint));
        }
    });

    let to_engine = matches!(picked.map(|c| &c.target), Some(Target::Engine(_)));
    let ready =
        version.is_some() && (kind != fab::AssetKind::Project || !form.location.trim().is_empty());
    let add_why = if picked.is_none() {
        Some(text::PICK_A_DESTINATION)
    } else if to_engine {
        Some(text::PICK_A_PROJECT)
    } else if !ready {
        Some(text::NO_COMPATIBLE)
    } else {
        None
    };
    let engine_why = if !to_engine {
        Some(text::PICK_AN_ENGINE_TARGET)
    } else if !ready {
        Some(text::NO_COMPATIBLE)
    } else {
        None
    };
    footer(ui, |ui| {
        let (label, why) = if kind == fab::AssetKind::Project {
            (text::CREATE_PROJECT, add_why)
        } else {
            (text::ASSET_DIALOG_ADD, add_why)
        };
        if ui
            .add(
                Button::primary(label)
                    .icon(Icon::Package)
                    .enabled(why.is_none(), why.unwrap_or_default()),
            )
            .clicked()
        {
            intents.push(Intent::StartFabInstall);
        }
        if kind != fab::AssetKind::Content
            && kind != fab::AssetKind::Project
            && ui
                .add(
                    Button::secondary(text::ASSET_DIALOG_ENGINE)
                        .icon(Icon::Box)
                        .enabled(engine_why.is_none(), engine_why.unwrap_or_default()),
                )
                .clicked()
        {
            intents.push(Intent::StartFabInstall);
        }
        if ui.add(Button::ghost(text::CANCEL)).clicked() {
            intents.push(Intent::CloseDialog);
        }
    });
}

/// The picture and the kind line at the top of the asset dialog.
fn asset_header(ui: &mut Ui, asset: &fab::LibraryAsset, images: &Images) {
    let p = colors(ui);
    let picture = asset
        .thumbnail_url
        .as_ref()
        .and_then(|url| images.texture(&ImageKey::Remote(url.clone())));
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, width / size::THUMB_RATIO),
        egui::Sense::hover(),
    );
    let icon = if asset.kind == fab::AssetKind::Plugin {
        Icon::Box
    } else {
        Icon::Package
    };
    thumbnail::paint(
        ui,
        rect,
        crate::theme::corner(crate::theme::radius::LG),
        picture,
        &asset.title,
        thumbnail::Mark::Icon(icon),
    );
    ui.label(Text::Caption.colored(super::super::fab::meta(asset), p.ink_muted));
}

/// Where the asset can go: projects and engines (or new projects, for a
/// complete project).
fn choices(snapshot: &crate::backend::Snapshot, asset: &fab::LibraryAsset) -> Vec<Choice> {
    let mut choices = Vec::new();
    if asset.kind == fab::AssetKind::Project {
        for engine in snapshot.engines() {
            choices.push(Choice {
                label: format!(
                    "New project with Unreal Engine {} ({})",
                    engine.version, engine.id
                ),
                target: Target::NewProject {
                    parent: std::path::PathBuf::new(),
                    engine: engine.path.clone(),
                },
                version: Some(engine.version),
            });
        }
        return choices;
    }
    for project in snapshot.projects.iter().filter_map(|p| p.as_ref().ok()) {
        let engine = snapshot.engine_for(project);
        choices.push(Choice {
            label: format!(
                "Project {} ({})",
                project.name,
                engine.map_or_else(|| "?".to_owned(), |e| e.version.to_string())
            ),
            target: Target::Project(project.path.clone()),
            version: engine.map(|e| e.version),
        });
    }
    if asset.kind != fab::AssetKind::Content {
        for engine in snapshot.engines() {
            choices.push(Choice {
                label: format!("Engine {} ({})", engine.id, engine.version),
                target: Target::Engine(engine.path.clone()),
                version: Some(engine.version),
            });
        }
    }
    choices
}

/// Explains that the build being installed was made for other engines.
fn other_version_warning(ui: &mut Ui, build: &fab::AssetVersion, engine: ue_core::EngineVersion) {
    let p = colors(ui);
    let made_for = build
        .engine_versions
        .iter()
        .map(|(major, minor)| format!("{major}.{minor}"))
        .collect::<Vec<_>>()
        .join(", ");
    ui.label(Text::Body.colored(
        format!(
            "There is no build for Unreal Engine {}.{}. The build for {made_for} will be installed; it may not work.",
            engine.major, engine.minor
        ),
        p.warning,
    ));
}

pub(super) fn same_target(a: &Target, b: &Target) -> bool {
    match (a, b) {
        (Target::Project(a), Target::Project(b))
        | (Target::Engine(a), Target::Engine(b))
        | (Target::NewProject { engine: a, .. }, Target::NewProject { engine: b, .. }) => a == b,
        _ => false,
    }
}
