// SPDX-License-Identifier: GPL-3.0-or-later

//! The "Connect GitHub" steps of Install engine → From source.

use egui::Ui;
use github_auth::Access;

use crate::app::{GhLogin, GithubState, Intent, LibraryState};
use crate::text;
use crate::theme::{Icon, Text, space};
use crate::ui::widgets::button::Button;
use crate::ui::widgets::colors;
use crate::ui::widgets::dialog::{self, Step, StepState};

/// Where a step stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Progress {
    Done,
    Todo,
    /// Waits for an earlier step.
    Later,
}

impl Progress {
    fn state(self) -> StepState {
        match self {
            Self::Done => StepState::Done,
            Self::Todo => StepState::Current,
            Self::Later => StepState::Todo,
        }
    }
}

fn check_again(ui: &mut Ui, github: &GithubState, intents: &mut Vec<Intent>) {
    ui.horizontal(|ui| {
        if ui
            .add(
                Button::secondary(text::CHECK_AGAIN)
                    .icon(Icon::RefreshCw)
                    .enabled(!github.checking, text::CHECKING),
            )
            .clicked()
        {
            intents.push(Intent::GithubRefresh);
        }
        if github.checking || github.detecting {
            ui.spinner();
        }
    });
}

#[allow(
    clippy::too_many_lines,
    reason = "one list of steps, drawn top to bottom"
)]
pub fn connect_github(
    ui: &mut Ui,
    library: &LibraryState,
    github: &GithubState,
    intents: &mut Vec<Intent>,
) {
    let p = colors(ui);
    ui.separator();
    ui.label(Text::CardTitle.colored(text::CONNECT_GITHUB_TITLE, p.ink));
    ui.label(Text::Body.colored(text::CONNECT_GITHUB_HELP, p.ink_muted));
    ui.add_space(space::S1);

    let access = github.access();
    let epic_done = library.epic.is_signed_in();
    let epic_detail = match library.epic.display_name() {
        Some(name) => format!("{} {name}", text::CONNECTED_AS),
        None => text::STEP_EPIC_TODO.to_owned(),
    };
    let connected = access.is_some_and(|a| a.login().is_some());
    let github_detail = match github.login() {
        Some(login) => format!("{} @{login}", text::CONNECTED_AS),
        None => text::STEP_GITHUB_TODO.to_owned(),
    };
    let (link, invite) = match access {
        Some(Access::Ready { .. } | Access::Blocked { .. }) => (Progress::Done, Progress::Done),
        Some(Access::InvitationPending { .. }) => (Progress::Done, Progress::Todo),
        Some(Access::NotLinked { .. }) => (Progress::Todo, Progress::Later),
        _ => (Progress::Later, Progress::Later),
    };
    let ready = if github.is_ready() {
        Progress::Done
    } else {
        Progress::Later
    };
    let ready_detail = if github.is_ready() {
        text::STEP_READY_DONE
    } else {
        text::STEP_READY_TODO
    };
    let items = [
        step(
            1,
            if epic_done {
                Progress::Done
            } else {
                Progress::Todo
            },
            text::STEP_EPIC,
            &epic_detail,
        ),
        step(
            2,
            if connected {
                Progress::Done
            } else {
                Progress::Todo
            },
            text::STEP_GITHUB,
            &github_detail,
        ),
        step(3, link, text::STEP_LINK, link_detail(link)),
        step(4, invite, text::STEP_INVITE, invite_detail(invite)),
        step(5, ready, text::STEP_READY, ready_detail),
    ];
    dialog::steps(ui, &items, |ui, index| match index {
        0 if !epic_done => {
            if ui
                .add(Button::primary(text::SIGN_IN_EPIC).icon(Icon::LogIn))
                .clicked()
            {
                intents.push(Intent::SignInWithWindow);
            }
        }
        1 if !connected => github_choices(ui, github, intents),
        2 if link == Progress::Todo => {
            if github.linking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(Text::Caption.colored(text::LINKING_WINDOW, p.ink_muted));
                });
            } else if ui
                .add(Button::primary(text::OPEN_EPIC_CONNECTIONS).icon(Icon::ExternalLink))
                .clicked()
            {
                intents.push(Intent::OpenEpicConnections);
            }
            check_again(ui, github, intents);
        }
        3 if invite == Progress::Todo => {
            if ui
                .add(Button::primary(text::OPEN_INVITATION).icon(Icon::ExternalLink))
                .clicked()
            {
                intents.push(Intent::OpenGithubInvitation);
            }
            check_again(ui, github, intents);
        }
        _ => {}
    });

    if matches!(access, Some(Access::Blocked { .. })) {
        ui.label(Text::Body.colored(text::GITHUB_BLOCKED, p.warning));
        if !code_in_progress(ui, github, intents) {
            existing_offers(ui, github, intents);
            gh_option(ui, github, intents);
        }
    }
    if let Some(error) = &github.error {
        crate::ui::error_box(ui, error);
    }
    if let Some(Err(error)) = &github.access {
        crate::ui::error_box(ui, error);
    }
}

