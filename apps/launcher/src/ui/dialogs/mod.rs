// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialogs: a title, a body and a footer, drawn with
//! `widgets::dialog`.

use egui::{Id, Ui};
use ue_core::Engine;

use super::error_box;
use super::widgets::button::{Button, Kind};
use super::widgets::dialog::{self, body, footer};
pub(super) use super::widgets::text_field::{path_label, path_row, text_field};
use super::widgets::{colors, thumbnail};
use crate::app::{Action, Dialog, Intent, NewProjectForm};
use crate::images::{ImageKey, Images};
use crate::text;
use crate::theme::{Icon, Text, size, space};

mod assets;
mod github;

pub use github::{code_in_progress, gh_option};
mod install;

use assets::{fab_install, maintenance};
use install::install_engine;

/// Read-only app state the dialogs need.
pub struct Context<'a> {
    pub snapshot: Option<&'a crate::backend::Snapshot>,
    pub library: &'a crate::app::LibraryState,
    pub github: &'a crate::app::GithubState,
    pub images: &'a Images,
}

/// The title of a dialog.
fn title(dialog: &Dialog) -> String {
    match dialog {
        Dialog::FirstRun(_) => text::FIRST_RUN_TITLE.to_owned(),
        Dialog::Confirm { title, .. } | Dialog::TypedConfirm { title, .. } => title.clone(),
        Dialog::Error(_) => text::SOMETHING_WRONG.to_owned(),
        Dialog::TaskDetails { .. } => text::TASK_DETAILS_TITLE.to_owned(),
        Dialog::ChangeEngine { .. } => text::CHANGE_ENGINE_TITLE.to_owned(),
        Dialog::NewProject(_) => text::NEW_PROJECT_TITLE.to_owned(),
        Dialog::Created(_) => text::PROJECT_CREATED.to_owned(),
        Dialog::InstallEngine(_) => text::INSTALL_TITLE.to_owned(),
        Dialog::Maintenance(form) => format!("{} — {}", text::MAINTENANCE_TITLE, form.engine.id),
        Dialog::FabInstall(form) => form.asset.title.clone(),
        Dialog::EditorCrashed(_) => text::CRASH_TITLE.to_owned(),
        Dialog::ZipFound { .. } => text::ZIP_FOUND_TITLE.to_owned(),
    }
}

pub fn draw(
    dialog: &mut Option<Dialog>,
    context: &Context<'_>,
    ctx: &egui::Context,
    intents: &mut Vec<Intent>,
) {
    let engines = context.snapshot.map_or(&[][..], |s| s.engines());
    let Some(current) = dialog else {
        return;
    };
    // The first-run choice and a project being created cannot be closed
    // by Escape or a click outside.
    let busy = matches!(current, Dialog::NewProject(form) if form.creating);
    let closable = !busy && !matches!(current, Dialog::FirstRun(_));
    let heading = title(current);
    let close = dialog::show(
        ctx,
        Id::new("dialog"),
        &heading,
        closable,
        |ui| match current {
            Dialog::FirstRun(found) => first_run(ui, found, intents),
            Dialog::Confirm { body, action, .. } => confirm(ui, body, action, intents),
            Dialog::TypedConfirm {
                body,
                expected,
                typed,
                action,
                ..
            } => typed_confirm(ui, body, expected, typed, action, intents),
            Dialog::Error(error) => error_dialog(ui, error, intents),
            Dialog::TaskDetails {
                title,
                error,
                has_logs,
            } => task_details(ui, title, error, *has_logs, intents),
            Dialog::ChangeEngine {
                project,
                name,
                selected,
            } => change_engine(ui, project, name, selected, engines, intents),
            Dialog::NewProject(form) => new_project(ui, form, engines, context.images, intents),
            Dialog::InstallEngine(form) => install_engine(ui, form, context, intents),
            Dialog::FabInstall(form) => fab_install(ui, form, context, intents),
            Dialog::Maintenance(form) => maintenance(ui, form, intents),
            Dialog::ZipFound { found, destination } => zip_found(ui, found, destination, intents),
            Dialog::EditorCrashed(crash) => editor_crashed(ui, crash, intents),
            Dialog::Created(path) => created(ui, path, intents),
        },
    );
    if close {
        intents.push(Intent::CloseDialog);
    }
}

/// The footer of a dialog that asks: `ok` as the primary action, then
/// Cancel. Returns whether `ok` was clicked.
fn ask_footer(
    ui: &mut Ui,
    ok: &str,
    kind: Kind,
    why_not: Option<&str>,
    intents: &mut Vec<Intent>,
) -> bool {
    let mut clicked = false;
    footer(ui, |ui| {
        clicked = ui
            .add(Button::new(kind, ok).enabled(why_not.is_none(), why_not.unwrap_or_default()))
            .clicked();
        if ui.add(Button::secondary(text::CANCEL)).clicked() {
            intents.push(Intent::CloseDialog);
        }
    });
    clicked
}

