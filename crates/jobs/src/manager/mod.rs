// SPDX-License-Identifier: GPL-3.0-or-later

//! The task queue: [`TaskManager`].

mod inner;
mod store;
mod worker;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::error::{Error, Result};
use crate::reporter::Control;
use crate::task::{Task, TaskError, TaskId, TaskState, TaskView};
use inner::{Entry, Inner, Tasks, invalid, lock, now, position, prune, send};
use store::TaskRecord;

/// How many tasks run at the same time. Engine builds and multi-gigabyte
/// downloads saturate the CPU, disk and network, so they run one at a time,
/// in submission (FIFO) order.
pub const MAX_RUNNING_TASKS: usize = 1;

/// How many finished tasks are kept in the list (and the state file); older
/// ones are dropped first.
pub const MAX_FINISHED_TASKS: usize = 50;

/// Recreates a task from its [`Task::spec`]; see
/// [`TaskManager::register_factory`].
pub type TaskFactory =
    dyn Fn(serde_json::Value) -> std::result::Result<Box<dyn Task>, TaskError> + Send + Sync;

/// Callback run whenever the task list changes.
pub(super) type OnChange = Arc<dyn Fn() + Send + Sync>;

/// A queue of long-running tasks with states, progress and persistence.
///
/// Cheap to clone; clones share the same queue.
///
/// Design:
///
/// - All bookkeeping lives in one `std::sync::Mutex` held only for short,
///   non-async sections, so the GUI thread can call [`TaskManager::list`]
///   every frame without blocking on I/O.
/// - [`MAX_RUNNING_TASKS`] worker loops (one) take the oldest
///   [`TaskState::Queued`] task, run it on its own tokio task and record the
///   result. A paused task keeps its slot: pausing a build does not start the
///   next download.
/// - A persister loop writes the state file on a blocking thread whenever a
///   state changes (never on progress ticks). Requests are coalesced, and each
///   write takes a fresh snapshot, so the file always ends with the latest
///   state.
#[derive(Clone)]
pub struct TaskManager {
    inner: Arc<Inner>,
}

impl fmt::Debug for TaskManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskManager")
            .field("state_file", &self.inner.state_file)
            .field("tasks", &lock(&self.inner.tasks).entries.len())
            .finish_non_exhaustive()
    }
}

