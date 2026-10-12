// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared state of a [`TaskManager`](super::TaskManager) and small helpers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Notify;

use super::store::{self, TaskRecord};
use super::{MAX_FINISHED_TASKS, OnChange, TaskFactory};
use crate::error::{Error, Result};
use crate::reporter::{Control, TaskHandle};
use crate::task::{Task, TaskId, TaskView};

/// Shared state of a [`TaskManager`](super::TaskManager).
pub(super) struct Inner {
    pub(super) state_file: PathBuf,
    pub(super) on_change: OnChange,
    pub(super) tasks: Mutex<Tasks>,
    pub(super) factories: Mutex<HashMap<String, Arc<TaskFactory>>>,
    /// Wakes a worker when a task is queued.
    pub(super) work: Notify,
    /// Wakes the persister when a state changed.
    pub(super) persist: Notify,
    /// Serializes writes of the state file.
    pub(super) write_lock: Mutex<()>,
}

/// The task list.
pub(super) struct Tasks {
    pub(super) next_id: u64,
    /// In submission order.
    pub(super) entries: Vec<Entry>,
}

/// One task: its record plus live parts.
pub(super) struct Entry {
    pub(super) record: TaskRecord,
    /// The task, until a worker takes it.
    pub(super) task: Option<Box<dyn Task>>,
    /// The control side, while it runs.
    pub(super) handle: Option<TaskHandle>,
    pub(super) is_cancelling: bool,
}

impl Entry {
    pub(super) fn view(&self) -> TaskView {
        let progress = self
            .handle
            .as_ref()
            .map_or_else(|| self.record.progress.clone(), TaskHandle::progress);
        self.record.view(progress, self.is_cancelling)
    }
}

impl Inner {
    /// Requests a state file write and tells the owner.
    pub(super) fn changed(&self) {
        self.persist.notify_one();
        (self.on_change)();
    }

    /// Writes the current state to the state file.
    pub(super) fn save(&self) -> Result<()> {
        let _guard = lock(&self.write_lock);
        let snapshot = {
            let tasks = lock(&self.tasks);
            let records = tasks
                .entries
                .iter()
                .map(|entry| {
                    let mut record = entry.record.clone();
                    if let Some(handle) = &entry.handle {
                        record.progress = handle.progress();
                    }
                    record
                })
                .collect();
            store::snapshot(tasks.next_id, records)
        };
        store::save(&self.state_file, &snapshot)
    }
}

/// Index of task `id`.
pub(super) fn position(tasks: &Tasks, id: TaskId) -> Result<usize> {
    tasks
        .entries
        .iter()
        .position(|e| e.record.id == id)
        .ok_or(Error::UnknownTask { id })
}

/// [`Error::InvalidState`] for `entry`.
pub(super) fn invalid(entry: &Entry, action: &'static str) -> Error {
    Error::InvalidState {
        id: entry.record.id,
        state: entry.record.state,
        action,
    }
}

/// Sends `control` to a running task.
pub(super) fn send(entry: &Entry, control: Control) {
    if let Some(handle) = &entry.handle {
        handle.send(control);
    }
}

/// Drops the oldest finished tasks beyond [`MAX_FINISHED_TASKS`].
pub(super) fn prune(entries: &mut Vec<Entry>) {
    let mut finished: Vec<(u64, TaskId)> = entries
        .iter()
        .filter(|e| e.record.state.is_finished())
        .map(|e| (e.record.finished_at.unwrap_or(0), e.record.id))
        .collect();
    if finished.len() <= MAX_FINISHED_TASKS {
        return;
    }
    finished.sort_unstable();
    let excess = finished.len() - MAX_FINISHED_TASKS;
    let dropped: Vec<TaskId> = finished[..excess].iter().map(|(_, id)| *id).collect();
    entries.retain(|e| !dropped.contains(&e.record.id));
}

/// Locks `mutex`, ignoring poisoning (the data stays consistent because
/// critical sections never panic half-way through an update).
pub(super) fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Seconds since the Unix epoch.
pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