/// A footer with only a Close button.
fn close_footer(ui: &mut Ui, intents: &mut Vec<Intent>) {
    footer(ui, |ui| {
        if ui.add(Button::secondary(text::CLOSE)).clicked() {
            intents.push(Intent::CloseDialog);
        }
    });
}

fn error_dialog(ui: &mut Ui, error: &crate::backend::UiError, intents: &mut Vec<Intent>) {
    body(ui, |ui| error_box(ui, error));
    close_footer(ui, intents);
}

/// Why a task failed, with a shortcut to the logs of builds.
fn task_details(
    ui: &mut Ui,
    title: &str,
    error: &crate::backend::UiError,
    has_logs: bool,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    body(ui, |ui| {
        ui.label(Text::BodyStrong.colored(title, p.ink));
        error_box(ui, error);
    });
    footer(ui, |ui| {
        if ui.add(Button::secondary(text::CLOSE)).clicked() {
            intents.push(Intent::CloseDialog);
        }
        if has_logs
            && ui
                .add(Button::secondary(text::OPEN_LOGS_FOLDER).icon(Icon::FolderOpen))
                .clicked()
        {
            intents.push(Intent::OpenLogsFolder);
        }
    });
}

/// The editor crashed: why, where the log is, and how to start it again.
fn editor_crashed(ui: &mut Ui, crash: &crate::backend::EditorCrash, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    body(ui, |ui| {
        ui.label(Text::Body.colored(format!("{} {}.", crash.label, crash.exit), p.ink));
        let explanation = match crash.cause.as_ref().map(|c| c.kind) {
            Some(ue_core::CrashKind::GpuLost) => text::CRASH_GPU_LOST,
            Some(ue_core::CrashKind::OutOfMemory) => text::CRASH_OUT_OF_MEMORY,
            Some(ue_core::CrashKind::Fatal) => text::CRASH_FATAL,
            None => text::CRASH_UNKNOWN,
        };
        ui.label(Text::Body.colored(explanation, p.ink_muted));
        if let Some(cause) = &crash.cause {
            ui.label(Text::Mono.colored(&cause.line, p.ink_muted));
        }
        path_label(ui, &crash.log.display().to_string());
    });
    footer(ui, |ui| {
        if ui
            .add(Button::primary(text::REOPEN).icon(Icon::Play))
            .clicked()
        {
            intents.push(Intent::Relaunch(crash.target.clone()));
        }
        if crash.log.exists()
            && ui
                .add(Button::secondary(text::OPEN_LOG).icon(Icon::FolderOpen))
                .clicked()
        {
            intents.push(Intent::OpenFile(crash.log.clone()));
        }
        if ui.add(Button::ghost(text::CLOSE)).clicked() {
            intents.push(Intent::CloseDialog);
        }
    });
}

/// An engine zip finished downloading in the browser.
fn zip_found(
    ui: &mut Ui,
    found: &ue_prebuilt::FoundEngineZip,
    destination: &std::path::Path,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    let free = crate::tasks::is_free_destination(destination);
    body(ui, |ui| {
        ui.label(Text::BodyStrong.colored(format!("Unreal Engine {}", found.version), p.ink));
        path_label(ui, &found.path.display().to_string());
        ui.label(Text::Body.colored(text::ZIP_FOUND_QUESTION, p.ink));
        ui.label(Text::Caption.colored(
            format!("{}: {}", text::INSTALL_TO, destination.display()),
            p.ink_muted,
        ));
        if !free {
            ui.label(Text::Body.colored(text::DESTINATION_NOT_EMPTY, p.warning));
        }
    });
    footer(ui, |ui| {
        if ui
            .add(
                Button::primary(text::IMPORT)
                    .icon(Icon::Download)
                    .enabled(free, text::DESTINATION_NOT_EMPTY),
            )
            .clicked()
        {
            intents.push(Intent::StartImport {
                zip: found.path.clone(),
                destination: destination.to_path_buf(),
            });
        }
        if ui
            .add(Button::secondary(text::CHOOSE_OTHER_FOLDER))
            .clicked()
        {
            intents.push(Intent::ChooseZip(found.path.clone()));
        }
        if ui.add(Button::ghost(text::NOT_NOW)).clicked() {
            intents.push(Intent::CloseDialog);
        }
    });
}

