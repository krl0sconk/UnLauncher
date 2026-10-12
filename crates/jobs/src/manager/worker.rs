// SPDX-License-Identifier: GPL-3.0-or-later

//! The background loops of a [`TaskManager`](super::TaskManager): workers
//! that run tasks and the persister that writes the state file.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

use super::inner::{Inner, lock, now, prune};
use crate::reporter::{Progress, Reporter, channel};
use crate::task::{Task, TaskError, TaskFailure, TaskId, TaskOutcome, TaskState};

/// Minimum time between two progress-triggered `on_change` calls.
const PROGRESS_NOTIFY_INTERVAL: Duration = Duration::from_millis(100);

/// A task taken from the queue, ready to run.
struct Started {
    id: TaskId,
    task: Box<dyn Task>,
    reporter: Reporter,
    progress: watch::Receiver<Progress>,
}

/// Runs queued tasks one after another, forever.
pub(super) async fn run_worker(inner: Arc<Inner>) {
    loop {
        let Some(started) = start_next(&inner) else {
            inner.work.notified().await;
            continue;
        };
        let id = started.id;
        tracing::info!(task_id = %id, "task started");
        inner.changed();
        let result = supervise(&inner, started).await;
        finish(&inner, id, result);
    }
}

/// Writes the state file whenever a change is reported, forever.
pub(super) async fn run_persister(inner: Arc<Inner>) {
    loop {
        inner.persist.notified().await;
        let writer = Arc::clone(&inner);
        match tokio::task::spawn_blocking(move || writer.save()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "cannot save the task list"),
            Err(error) => tracing::warn!(%error, "the task list writer stopped"),
        }
    }
}

/// Marks the oldest queued task as running and hands it out.
fn start_next(inner: &Inner) -> Option<Started> {
    let mut tasks = lock(&inner.tasks);
    let entry = tasks
        .entries
        .iter_mut()
        .find(|e| e.record.state == TaskState::Queued && e.task.is_some())?;
    let task = entry.task.take()?;
    let (reporter, handle) = channel();
    let progress = handle.subscribe();
    entry.record.state = TaskState::Running;
    entry.handle = Some(handle);
    Some(Started {
        id: entry.record.id,
        task,
        reporter,
        progress,
    })
}

/// Runs the task on its own tokio task, forwarding progress to `on_change`
/// (throttled, with a trailing call so the last update is never lost).
async fn supervise(inner: &Inner, started: Started) -> Result<TaskOutcome, TaskError> {
    let Started {
        id,
        task,
        reporter,
        mut progress,
    } = started;
    let mut join = tokio::spawn(task.run(reporter));
    let mut last_notify: Option<Instant> = None;
    let mut is_pending = false;
    let mut has_progress = true;
    let joined = loop {
        let deadline = last_notify.map_or_else(Instant::now, |t| t + PROGRESS_NOTIFY_INTERVAL);
        tokio::select! {
            joined = &mut join => break joined,
            changed = progress.changed(), if has_progress => {
                if changed.is_err() {
                    has_progress = false;
                } else if Instant::now() >= deadline {
                    (inner.on_change)();
                    last_notify = Some(Instant::now());
                    is_pending = false;
                } else {
                    is_pending = true;
                }
            }
            () = tokio::time::sleep_until(deadline), if is_pending => {
                (inner.on_change)();
                last_notify = Some(Instant::now());
                is_pending = false;
            }
        }
    };
    joined.unwrap_or_else(|error| {
        tracing::error!(task_id = %id, %error, "task stopped unexpectedly");
        Err(TaskError::failed(
            "the task stopped unexpectedly",
            Some("This is a bug in UnLauncher; please report it.".to_owned()),
        ))
    })
}

/// Records the result of task `id`.
fn finish(inner: &Inner, id: TaskId, result: Result<TaskOutcome, TaskError>) {
    {
        let mut tasks = lock(&inner.tasks);
        let Some(entry) = tasks.entries.iter_mut().find(|e| e.record.id == id) else {
            return;
        };
        if let Some(handle) = entry.handle.take() {
            entry.record.progress = handle.progress();
        }
        entry.is_cancelling = false;
        entry.record.finished_at = Some(now());
        match result {
            Ok(outcome) => {
                tracing::info!(task_id = %id, message = %outcome.message, "task done");
                entry.record.state = TaskState::Done;
                entry.record.outcome = Some(outcome.message);
            }
            Err(TaskError::Cancelled) => {
                tracing::info!(task_id = %id, "task cancelled");
                entry.record.state = TaskState::Cancelled;
            }
            Err(TaskError::Failed { summary, hint }) => {
                tracing::error!(task_id = %id, %summary, "task failed");
                entry.record.state = TaskState::Failed;
                entry.record.error = Some(TaskFailure { summary, hint });
            }
        }
        prune(&mut tasks.entries);
    }
    inner.changed();
}
