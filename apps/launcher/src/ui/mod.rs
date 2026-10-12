// SPDX-License-Identifier: GPL-3.0-or-later

//! Drawing. Nothing here touches disk: it reads the app state and pushes
//! [`Intent`]s. Colors, sizes and spaces come from `theme`; the components
//! are in `widgets`.

mod dialogs;
mod fab;
mod library;
mod settings;
mod tasks;
pub mod widgets;

use egui::{Align, Frame, Layout, ScrollArea, Ui};

use crate::app::{Intent, LauncherApp, Screen};
use crate::backend::UiError;
use crate::text;
use crate::theme::{self, Text, size, space};
use widgets::sidebar::{SidebarView, sidebar};

pub fn draw(app: &mut LauncherApp, ui: &mut Ui, intents: &mut Vec<Intent>) {
    let p = theme::palette(ui.ctx());
    let tasks = app.tasks.list();
    let failed = tasks
        .iter()
        .filter(|task| {
            matches!(
                task.state,
                jobs::TaskState::Failed | jobs::TaskState::Interrupted
            )
        })
        .count();
    let active = tasks
        .iter()
        .filter(|task| !task.state.is_finished())
        .count();
    let account = account_summary(app);
    let view = SidebarView {
        current: app.screen,
        active_tasks: active + failed,
        tasks_failed: failed > 0,
        brand: app.images.brand_mark(),
        account_name: &account.0,
        account_detail: &account.1,
    };
    if let Some(screen) = sidebar(ui, &view) {
        intents.push(Intent::Navigate(screen));
    }

    let frame = Frame::new()
        .fill(p.bg_100)
        .inner_margin(theme::margin(space::S6));
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // `set_max_width` can also widen: only ever narrow it.
                ui.set_max_width(ui.available_width().min(size::CONTENT_MAX_WIDTH));
                match app.screen {
                    Screen::Library => library::draw(app, ui, intents),
                    Screen::Fab => fab::draw(app, ui, intents),
                    Screen::Tasks => tasks::draw(app, &tasks, ui, intents),
                    Screen::Settings => settings::draw(app, ui, intents),
                }
            });
    });

    let ctx = ui.ctx().clone();
    let context = dialogs::Context {
        snapshot: app.snapshot.as_ref(),
        library: &app.library,
        github: &app.github,
        images: &app.images,
    };
    dialogs::draw(&mut app.dialog, &context, &ctx, intents);
}

/// The name and detail line of the account row in the sidebar.
fn account_summary(app: &LauncherApp) -> (String, String) {
    let epic = app.library.epic.display_name().map(str::to_owned);
    let github = app.github.saved == Some(true);
    let mut connected = Vec::new();
    if epic.is_some() {
        connected.push(text::ACCOUNT_EPIC_SHORT);
    }
    if github {
        connected.push(text::ACCOUNT_GITHUB_SHORT);
    }
    let detail = if connected.is_empty() {
        text::ACCOUNTS_NONE.to_owned()
    } else {
        connected.join(" · ")
    };
    (
        epic.unwrap_or_else(|| text::NOT_SIGNED_IN.to_owned()),
        detail,
    )
}

/// The title row of a screen: the title, what the app is doing and, on the
/// right, the screen's actions (the first one added is the rightmost).
fn page_header(ui: &mut Ui, app: &LauncherApp, title: &str, actions: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(Text::PageTitle.colored(title, p.ink));
        if app.loading || app.busy > 0 {
            ui.spinner();
            ui.label(Text::Caption.colored(text::WORKING_STATUS, p.ink_muted));
        } else if let Some(Ok(message)) = &app.status {
            ui.label(Text::Caption.colored(message, p.ink_muted));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    widgets::gap(ui, space::S6);
}

/// What went wrong and what to do about it.
fn error_box(ui: &mut Ui, error: &UiError) {
    let p = theme::palette(ui.ctx());
    ui.label(Text::Body.colored(&error.summary, p.danger));
    if let Some(hint) = &error.hint {
        ui.label(Text::Caption.colored(hint, p.ink_muted));
    }
}

/// A path for display, with the home directory shown as `~`.
pub fn short_path(path: &std::path::Path, home: &std::path::Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::short_path;

    #[test]
    fn shortens_home() {
        let home = Path::new("/home/me");
        assert_eq!(short_path(Path::new("/home/me/UE/5.7"), home), "~/UE/5.7");
        assert_eq!(short_path(Path::new("/opt/UE"), home), "/opt/UE");
        assert_eq!(short_path(home, home), "~");
    }
}