fn first_run(
    ui: &mut Ui,
    found: &mut [(ue_core::DiscoveredEngine, bool)],
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    body(ui, |ui| {
        if found.is_empty() {
            ui.label(Text::Body.colored(text::FIRST_RUN_NO_ENGINES, p.ink_muted));
        } else {
            ui.label(Text::Body.colored(text::FIRST_RUN_INTRO, p.ink_muted));
            for (engine, selected) in found.iter_mut() {
                ui.checkbox(
                    selected,
                    Text::Body.colored(
                        format!(
                            "Unreal Engine {} — {}",
                            engine.version,
                            engine.path.display()
                        ),
                        p.ink,
                    ),
                );
            }
            ui.label(Text::Caption.colored(text::BACKUP_NOTE, p.ink_faint));
        }
    });
    footer(ui, |ui| {
        if !found.is_empty() && ui.add(Button::primary(text::FIRST_RUN_REGISTER)).clicked() {
            let paths = found
                .iter()
                .filter(|(_, selected)| *selected)
                .map(|(engine, _)| engine.path.clone())
                .collect();
            intents.push(Intent::FinishFirstRun(paths));
        }
        let label = if found.is_empty() {
            text::CLOSE
        } else {
            text::NOT_NOW
        };
        if ui.add(Button::secondary(label)).clicked() {
            intents.push(Intent::FinishFirstRun(Vec::new()));
        }
    });
}

fn confirm(ui: &mut Ui, text_body: &str, action: &Action, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    body(ui, |ui| {
        ui.label(Text::Body.colored(text_body, p.ink_muted));
    });
    if ask_footer(ui, text::CONFIRM, Kind::Primary, None, intents) {
        intents.push(Intent::Execute(action.clone()));
    }
}

fn engine_combo(
    ui: &mut Ui,
    id: &str,
    engines: &[Engine],
    selected: Option<&Engine>,
) -> Option<Engine> {
    let mut picked = None;
    let label = selected.map_or_else(String::new, |e| format!("{} ({})", e.version, e.id));
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for engine in engines {
                let is_selected = selected.is_some_and(|s| s.id == engine.id);
                if ui
                    .selectable_label(is_selected, format!("{} ({})", engine.version, engine.id))
                    .clicked()
                {
                    picked = Some(engine.clone());
                }
            }
        });
    picked
}

fn change_engine(
    ui: &mut Ui,
    project: &std::path::Path,
    name: &str,
    selected: &mut Option<ue_core::EngineId>,
    engines: &[Engine],
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    body(ui, |ui| {
        ui.label(Text::Body.colored(format!("Which engine should {name} use?"), p.ink));
        let current = selected
            .as_ref()
            .and_then(|id| engines.iter().find(|e| &e.id == id));
        if let Some(engine) = engine_combo(ui, "change_engine", engines, current) {
            *selected = Some(engine.id);
        }
        ui.label(Text::Caption.colored(
            format!("{} {}", project.display(), text::BACKUP_NOTE),
            p.ink_faint,
        ));
    });
    let mut apply = false;
    footer(ui, |ui| {
        apply = ui
            .add(Button::primary(text::CONFIRM).enabled(selected.is_some(), text::PICK_AN_ENGINE))
            .clicked();
        if ui.add(Button::secondary(text::CANCEL)).clicked() {
            intents.push(Intent::CloseDialog);
        }
        if ui
            .add(Button::ghost(text::INSTALL_ENGINE_BUTTON).icon(Icon::Plus))
            .clicked()
        {
            intents.push(Intent::InstallEngine);
        }
    });
    if apply && let Some(engine) = selected.clone() {
        intents.push(Intent::Execute(Action::Associate {
            project: project.to_path_buf(),
            engine,
        }));
    }
}

