// SPDX-License-Identifier: GPL-3.0-or-later

//! Tasks: downloads, imports and builds, in progress and finished.

use egui::Ui;
use jobs::{TaskState, TaskView};

use super::page_header;
use super::widgets::button::{Button, IconButton};
use super::widgets::empty_state::empty_state;
use super::widgets::gap;
use super::widgets::section_header::section_header;
use super::widgets::task_row::{TaskRow, Tone, task_row};
use crate::app::{Intent, LauncherApp, Screen, TaskOp};
use crate::backend::UiError;
use crate::text;
use crate::theme::{Icon, space};

pub fn draw(app: &LauncherApp, tasks: &[TaskView], ui: &mut Ui, intents: &mut Vec<Intent>) {
    let any_finished = tasks.iter().any(|task| task.state.is_finished());
    page_header(ui, app, text::TASKS_TITLE, |ui| {
        if any_finished && ui.add(Button::ghost(text::CLEAR_FINISHED)).clicked() {
            intents.push(Intent::ClearFinished);
        }
    });
    if tasks.is_empty() {
        empty_state(
            ui,
            Icon::ListChecks,
            text::EMPTY_TASKS_TITLE,
            text::EMPTY_TASKS_BODY,
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
    // The queue in its order; finished ones, the newest first.
    let mut running: Vec<&TaskView> = tasks.iter().filter(|t| !t.state.is_finished()).collect();
    running.sort_by_key(|task| task.id);
    let mut finished: Vec<&TaskView> = tasks.iter().filter(|t| t.state.is_finished()).collect();
    finished.sort_by_key(|task| std::cmp::Reverse(task.id));

    for (label, list) in [(text::IN_PROGRESS, running), (text::FINISHED, finished)] {
        if list.is_empty() {
            continue;
        }
        section_header(ui, label, |_| {});
        for task in list {
            row(ui, task, intents);
            ui.add_space(space::S2);
        }
        gap(ui, space::S4);
    }
}

/// The icon of a task by what it does.
pub fn kind_icon(kind: &str) -> Icon {
    match kind {
        "download-engine" => Icon::Download,
        "build-engine" | "update-engine" => Icon::Hammer,
        "fab-install" => Icon::Package,
        _ => Icon::FolderOpen,
    }
}

/// How a task is drawn.
pub fn tone(task: &TaskView) -> Tone {
    match task.state {
        TaskState::Queued | TaskState::Running => Tone::Running,
        TaskState::Paused => Tone::Paused,
        TaskState::Done => Tone::Done,
        TaskState::Failed | TaskState::Interrupted => Tone::Failed,
        TaskState::Cancelled => Tone::Stopped,
    }
}

/// The right-aligned figures: progress and the state when it is not plain
/// running.
pub fn figures(task: &TaskView) -> String {
    let mut parts: Vec<String> = Vec::new();
    if matches!(
        task.state,
        TaskState::Queued | TaskState::Running | TaskState::Paused
    ) && let Some(fraction) = task.progress.fraction
    {
        parts.push(format!("{:.0} %", fraction.clamp(0.0, 1.0) * 100.0));
    }
    let state = if task.is_cancelling {
        Some(text::STATE_CANCELLING)
    } else {
        match task.state {
            TaskState::Queued => Some(text::STATE_QUEUED),
            TaskState::Paused => Some(text::STATE_PAUSED),
            TaskState::Cancelled => Some(text::STATE_CANCELLED),
            TaskState::Interrupted => Some(text::STATE_INTERRUPTED),
            TaskState::Failed => Some(text::STATE_FAILED),
            TaskState::Running | TaskState::Done => None,
        }
    };
    parts.extend(state.map(str::to_owned));
    parts.join(" · ")
}

/// The line under the bar: the current step, the result or the error.
pub fn step(task: &TaskView) -> String {
    if let Some(error) = &task.error {
        return error.summary.clone();
    }
    if let Some(outcome) = &task.outcome {
        return outcome.clone();
    }
    match &task.progress.message {
        Some(message) if !message.is_empty() => message.clone(),
        _ => task.progress.stage.clone(),
    }
}

fn row(ui: &mut Ui, task: &TaskView, intents: &mut Vec<Intent>) {
    let fraction = match task.state {
        TaskState::Done => Some(1.0),
        _ => task.progress.fraction,
    };
    let data = TaskRow {
        icon: kind_icon(&task.kind),
        tone: tone(task),
        title: &task.title,
        figures: &figures(task),
        fraction,
        step: &step(task),
        animate: task.state == TaskState::Running,
    };
    task_row(ui, &data, |ui| actions(ui, task, intents));
}

/// The buttons of a row (the first one added is the rightmost).
fn actions(ui: &mut Ui, task: &TaskView, intents: &mut Vec<Intent>) {
    let op = |op| Intent::Task(task.id, op);
    match task.state {
        TaskState::Queued | TaskState::Running | TaskState::Paused => {
            if task.is_cancelling {
                return;
            }
            if ui.add(IconButton::new(Icon::X, text::CANCEL)).clicked() {
                intents.push(op(TaskOp::Cancel));
            }
            match task.state {
                TaskState::Running
                    if task.pausable
                        && ui.add(IconButton::new(Icon::Pause, text::PAUSE)).clicked() =>
                {
                    intents.push(op(TaskOp::Pause));
                }
                TaskState::Paused
                    if task.pausable
                        && ui
                            .add(Button::secondary(text::RESUME).icon(Icon::Play))
                            .clicked() =>
                {
                    intents.push(op(TaskOp::Resume));
                }
                _ => {}
            }
        }
        TaskState::Done => {
            if ui.add(IconButton::new(Icon::X, text::REMOVE)).clicked() {
                intents.push(op(TaskOp::Remove));
            }
            if ui
                .add(Button::secondary(text::SHOW).icon(Icon::FolderOpen))
                .clicked()
            {
                intents.push(Intent::Navigate(Screen::Library));
            }
        }
        TaskState::Failed | TaskState::Cancelled | TaskState::Interrupted => {
            if ui.add(IconButton::new(Icon::X, text::REMOVE)).clicked() {
                intents.push(op(TaskOp::Remove));
            }
            if task.state == TaskState::Failed && ui.add(Button::secondary(text::DETAILS)).clicked()
            {
                let error = task.error.as_ref().map_or_else(
                    || UiError::new(step(task)),
                    |error| UiError {
                        summary: error.summary.clone(),
                        hint: error.hint.clone(),
                    },
                );
                intents.push(Intent::TaskDetails {
                    title: task.title.clone(),
                    error,
                });
            }
            if task.state.can_retry()
                && ui
                    .add(Button::secondary(text::RETRY).icon(Icon::RefreshCw))
                    .clicked()
            {
                intents.push(op(TaskOp::Retry));
            }
        }
    }
}