impl TaskManager {
    /// Creates the manager, loads `state_file` and starts the workers on the
    /// current tokio runtime.
    ///
    /// Tasks that were queued, running or paused when the state file was last
    /// written become [`TaskState::Interrupted`]. A missing state file starts
    /// an empty list; a corrupt one is renamed to `<state_file>.corrupt` and
    /// an empty list is started (a warning is logged).
    ///
    /// `on_change` is called from any thread whenever a state changes, and at
    /// most about ten times per second for progress; a GUI uses it to request
    /// a repaint. It must be cheap and must not block.
    ///
    /// The workers live as long as the runtime, so create the manager once.
    ///
    /// # Errors
    ///
    /// [`Error::NoRuntime`] if called outside a tokio runtime.
    pub fn new(state_file: PathBuf, on_change: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| Error::NoRuntime)?;
        let state = store::load(&state_file, now());
        let had_tasks = !state.tasks.is_empty();
        let mut entries: Vec<Entry> = state
            .tasks
            .into_iter()
            .map(|record| Entry {
                record,
                task: None,
                handle: None,
                is_cancelling: false,
            })
            .collect();
        prune(&mut entries);
        let inner = Arc::new(Inner {
            state_file,
            on_change,
            tasks: Mutex::new(Tasks {
                next_id: state.next_id.max(1),
                entries,
            }),
            factories: Mutex::new(HashMap::new()),
            work: Notify::new(),
            persist: Notify::new(),
            write_lock: Mutex::new(()),
        });
        for _ in 0..MAX_RUNNING_TASKS {
            runtime.spawn(worker::run_worker(Arc::clone(&inner)));
        }
        runtime.spawn(worker::run_persister(Arc::clone(&inner)));
        if had_tasks {
            // Store the Interrupted states.
            inner.persist.notify_one();
        }
        Ok(Self { inner })
    }

    /// Registers how to recreate tasks of `kind` from their
    /// [`Task::spec`], for [`TaskManager::retry`]. Replaces any factory
    /// already registered for `kind`.
    pub fn register_factory<F>(&self, kind: impl Into<String>, factory: F)
    where
        F: Fn(serde_json::Value) -> std::result::Result<Box<dyn Task>, TaskError>
            + Send
            + Sync
            + 'static,
    {
        lock(&self.inner.factories).insert(kind.into(), Arc::new(factory));
    }

    /// Adds `task` at the end of the queue and returns its id. It starts when
    /// every task submitted before it has finished.
    pub fn submit(&self, task: Box<dyn Task>) -> TaskId {
        let record = TaskRecord {
            id: TaskId(0),
            kind: task.kind().to_owned(),
            title: task.title(),
            spec: task.spec(),
            pausable: task.pausable(),
            state: TaskState::Queued,
            progress: crate::Progress::default(),
            created_at: now(),
            finished_at: None,
            outcome: None,
            error: None,
        };
        let id = {
            let mut tasks = lock(&self.inner.tasks);
            let id = TaskId(tasks.next_id);
            tasks.next_id += 1;
            tasks.entries.push(Entry {
                record: TaskRecord { id, ..record },
                task: Some(task),
                handle: None,
                is_cancelling: false,
            });
            id
        };
        tracing::info!(task_id = %id, "task queued");
        self.inner.work.notify_one();
        self.inner.changed();
        id
    }

    /// Pauses a running task. It stops at its next checkpoint and keeps its
    /// slot, so the next queued task does not start.
    ///
    /// # Errors
    ///
    /// - [`Error::UnknownTask`] if there is no such task.
    /// - [`Error::NotPausable`] if the task does not support pausing.
    /// - [`Error::InvalidState`] if it is not [`TaskState::Running`].
    pub fn pause(&self, id: TaskId) -> Result<()> {
        self.control(id, "pause", |entry| {
            if !entry.record.pausable {
                return Err(Error::NotPausable { id });
            }
            if entry.record.state != TaskState::Running || entry.is_cancelling {
                return Err(invalid(entry, "pause"));
            }
            entry.record.state = TaskState::Paused;
            send(entry, Control::Pause);
            Ok(())
        })
    }

    /// Resumes a paused task.
    ///
    /// # Errors
    ///
    /// - [`Error::UnknownTask`] if there is no such task.
    /// - [`Error::InvalidState`] if it is not [`TaskState::Paused`].
    pub fn resume(&self, id: TaskId) -> Result<()> {
        self.control(id, "resume", |entry| {
            if entry.record.state != TaskState::Paused || entry.is_cancelling {
                return Err(invalid(entry, "resume"));
            }
            entry.record.state = TaskState::Running;
            send(entry, Control::Run);
            Ok(())
        })
    }

    /// Cancels a task. A queued task becomes [`TaskState::Cancelled`] at
    /// once; a running or paused one is asked to stop and becomes
    /// [`TaskState::Cancelled`] when it does (until then its view has
    /// `is_cancelling` set). Cancelling twice is not an error.
    ///
    /// # Errors
    ///
    /// - [`Error::UnknownTask`] if there is no such task.
    /// - [`Error::InvalidState`] if the task already finished.
    pub fn cancel(&self, id: TaskId) -> Result<()> {
        self.control(id, "cancel", |entry| match entry.record.state {
            TaskState::Queued => {
                entry.task = None;
                entry.record.state = TaskState::Cancelled;
                entry.record.finished_at = Some(now());
                Ok(())
            }
            TaskState::Running | TaskState::Paused => {
                entry.is_cancelling = true;
                send(entry, Control::Cancel);
                Ok(())
            }
            _ => Err(invalid(entry, "cancel")),
        })
    }

    /// Removes a finished task from the list.
    ///
    /// # Errors
    ///
    /// - [`Error::UnknownTask`] if there is no such task.
    /// - [`Error::InvalidState`] if the task has not finished.
    pub fn remove(&self, id: TaskId) -> Result<()> {
        {
            let mut tasks = lock(&self.inner.tasks);
            let index = position(&tasks, id)?;
            let entry = &tasks.entries[index];
            if !entry.record.state.is_finished() {
                return Err(invalid(entry, "remove"));
            }
            tasks.entries.remove(index);
        }
        self.inner.changed();
        Ok(())
    }

    /// Queues a failed, cancelled or interrupted task again, recreated from
    /// its spec by the factory registered for its kind. The old entry is
    /// removed and the new task's id is returned.
    ///
    /// # Errors
    ///
    /// - [`Error::UnknownTask`] if there is no such task.
    /// - [`Error::InvalidState`] if the task is not
    ///   [`Failed`](TaskState::Failed), [`Cancelled`](TaskState::Cancelled)
    ///   or [`Interrupted`](TaskState::Interrupted).
    /// - [`Error::NoFactory`] if no factory is registered for its kind.
    /// - [`Error::Recreate`] if the factory fails.
    pub fn retry(&self, id: TaskId) -> Result<TaskId> {
        let (kind, title, spec) = {
            let tasks = lock(&self.inner.tasks);
            let entry = &tasks.entries[position(&tasks, id)?];
            if !entry.record.state.can_retry() {
                return Err(invalid(entry, "retry"));
            }
            (
                entry.record.kind.clone(),
                entry.record.title.clone(),
                entry.record.spec.clone(),
            )
        };
        let factory = lock(&self.inner.factories)
            .get(&kind)
            .cloned()
            .ok_or(Error::NoFactory { kind })?;
        let task = factory(spec).map_err(|error| match error {
            TaskError::Failed { summary, hint } => Error::Recreate {
                title: title.clone(),
                summary,
                hint,
            },
            TaskError::Cancelled => Error::Recreate {
                title: title.clone(),
                summary: "cancelled".to_owned(),
                hint: None,
            },
        })?;
        {
            let mut tasks = lock(&self.inner.tasks);
            if let Ok(index) = position(&tasks, id) {
                tasks.entries.remove(index);
            }
        }
        let new_id = self.submit(task);
        tracing::info!(old_task_id = %id, task_id = %new_id, "task retried");
        Ok(new_id)
    }

    /// Snapshots of every task, in submission order.
    #[must_use]
    pub fn list(&self) -> Vec<TaskView> {
        lock(&self.inner.tasks)
            .entries
            .iter()
            .map(Entry::view)
            .collect()
    }

    /// A snapshot of one task, if it exists.
    #[must_use]
    pub fn get(&self, id: TaskId) -> Option<TaskView> {
        let tasks = lock(&self.inner.tasks);
        position(&tasks, id).ok().map(|i| tasks.entries[i].view())
    }

    /// Writes the state file now, waiting for the write to finish. Call it
    /// before exiting so the last changes are not lost.
    ///
    /// # Errors
    ///
    /// [`Error::WriteState`] if the file cannot be written.
    pub async fn flush(&self) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        let path = inner.state_file.clone();
        tokio::task::spawn_blocking(move || inner.save())
            .await
            .map_err(|error| Error::WriteState {
                path,
                source: std::io::Error::other(error.to_string()),
            })?
    }

    /// Applies `change` to the task `id` and reports the change.
    fn control(
        &self,
        id: TaskId,
        action: &'static str,
        change: impl FnOnce(&mut Entry) -> Result<()>,
    ) -> Result<()> {
        {
            let mut tasks = lock(&self.inner.tasks);
            let index = position(&tasks, id)?;
            change(&mut tasks.entries[index])?;
        }
        tracing::info!(task_id = %id, action, "task control");
        self.inner.changed();
        Ok(())
    }
}
