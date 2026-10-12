// SPDX-License-Identifier: GPL-3.0-or-later

//! Running one build step: streaming output, log file, progress and cancel.

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use jobs::Reporter;
use system::{HostCommand, Sandbox};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::known_errors::Diagnoser;
use crate::parser::parse_progress;

/// How often the cancel flag is checked while a step runs.
const CANCEL_POLL: Duration = Duration::from_millis(200);
/// Time between SIGTERM and SIGKILL when cancelling.
const KILL_GRACE: Duration = Duration::from_secs(10);
/// Exit code of a process killed by SIGKILL, as reported by a shell.
const SHELL_SIGKILL_EXIT: i32 = 128 + 9;

/// The full build log.
pub(crate) struct BuildLog {
    path: std::path::PathBuf,
    writer: BufWriter<tokio::fs::File>,
}

impl BuildLog {
    pub(crate) async fn create(path: &Path) -> Result<Self> {
        let write_err = |source| Error::WriteFile {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await.map_err(write_err)?;
        }
        let file = tokio::fs::File::create(path).await.map_err(write_err)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
        })
    }

    pub(crate) async fn line(&mut self, line: &str) -> Result<()> {
        let result = async {
            self.writer.write_all(line.as_bytes()).await?;
            self.writer.write_all(b"\n").await
        }
        .await;
        result.map_err(|source| Error::WriteFile {
            path: self.path.clone(),
            source,
        })
    }

    pub(crate) async fn flush(&mut self) -> Result<()> {
        self.writer
            .flush()
            .await
            .map_err(|source| Error::WriteFile {
                path: self.path.clone(),
                source,
            })
    }
}

/// How a step ended (when not cancelled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StepExit {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    /// The kernel (most likely the OOM killer) sent SIGKILL.
    pub(crate) killed: bool,
}

/// Sends `signal` to the step's process group.
fn signal_group(pid: Option<u32>, signal: rustix::process::Signal) {
    let pid = pid
        .and_then(|p| i32::try_from(p).ok())
        .and_then(rustix::process::Pid::from_raw);
    if let Some(pid) = pid
        && let Err(error) = rustix::process::kill_process_group(pid, signal)
    {
        tracing::debug!(%error, "failed to signal the build process group");
    }
}

/// Splits a byte stream into lines on `\n` or `\r` (progress meters redraw
/// with `\r`) and sends them to `tx`.
async fn read_lines(mut reader: impl AsyncRead + Unpin, tx: mpsc::Sender<String>) {
    let mut pending = Vec::new();
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        let n = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &byte in &buffer[..n] {
            if byte == b'\n' || byte == b'\r' {
                if !pending.is_empty() {
                    let line = String::from_utf8_lossy(&pending).into_owned();
                    pending.clear();
                    if tx.send(line).await.is_err() {
                        return;
                    }
                }
            } else {
                pending.push(byte);
            }
        }
    }
    if !pending.is_empty() {
        let _ = tx
            .send(String::from_utf8_lossy(&pending).into_owned())
            .await;
    }
}

/// Runs `command`, streaming stdout and stderr line by line into the log,
/// the progress parser and the diagnoser. Kills the whole process group if
/// `reporter` is cancelled.
pub(crate) async fn run_step(
    command: &HostCommand,
    program: &str,
    sandbox: Sandbox,
    reporter: &Reporter,
    log: &mut BuildLog,
    diagnoser: &mut Diagnoser,
) -> Result<StepExit> {
    let mut std_command = command.to_command(sandbox);
    // Own process group, so cancel reaches the compilers the script starts.
    std_command.process_group(0);
    let mut child = tokio::process::Command::from(std_command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| Error::Spawn {
            program: program.to_owned(),
            source,
        })?;
    let pid = child.id();
    let (tx, mut rx) = mpsc::channel::<String>(1024);
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(read_lines(stdout, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(read_lines(stderr, tx.clone()));
    }
    drop(tx);

    let mut ticker = tokio::time::interval(CANCEL_POLL);
    let mut kill_deadline: Option<Instant> = None;
    loop {
        tokio::select! {
            line = rx.recv() => {
                let Some(line) = line else { break };
                log.line(&line).await?;
                diagnoser.feed(&line);
                if let Some(progress) = parse_progress(&line) {
                    reporter.fraction(Some(progress.fraction));
                    reporter.message(progress.message);
                } else if !line.trim().is_empty() {
                    reporter.message(line.trim());
                }
            }
            _ = ticker.tick() => {
                if reporter.is_cancelled() {
                    match kill_deadline {
                        None => {
                            tracing::info!(%program, "cancelling build step");
                            signal_group(pid, rustix::process::Signal::TERM);
                            kill_deadline = Some(Instant::now() + KILL_GRACE);
                        }
                        Some(deadline) if Instant::now() >= deadline => {
                            signal_group(pid, rustix::process::Signal::KILL);
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    let status = child.wait().await.map_err(|source| Error::Spawn {
        program: program.to_owned(),
        source,
    })?;
    if kill_deadline.is_some() || reporter.is_cancelled() {
        // Make sure nothing of the group survives.
        signal_group(pid, rustix::process::Signal::KILL);
        log.line("==> cancelled").await?;
        return Err(Error::Cancelled);
    }
    let killed = status.signal() == Some(9) || status.code() == Some(SHELL_SIGKILL_EXIT);
    Ok(StepExit {
        success: status.success(),
        code: status.code(),
        killed,
    })
}
