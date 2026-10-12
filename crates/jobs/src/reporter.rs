// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

/// How often blocking code re-checks a paused task.
const PAUSE_POLL: Duration = Duration::from_millis(200);

/// A snapshot of a task's progress.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Current stage, e.g. "Cloning", "Setup.sh", "Compiling".
    pub stage: String,
    /// Completion of the current stage in `0.0..=1.0`, if known.
    pub fraction: Option<f32>,
    /// Latest detail line, e.g. the file being compiled.
    pub message: Option<String>,
}

/// What the owner of a task wants it to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    /// Keep running.
    Run,
    /// Stop at the next checkpoint until resumed.
    Pause,
    /// Stop as soon as possible.
    Cancel,
}

/// Returned by checkpoints when the task was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// The task side: reports progress and checks for pause/cancel.
///
/// Cheap to clone; clones report to the same task.
#[derive(Debug, Clone)]
pub struct Reporter {
    progress: Arc<watch::Sender<Progress>>,
    control: watch::Receiver<Control>,
}

/// The owner side: reads progress and sends control requests.
#[derive(Debug, Clone)]
pub struct TaskHandle {
    progress: watch::Receiver<Progress>,
    control: Arc<watch::Sender<Control>>,
}

/// Creates a connected [`Reporter`] and [`TaskHandle`].
#[must_use]
pub fn channel() -> (Reporter, TaskHandle) {
    let (progress_tx, progress_rx) = watch::channel(Progress::default());
    let (control_tx, control_rx) = watch::channel(Control::Run);
    (
        Reporter {
            progress: Arc::new(progress_tx),
            control: control_rx,
        },
        TaskHandle {
            progress: progress_rx,
            control: Arc::new(control_tx),
        },
    )
}

impl Reporter {
    /// A reporter nobody listens to and that is never paused or cancelled.
    #[must_use]
    pub fn detached() -> Self {
        let (reporter, handle) = channel();
        // Keep the control sender alive so the channel never closes.
        std::mem::forget(handle.control);
        reporter
    }

    /// Starts a new stage, clearing fraction and message.
    pub fn stage(&self, stage: impl Into<String>) {
        let stage = stage.into();
        tracing::info!(%stage, "task stage");
        self.progress.send_replace(Progress {
            stage,
            fraction: None,
            message: None,
        });
    }

    /// Updates the fraction (clamped to `0.0..=1.0`) of the current stage.
    pub fn fraction(&self, fraction: Option<f32>) {
        self.progress.send_modify(|p| {
            p.fraction = fraction.map(|f| f.clamp(0.0, 1.0));
        });
    }

    /// Updates the detail message of the current stage.
    pub fn message(&self, message: impl Into<String>) {
        let message = message.into();
        self.progress.send_modify(|p| p.message = Some(message));
    }

    /// The current progress.
    #[must_use]
    pub fn current(&self) -> Progress {
        self.progress.borrow().clone()
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.control.borrow() == Control::Cancel
    }

    /// Async checkpoint: waits while paused.
    ///
    /// # Errors
    ///
    /// [`Cancelled`] if the task was cancelled (or its handle dropped while
    /// paused).
    pub async fn checkpoint(&self) -> Result<(), Cancelled> {
        let mut control = self.control.clone();
        loop {
            let state = *control.borrow_and_update();
            match state {
                Control::Run => return Ok(()),
                Control::Cancel => return Err(Cancelled),
                Control::Pause => {
                    if control.changed().await.is_err() {
                        return Err(Cancelled);
                    }
                }
            }
        }
    }

    /// Blocking checkpoint for code running on a blocking thread (for example
    /// inside `spawn_blocking`): waits while paused.
    ///
    /// # Errors
    ///
    /// [`Cancelled`] if the task was cancelled.
    pub fn checkpoint_blocking(&self) -> Result<(), Cancelled> {
        loop {
            let state = *self.control.borrow();
            match state {
                Control::Run => return Ok(()),
                Control::Cancel => return Err(Cancelled),
                Control::Pause => {
                    if self.control.has_changed().is_err() {
                        return Err(Cancelled);
                    }
                    std::thread::sleep(PAUSE_POLL);
                }
            }
        }
    }
}

impl TaskHandle {
    /// The latest progress.
    #[must_use]
    pub fn progress(&self) -> Progress {
        self.progress.borrow().clone()
    }

    /// A receiver to await progress changes.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<Progress> {
        self.progress.clone()
    }

    /// The last control request sent.
    #[must_use]
    pub fn control(&self) -> Control {
        *self.control.borrow()
    }

    /// Sends a control request. A cancelled task stays cancelled.
    pub fn send(&self, control: Control) {
        self.control.send_if_modified(|current| {
            if *current == Control::Cancel || *current == control {
                false
            } else {
                *current = control;
                true
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn progress_reaches_the_handle() {
        let (reporter, handle) = channel();
        reporter.stage("Unzipping");
        reporter.fraction(Some(1.5));
        reporter.message("Engine/Binaries");
        assert_eq!(
            handle.progress(),
            Progress {
                stage: "Unzipping".into(),
                fraction: Some(1.0),
                message: Some("Engine/Binaries".into()),
            }
        );
    }

    #[test]
    fn cancel_is_final() {
        let (reporter, handle) = channel();
        handle.send(Control::Cancel);
        handle.send(Control::Run);
        assert!(reporter.is_cancelled());
        assert_eq!(reporter.checkpoint_blocking(), Err(Cancelled));
    }

    #[tokio::test]
    async fn checkpoint_waits_while_paused() {
        let (reporter, handle) = channel();
        handle.send(Control::Pause);
        let waiter = tokio::spawn(async move { reporter.checkpoint().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());
        handle.send(Control::Run);
        assert_eq!(waiter.await.unwrap(), Ok(()));
    }

    #[test]
    fn blocking_checkpoint_resumes() {
        let (reporter, handle) = channel();
        handle.send(Control::Pause);
        let waiter = std::thread::spawn(move || reporter.checkpoint_blocking());
        std::thread::sleep(Duration::from_millis(50));
        handle.send(Control::Run);
        assert_eq!(waiter.join().unwrap(), Ok(()));
    }

    #[test]
    fn detached_reporter_runs() {
        assert_eq!(Reporter::detached().checkpoint_blocking(), Ok(()));
    }
}