fn step<'a>(number: usize, progress: Progress, title: &'a str, detail: &'a str) -> Step<'a> {
    Step {
        state: progress.state(),
        number,
        title,
        detail: Some(detail),
    }
}

fn link_detail(progress: Progress) -> &'static str {
    match progress {
        Progress::Done => text::STEP_LINK_DONE,
        _ => text::STEP_LINK_TODO,
    }
}

fn invite_detail(progress: Progress) -> &'static str {
    match progress {
        Progress::Done => text::STEP_INVITE_DONE,
        _ => text::STEP_INVITE_TODO,
    }
}

/// Step 2 when GitHub is not connected: this computer's account, the GitHub
/// CLI (approved by EpicGames), or UnLauncher's own GitHub app.
fn github_choices(ui: &mut Ui, github: &GithubState, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    if matches!(github.access(), Some(Access::Revoked)) {
        ui.label(Text::Caption.colored(text::GITHUB_REVOKED, p.warning));
    }
    if code_in_progress(ui, github, intents) {
        return;
    }
    existing_offers(ui, github, intents);
    gh_option(ui, github, intents);
    ui.horizontal(|ui| {
        if ui
            .add(Button::secondary(text::CONNECT_WITH_GITHUB))
            .clicked()
        {
            intents.push(Intent::GithubConnect);
        }
        if github.checking || github.detecting {
            ui.spinner();
        }
    });
}

/// The code to type on GitHub, while the GitHub CLI or the Device Flow
/// waits. Returns whether one is in progress.
pub fn code_in_progress(ui: &mut Ui, github: &GithubState, intents: &mut Vec<Intent>) -> bool {
    let p = colors(ui);
    let code = match (&github.gh_login, &github.device) {
        (Some(GhLogin::Code(code)), _) => Some(code.as_str()),
        (Some(GhLogin::Starting), _) => None,
        (None, Some(device)) => Some(device.user_code.as_str()),
        (None, None) => return false,
    };
    match code {
        Some(code) => {
            dialog::code(ui, code);
            ui.label(Text::Caption.colored(text::DEVICE_CODE_HELP, p.ink_muted));
        }
        None => {
            ui.label(Text::Caption.colored(text::GH_STARTING, p.ink_muted));
        }
    }
    ui.horizontal(|ui| {
        ui.spinner();
        if code.is_some()
            && ui
                .add(Button::primary(text::OPEN_GITHUB_PAGE).icon(Icon::ExternalLink))
                .clicked()
        {
            intents.push(Intent::OpenUrl(text::GITHUB_DEVICE_URL.to_owned()));
        }
        if ui.add(Button::ghost(text::CANCEL)).clicked() {
            intents.push(Intent::GithubCancelDevice);
        }
    });
    true
}

/// "Connect with the GitHub CLI", or how to install it.
pub fn gh_option(ui: &mut Ui, github: &GithubState, intents: &mut Vec<Intent>) {
    let p = colors(ui);
    match github.gh_installed {
        Some(true) => {
            ui.label(Text::Caption.colored(text::GH_HELP, p.ink_muted));
            if ui
                .add(Button::primary(text::CONNECT_WITH_GH).icon(Icon::LogIn))
                .clicked()
            {
                intents.push(Intent::GithubGhLogin);
            }
        }
        Some(false) => {
            ui.label(Text::Caption.colored(text::GH_HELP, p.ink_muted));
            ui.label(Text::Caption.colored(text::GH_MISSING, p.ink));
            if ui
                .add(
                    Button::secondary(text::CHECK_AGAIN)
                        .icon(Icon::RefreshCw)
                        .enabled(!github.detecting, text::CHECKING),
                )
                .clicked()
            {
                intents.push(Intent::GithubRefresh);
            }
        }
        None => {}
    }
}

/// "Use this computer's GitHub account (@login)" for each credential that
/// can read the repository.
fn existing_offers(ui: &mut Ui, github: &GithubState, intents: &mut Vec<Intent>) {
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
}
