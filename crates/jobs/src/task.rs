// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use common::UserFacing;
use serde::{Deserialize, Serialize};

use crate::reporter::{Cancelled, Progress, Reporter};

/// The future returned by [`Task::run`].
pub type TaskFuture = Pin<Box<dyn Future<Output = Result<TaskOutcome, TaskError>> + Send>>;

/// A unit of long-running work the [`TaskManager`](crate::TaskManager) can
/// queue, run, pause, cancel and recreate after a restart.
pub trait Task: Send + 'static {
    /// Stable identifier of the kind of task, e.g. `"ue-build.compile"`. Used
    /// to find the factory that recreates the task from its
    /// [`spec`](Task::spec) (see
    /// [`TaskManager::register_factory`](crate::TaskManager::register_factory)).
    fn kind(&self) -> &'static str;

    /// Title shown to the user, e.g. "Build Unreal Engine 5.6".
    fn title(&self) -> String;

    /// Everything needed to recreate the task (paths, versions, options).
    /// Stored in the state file, so it must never contain secrets.
    fn spec(&self) -> serde_json::Value;

    /// Whether the task honours [`Control::Pause`](crate::Control::Pause) at
    /// its checkpoints. Only pausable tasks can be paused.
    fn pausable(&self) -> bool;

    /// Runs the task. Report progress and call
    /// [`Reporter::checkpoint`] regularly so pause and cancel work.
    fn run(self: Box<Self>, reporter: Reporter) -> TaskFuture;
}

/// What a task produced when it finished successfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOutcome {
    /// User-facing success message, e.g. "Unreal Engine 5.6 is ready".
    pub message: String,
}

impl TaskOutcome {
    /// An outcome with the given user-facing message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Why a task did not finish successfully.
///
/// Any error implementing [`UserFacing`] converts into
/// [`TaskError::Failed`] with `?`. [`Cancelled`] converts into
/// [`TaskError::Cancelled`]; if your own error type wraps a cancellation,
/// map that variant to [`TaskError::Cancelled`] explicitly, or the task will
/// show up as failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskError {
    /// The task stopped because it was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The task failed.
    #[error("{summary}")]
    Failed {
        /// Short description of what went wrong.
        summary: String,
        /// What the user can do about it, if anything.
        hint: Option<String>,
    },
}

impl TaskError {
    /// A failure with a summary and an optional hint.
    #[must_use]
    pub fn failed(summary: impl Into<String>, hint: Option<String>) -> Self {
        Self::Failed {
            summary: summary.into(),
            hint,
        }
    }

    /// A failure described by a [`UserFacing`] error.
    #[must_use]
    pub fn from_user_facing<E: UserFacing + ?Sized>(error: &E) -> Self {
        Self::Failed {
            summary: error.summary(),
            hint: error.hint(),
        }
    }
}

impl From<Cancelled> for TaskError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

impl<E: UserFacing> From<E> for TaskError {
    fn from(error: E) -> Self {
        Self::from_user_facing(&error)
    }
}

/// Identifier of a task in a [`TaskManager`](crate::TaskManager). Unique
/// within one state file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Lifecycle of a task:
/// `Queued → Running ⇄ Paused → Done | Failed | Cancelled`, plus
/// `Interrupted` for tasks that had not finished when UnLauncher last exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    /// Waiting for its turn.
    Queued,
    /// Running.
    Running,
    /// Paused at a checkpoint; still holds its slot in the queue.
    Paused,
    /// Finished successfully.
    Done,
    /// Finished with an error.
    Failed,
    /// Cancelled by the user.
    Cancelled,
    /// Was queued, running or paused when UnLauncher last exited.
    Interrupted,
}

impl TaskState {
    /// Whether the task will not run any more (it can be removed, and
    /// retried unless it is [`TaskState::Done`]).
    #[must_use]
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Whether [`TaskManager::retry`](crate::TaskManager::retry) accepts a
    /// task in this state.
    #[must_use]
    pub fn can_retry(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::Interrupted)
    }
}

/// A failure as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFailure {
    /// Short description of what went wrong.
    pub summary: String,
    /// What the user can do about it, if anything.
    pub hint: Option<String>,
}

/// A snapshot of a task, for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskView {
    /// The task's id.
    pub id: TaskId,
    /// The task's [`kind`](Task::kind).
    pub kind: String,
    /// The task's [`title`](Task::title).
    pub title: String,
    /// Current state.
    pub state: TaskState,
    /// Latest progress (live while running, last known otherwise).
    pub progress: Progress,
    /// Whether the task can be paused.
    pub pausable: bool,
    /// Whether a cancel was requested and the task has not stopped yet.
    pub is_cancelling: bool,
    /// When the task was submitted, in seconds since the Unix epoch.
    pub created_at: u64,
    /// When the task finished, in seconds since the Unix epoch.
    pub finished_at: Option<u64>,
    /// Success message, for [`TaskState::Done`].
    pub outcome: Option<String>,
    /// The error, for [`TaskState::Failed`].
    pub error: Option<TaskFailure>,
}
