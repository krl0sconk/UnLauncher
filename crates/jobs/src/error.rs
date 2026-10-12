// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::path::PathBuf;

use common::UserFacing;

use crate::task::{TaskId, TaskState};

/// Errors returned by `jobs`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// [`TaskManager::new`](crate::TaskManager::new) was called outside a
    /// tokio runtime.
    #[error("the task manager must be created inside a tokio runtime")]
    NoRuntime,

    /// No task has this id (it may have been removed).
    #[error("task {id} does not exist")]
    UnknownTask {
        /// The id that was asked for.
        id: TaskId,
    },

    /// The action does not apply to the task's current state.
    #[error("cannot {action} task {id} while it is {state:?}")]
    InvalidState {
        /// The task.
        id: TaskId,
        /// Its current state.
        state: TaskState,
        /// What was attempted: "pause", "resume", "cancel", "remove", "retry".
        action: &'static str,
    },

    /// The task does not support pausing.
    #[error("task {id} cannot be paused")]
    NotPausable {
        /// The task.
        id: TaskId,
    },

    /// No factory is registered to recreate tasks of this kind.
    #[error("tasks of kind \"{kind}\" cannot be retried")]
    NoFactory {
        /// The task kind.
        kind: String,
    },

    /// The factory could not recreate the task from its stored spec.
    #[error("the task \"{title}\" could not be recreated: {summary}")]
    Recreate {
        /// The task's title.
        title: String,
        /// What the factory reported.
        summary: String,
        /// The factory's hint, if any.
        hint: Option<String>,
    },

    /// The state file could not be written.
    #[error("failed to save the task list to {}", path.display())]
    WriteState {
        /// The state file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Result type of `jobs`.
pub type Result<T> = std::result::Result<T, Error>;

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::NoRuntime | Self::NoFactory { .. } => {
                "This is a bug in UnLauncher; please report it."
            }
            Self::UnknownTask { .. } | Self::InvalidState { .. } | Self::NotPausable { .. } => {
                return None;
            }
            Self::Recreate { hint, .. } => {
                return Some(hint.clone().unwrap_or_else(|| {
                    "Start the task again from the place where you first started it.".to_owned()
                }));
            }
            Self::WriteState { .. } => {
                "Check that UnLauncher's data folder is writable and that the disk is not full."
            }
        };
        Some(hint.to_owned())
    }
}