fn new_project(
    ui: &mut Ui,
    form: &mut NewProjectForm,
    engines: &[Engine],
    images: &Images,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    if engines.is_empty() {
        body(ui, |ui| {
            ui.label(Text::Body.colored(text::NEEDS_AN_ENGINE, p.ink_muted));
        });
        close_footer(ui, intents);
        return;
    }
    body(ui, |ui| {
        ui.add_enabled_ui(!form.creating, |ui| {
            ui.label(Text::BodyStrong.colored(text::ENGINE, p.ink));
            if let Some(engine) =
                engine_combo(ui, "new_project_engine", engines, form.engine.as_ref())
            {
                intents.push(Intent::LoadTemplates(engine));
            }

            ui.label(Text::BodyStrong.colored(text::TEMPLATE, p.ink));
            match &form.templates {
                None => {
                    ui.spinner();
                }
                Some(Err(error)) => error_box(ui, error),
                Some(Ok(templates)) => {
                    template_list(ui, form.template, templates, images, &mut form.template);
                }
            }

            ui.label(Text::BodyStrong.colored(text::NAME, p.ink));
            text_field(ui, &mut form.name);
            ui.label(Text::BodyStrong.colored(text::LOCATION, p.ink));
            if path_row(ui, &form.location.display().to_string(), text::BROWSE) {
                intents.push(Intent::PickNewProjectLocation);
            }
        });
        if let Some(error) = &form.error {
            error_box(ui, error);
        }
        if form.creating {
            ui.spinner();
        }
    });

    let template = form
        .templates
        .as_ref()
        .and_then(|t| t.as_ref().ok())
        .zip(form.template)
        .and_then(|(templates, index)| templates.get(index));
    let why_not = if form.creating {
        Some(text::WORKING_STATUS)
    } else if template.is_none() {
        Some(text::PICK_A_TEMPLATE)
    } else if form.name.trim().is_empty() {
        Some(text::NAME_THE_PROJECT)
    } else {
        None
    };
    if ask_footer(ui, text::CREATE, Kind::Primary, why_not, intents)
        && let (Some(engine), Some(template)) =
            (form.engine.clone(), template.cloned().map(Box::new))
    {
        intents.push(Intent::CreateProject {
            engine,
            template,
            parent: form.location.clone(),
            name: form.name.trim().to_owned(),
        });
    }
}

/// The templates, grouped by category, and the picture of the chosen one.
fn template_list(
    ui: &mut Ui,
    chosen: Option<usize>,
    templates: &[ue_core::ProjectTemplate],
    images: &Images,
    selection: &mut Option<usize>,
) {
    let p = colors(ui);
    egui::ScrollArea::vertical()
        .max_height(size::TEMPLATE_LIST_HEIGHT)
        .show(ui, |ui| {
            let mut category = None;
            for (index, template) in templates.iter().enumerate() {
                if category != Some(&template.category) {
                    category = Some(&template.category);
                    ui.add_space(space::S1);
                    ui.label(Text::SectionLabel.colored(&template.category, p.ink_muted));
                }
                let label = if template.has_code {
                    format!("{} (C++)", template.name)
                } else {
                    template.name.clone()
                };
                let response = ui.add_enabled(
                    !template.has_code,
                    egui::Button::selectable(chosen == Some(index), label),
                );
                let response = match &template.description {
                    Some(description) => response.on_hover_text(description),
                    None => response,
                };
                if response.clicked() {
                    *selection = Some(index);
                }
            }
        });
    ui.label(Text::Caption.colored(text::CODE_TEMPLATE_NOTE, p.ink_faint));
    // The picture of the chosen template, from its Media folder.
    if let Some(template) = chosen.and_then(|index| templates.get(index))
        && let Some(path) = &template.thumbnail
    {
        let picture = images.texture(&ImageKey::File(path.clone()));
        let width = ui.available_width().min(size::TEMPLATE_PREVIEW_WIDTH);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width, width / size::THUMB_RATIO),
            egui::Sense::hover(),
        );
        thumbnail::paint(
            ui,
            rect,
            crate::theme::corner(crate::theme::radius::LG),
            picture,
            &template.name,
            thumbnail::Mark::Icon(Icon::Layers),
        );
    }
}

/// A confirmation for deletions: the button only works after typing `expected`.
fn typed_confirm(
    ui: &mut Ui,
    text_body: &str,
    expected: &str,
    typed: &mut String,
    action: &Action,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    let confirmed = typed.trim() == expected;
    body(ui, |ui| {
        ui.label(Text::Body.colored(text_body, p.ink_muted));
        ui.label(Text::BodyStrong.colored(format!("{} {expected}", text::TYPE_TO_CONFIRM), p.ink));
        text_field(ui, typed);
    });
    let label = match action {
        Action::Uninstall(_) => text::UNINSTALL,
        _ => text::DELETE,
    };
    let why_not = (!confirmed).then_some(text::TYPE_TO_ENABLE);
    if ask_footer(ui, label, Kind::Danger, why_not, intents) {
        intents.push(Intent::Execute(action.clone()));
    }
}

fn created(ui: &mut Ui, path: &std::path::Path, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    body(ui, |ui| {
        ui.label(Text::Body.colored(format!("Created {}", path.display()), p.ink_muted));
    });
    footer(ui, |ui| {
        if ui
            .add(Button::primary(text::OPEN).icon(Icon::Play))
            .clicked()
        {
            intents.push(Intent::CloseDialog);
            intents.push(Intent::OpenProject(path.to_path_buf()));
        }
        if ui.add(Button::secondary(text::CLOSE)).clicked() {
            intents.push(Intent::CloseDialog);
        }
    });
}
