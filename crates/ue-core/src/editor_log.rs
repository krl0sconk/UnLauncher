// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the editor writes its log, and why it closed when it crashed.

use std::path::{Path, PathBuf};

use crate::version::EngineVersion;

/// Bytes read from the end of a log: the cause of a crash is near the end,
/// and editor logs can grow to hundreds of MB.
const TAIL_BYTES: u64 = 1024 * 1024;

/// Why the editor crashed, as far as its log tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashCause {
    /// The kind of failure.
    pub kind: CrashKind,
    /// The log line that shows it, trimmed.
    pub line: String,
}

/// Kinds of editor crash that UnLauncher can explain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    /// The GPU stopped responding (Vulkan `VK_ERROR_DEVICE_LOST`). Often a
    /// driver timeout under heavy load, such as the benchmark the "Auto"
    /// scalability setting runs.
    GpuLost,
    /// The editor ran out of memory.
    OutOfMemory,
    /// Any other fatal error (an unhandled exception, a failed assertion).
    Fatal,
}

/// The log of a project's editor session: `<project dir>/Saved/Logs/<name>.log`.
#[must_use]
pub fn project_log_path(uproject: &Path) -> PathBuf {
    let dir = uproject.parent().unwrap_or_else(|| Path::new(""));
    let name = uproject
        .file_stem()
        .map_or_else(|| "Unreal".into(), |s| s.to_string_lossy().into_owned());
    dir.join("Saved/Logs").join(format!("{name}.log"))
}

/// The log of an editor started without a project:
/// `<unreal config dir>/<major>.<minor>/Saved/Logs/Unreal.log`, where the
/// config dir is `~/.config/Epic/UnrealEngine` (from `system::paths`).
#[must_use]
pub fn engine_log_path(unreal_config_dir: &Path, version: EngineVersion) -> PathBuf {
    unreal_config_dir
        .join(format!("{}.{}", version.major, version.minor))
        .join("Saved/Logs/Unreal.log")
}

/// Reads the end of `log` and diagnoses it with [`diagnose_crash`]. A missing
/// or unreadable log gives `None`.
#[must_use]
pub fn read_crash_cause(log: &Path) -> Option<CrashCause> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(log).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    diagnose_crash(&String::from_utf8_lossy(&bytes))
}

/// Finds the cause of a crash in the text of an editor log, or `None` if the
/// log shows no known failure (for example, a normal exit).
///
/// A lost GPU and running out of memory are recognized first, since they
/// come with a "Fatal error" too; otherwise the most specific fatal line is
/// reported (the exception or assertion before Unreal's generic "Fatal
/// error!").
#[must_use]
pub fn diagnose_crash(log: &str) -> Option<CrashCause> {
    let find = |patterns: &[&str]| {
        log.lines()
            .map(str::trim)
            .find(|line| patterns.iter().any(|p| line.contains(p)))
            .map(|line| line.chars().take(300).collect::<String>())
    };
    if let Some(line) = find(&["VK_ERROR_DEVICE_LOST", "TerminateOnGPUCrash", "GPU crashed"]) {
        return Some(CrashCause {
            kind: CrashKind::GpuLost,
            line,
        });
    }
    if let Some(line) = find(&["Ran out of memory", "Out of memory"]) {
        return Some(CrashCause {
            kind: CrashKind::OutOfMemory,
            line,
        });
    }
    find(&["Unhandled Exception", "Assertion failed", "Caught signal"])
        .or_else(|| find(&["Fatal error", "=== Critical error"]))
        .map(|line| CrashCause {
            kind: CrashKind::Fatal,
            line,
        })
}
