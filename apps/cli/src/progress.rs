// SPDX-License-Identifier: GPL-3.0-or-later

//! Running a long task with a progress line and Ctrl+C cancellation.

use std::future::Future;
use std::io::{IsTerminal, Write};
use std::time::Duration;

use jobs::{Control, Reporter, TaskHandle};

const REFRESH: Duration = Duration::from_millis(250);

/// Runs `work` with a fresh [`Reporter`], printing its progress on stderr.
/// Ctrl+C asks the task to cancel and waits for it to clean up.
pub fn run<T, F, Fut>(work: F) -> anyhow::Result<T>
where
    F: FnOnce(Reporter) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let runtime = tokio::runtime::Runtime::new()?;
    let (reporter, handle) = jobs::channel();
    let task = runtime.spawn(work(reporter));
    let result = runtime.block_on(watch(task, &handle))?;
    eprintln!();
    Ok(result)
}

async fn watch<T>(mut task: tokio::task::JoinHandle<T>, handle: &TaskHandle) -> anyhow::Result<T> {
    let mut ticker = tokio::time::interval(REFRESH);
    let mut cancel_sent = false;
    loop {
        tokio::select! {
            result = &mut task => return Ok(result?),
            _ = tokio::signal::ctrl_c(), if !cancel_sent => {
                cancel_sent = true;
                eprintln!("\ncancelling, cleaning up…");
                handle.send(Control::Cancel);
            }
            _ = ticker.tick() => print_progress(handle),
        }
    }
}

fn print_progress(handle: &TaskHandle) {
    let progress = handle.progress();
    if progress.stage.is_empty() {
        return;
    }
    let percent = progress
        .fraction
        .map_or_else(String::new, |f| format!(" {:5.1}%", f * 100.0));
    let message = progress.message.unwrap_or_default();
    let mut stderr = std::io::stderr();
    if stderr.is_terminal() {
        let line: String = format!("{}{percent}  {message}", progress.stage)
            .chars()
            .take(110)
            .collect();
        let _ = write!(stderr, "\r\x1b[2K{line}");
        let _ = stderr.flush();
    }
}
