// SPDX-License-Identifier: GPL-3.0-or-later

//! The state file: the task list as JSON, without live handles.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::reporter::Progress;
use crate::task::{TaskFailure, TaskId, TaskState, TaskView};

/// Version of the state file format.
const FORMAT_VERSION: u32 = 1;

/// A task as stored on disk (and kept in memory next to its live handle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskRecord {
    pub id: TaskId,
    pub kind: String,
    pub title: String,
    pub spec: serde_json::Value,
    pub pausable: bool,
    pub state: TaskState,
    pub progress: Progress,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub outcome: Option<String>,
    pub error: Option<TaskFailure>,
}

impl TaskRecord {
    /// The view of this record, with `progress` and `is_cancelling` given by
    /// the caller (they may come from the live handle).
    pub fn view(&self, progress: Progress, is_cancelling: bool) -> TaskView {
        TaskView {
            id: self.id,
            kind: self.kind.clone(),
            title: self.title.clone(),
            state: self.state,
            progress,
            pausable: self.pausable,
            is_cancelling,
            created_at: self.created_at,
            finished_at: self.finished_at,
            outcome: self.outcome.clone(),
            error: self.error.clone(),
        }
    }
}

/// The whole state file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct StateFile {
    pub version: u32,
    pub next_id: u64,
    pub tasks: Vec<TaskRecord>,
}

/// Loads the state file at `path`.
///
/// A missing file gives an empty list. A file that cannot be parsed is
/// renamed to `<path>.corrupt` and an empty list is returned. Tasks that had
/// not finished are marked [`TaskState::Interrupted`]. Never fails: problems
/// are logged.
pub(crate) fn load(path: &Path, finished_at: u64) -> StateFile {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return StateFile::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot read the task list; starting empty");
            return StateFile::default();
        }
    };
    let mut state = match serde_json::from_slice::<StateFile>(&bytes) {
        Ok(state) if state.version == FORMAT_VERSION => state,
        Ok(state) => {
            backup_corrupt(path, &format!("unknown format version {}", state.version));
            return StateFile::default();
        }
        Err(error) => {
            backup_corrupt(path, &error.to_string());
            return StateFile::default();
        }
    };
    let max_id = state.tasks.iter().map(|t| t.id.0).max().unwrap_or(0);
    state.next_id = state.next_id.max(max_id.saturating_add(1));
    for task in &mut state.tasks {
        if !task.state.is_finished() {
            tracing::info!(task_id = %task.id, title = %task.title, "task was interrupted");
            task.state = TaskState::Interrupted;
            task.finished_at = Some(finished_at);
        }
    }
    state
}

/// Renames an unreadable state file to `<path>.corrupt`, logging why.
fn backup_corrupt(path: &Path, reason: &str) {
    let backup = sibling(path, ".corrupt");
    match fs::rename(path, &backup) {
        Ok(()) => tracing::warn!(
            path = %path.display(),
            backup = %backup.display(),
            reason,
            "the task list is corrupt; it was backed up and a new one started"
        ),
        Err(error) => tracing::warn!(
            path = %path.display(),
            reason,
            %error,
            "the task list is corrupt and could not be backed up; starting empty"
        ),
    }
}

/// Writes `state` to `path` atomically (temporary file + rename), creating
/// the parent folders.
pub(crate) fn save(path: &Path, state: &StateFile) -> Result<()> {
    let write_error = |source| Error::WriteState {
        path: path.to_path_buf(),
        source,
    };
    let json = serde_json::to_vec_pretty(state).map_err(|e| write_error(io::Error::other(e)))?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(write_error)?;
    }
    let tmp = sibling(path, &format!(".tmp-{}", std::process::id()));
    let result = fs::write(&tmp, json).and_then(|()| fs::rename(&tmp, path));
    if let Err(source) = result {
        let _ = fs::remove_file(&tmp);
        return Err(write_error(source));
    }
    Ok(())
}

/// `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("tasks"), ToOwned::to_owned);
    name.push(suffix);
    path.with_file_name(name)
}

/// Builds the file contents from records.
pub(crate) fn snapshot(next_id: u64, tasks: Vec<TaskRecord>) -> StateFile {
    StateFile {
        version: FORMAT_VERSION,
        next_id,
        tasks,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn record(id: u64, state: TaskState) -> TaskRecord {
        TaskRecord {
            id: TaskId(id),
            kind: "fake".into(),
            title: format!("task {id}"),
            spec: serde_json::json!({ "n": id }),
            pausable: true,
            state,
            progress: Progress::default(),
            created_at: 1,
            finished_at: None,
            outcome: None,
            error: None,
        }
    }

    #[test]
    fn round_trips_and_marks_unfinished_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/tasks.json");
        let tasks = vec![
            record(1, TaskState::Done),
            record(2, TaskState::Running),
            record(3, TaskState::Queued),
        ];
        save(&path, &snapshot(2, tasks)).unwrap();
        let loaded = load(&path, 99);
        let states: Vec<_> = loaded.tasks.iter().map(|t| t.state).collect();
        assert_eq!(
            states,
            [
                TaskState::Done,
                TaskState::Interrupted,
                TaskState::Interrupted
            ]
        );
        assert_eq!(loaded.tasks[1].finished_at, Some(99));
        // next_id never reuses a stored id.
        assert_eq!(loaded.next_id, 4);
    }

    #[test]
    fn unknown_version_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        fs::write(&path, r#"{"version": 999, "next_id": 1, "tasks": []}"#).unwrap();
        assert!(load(&path, 0).tasks.is_empty());
        assert!(dir.path().join("tasks.json.corrupt").is_file());
        assert!(!path.exists());
    }
}
