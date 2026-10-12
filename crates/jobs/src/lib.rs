// SPDX-License-Identifier: GPL-3.0-or-later

//! Long-running tasks with progress reporting and persistence.
//!
//! - [`Reporter`]: what a running task uses to report progress and to honour
//!   pause and cancel requests. Library crates (`ue-build`, `ue-prebuilt`,
//!   `fab`) take a `&Reporter` so they work the same from the GUI, the CLI and
//!   tests.
//! - [`TaskHandle`]: the other side, used to watch progress and send
//!   [`Control`] requests.
//! - [`Task`]: a unit of work the queue can run and recreate from its spec.
//! - [`TaskManager`]: the queue. Runs tasks one at a time
//!   ([`MAX_RUNNING_TASKS`]) in FIFO order, exposes [`TaskView`] snapshots,
//!   and persists the list so unfinished tasks show up as
//!   [`TaskState::Interrupted`] after a restart.

mod error;
mod manager;
mod reporter;
mod task;

pub use error::{Error, Result};
pub use manager::{MAX_FINISHED_TASKS, MAX_RUNNING_TASKS, TaskFactory, TaskManager};
pub use reporter::{Cancelled, Control, Progress, Reporter, TaskHandle, channel};
pub use task::{
    Task, TaskError, TaskFailure, TaskFuture, TaskId, TaskOutcome, TaskState, TaskView,
};
