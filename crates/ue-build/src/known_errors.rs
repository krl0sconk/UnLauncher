// SPDX-License-Identifier: GPL-3.0-or-later

//! The table of known build failures in `data/known_errors.toml`.

use std::sync::OnceLock;

use regex::{Regex, RegexSet};
use serde::{Deserialize, Serialize};

/// The versioned data file, embedded at compile time.
const KNOWN_ERRORS_TOML: &str = include_str!("../data/known_errors.toml");

/// A known problem recognised in the build output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnosis {
    /// Stable id of the table entry, e.g. `"out-of-memory"`.
    pub id: String,
    /// Probable cause, for the user.
    pub cause: String,
    /// What the user can do about it.
    pub solution: String,
    /// The first output line that matched (empty when inferred from the exit
    /// status).
    pub line: String,
}

/// One `[[error]]` entry of the data file.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct KnownErrorEntry {
    pub(crate) id: String,
    pub(crate) pattern: String,
    pub(crate) cause: String,
    pub(crate) solution: String,
}

#[derive(Debug, Deserialize)]
struct KnownErrorsFile {
    error: Vec<KnownErrorEntry>,
}

/// The compiled table.
#[derive(Debug)]
pub(crate) struct KnownErrors {
    entries: Vec<KnownErrorEntry>,
    set: RegexSet,
}

impl KnownErrors {
    /// Parses and compiles a table in the `known_errors.toml` format.
    pub(crate) fn parse(text: &str) -> std::result::Result<Self, String> {
        let file: KnownErrorsFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut ids = std::collections::HashSet::new();
        for entry in &file.error {
            if !ids.insert(entry.id.as_str()) {
                return Err(format!("duplicate id {:?}", entry.id));
            }
            Regex::new(&entry.pattern).map_err(|e| format!("entry {:?}: {e}", entry.id))?;
        }
        let set =
            RegexSet::new(file.error.iter().map(|e| &e.pattern)).map_err(|e| e.to_string())?;
        Ok(Self {
            entries: file.error,
            set,
        })
    }

    /// The embedded table, or `None` if it is invalid (a test guarantees it
    /// is not).
    pub(crate) fn embedded() -> Option<&'static Self> {
        static TABLE: OnceLock<Option<KnownErrors>> = OnceLock::new();
        TABLE
            .get_or_init(|| match Self::parse(KNOWN_ERRORS_TOML) {
                Ok(table) => Some(table),
                Err(error) => {
                    tracing::error!(%error, "the embedded known_errors.toml is invalid");
                    None
                }
            })
            .as_ref()
    }

    /// Indexes of the entries matching `line`.
    pub(crate) fn matches(&self, line: &str) -> Vec<usize> {
        self.set.matches(line).into_iter().collect()
    }

    /// Builds the diagnosis for entry `index` and the line that matched it.
    pub(crate) fn diagnosis(&self, index: usize, line: &str) -> Option<Diagnosis> {
        self.entries.get(index).map(|e| Diagnosis {
            id: e.id.clone(),
            cause: e.cause.clone(),
            solution: e.solution.clone(),
            line: line.to_owned(),
        })
    }

    /// The index of the entry with `id`.
    pub(crate) fn index_of(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Collects diagnoses line by line while a build streams its output.
///
/// Each entry is reported once, with the first line that matched; results are
/// ordered as in the data file (most likely root causes first).
#[derive(Debug, Default)]
pub struct Diagnoser {
    found: Vec<Option<Diagnosis>>,
}

impl Diagnoser {
    /// An empty diagnoser using the embedded table.
    #[must_use]
    pub fn new() -> Self {
        let len = KnownErrors::embedded().map_or(0, KnownErrors::len);
        Self {
            found: vec![None; len],
        }
    }

    /// Checks one output line.
    pub fn feed(&mut self, line: &str) {
        let Some(table) = KnownErrors::embedded() else {
            return;
        };
        for index in table.matches(line) {
            if let Some(slot @ None) = self.found.get_mut(index) {
                *slot = table.diagnosis(index, line.trim());
            }
        }
    }

    /// Adds the entry `id` (without a matching line) if it was not found yet.
    pub(crate) fn add_inferred(&mut self, id: &str) {
        let Some(table) = KnownErrors::embedded() else {
            return;
        };
        if let Some(index) = table.index_of(id)
            && let Some(slot @ None) = self.found.get_mut(index)
        {
            *slot = table.diagnosis(index, "");
        }
    }

    /// The diagnoses found so far, in table order.
    #[must_use]
    pub fn diagnoses(&self) -> Vec<Diagnosis> {
        self.found.iter().flatten().cloned().collect()
    }
}

/// Recognises known problems in build output lines.
///
/// Each known error is reported once, with the first line that matched, in
/// the order of `data/known_errors.toml`.
pub fn diagnose<I, S>(lines: I) -> Vec<Diagnosis>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut diagnoser = Diagnoser::new();
    for line in lines {
        diagnoser.feed(line.as_ref());
    }
    diagnoser.diagnoses()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_is_valid() {
        let table = KnownErrors::parse(KNOWN_ERRORS_TOML).expect("valid table");
        assert!(table.len() >= 8);
        for entry in &table.entries {
            assert!(
                !entry.cause.is_empty() && !entry.solution.is_empty(),
                "{}",
                entry.id
            );
        }
    }

    #[test]
    fn rejects_duplicate_ids() {
        let text = r#"
            version = 1
            [[error]]
            id = "a"
            pattern = "x"
            cause = "c"
            solution = "s"
            [[error]]
            id = "a"
            pattern = "y"
            cause = "c"
            solution = "s"
        "#;
        assert!(KnownErrors::parse(text).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn rejects_invalid_regex() {
        let text = r#"
            version = 1
            [[error]]
            id = "a"
            pattern = "("
            cause = "c"
            solution = "s"
        "#;
        assert!(KnownErrors::parse(text).is_err());
    }

    #[test]
    fn recognises_common_failures() {
        let cases = [
            (
                "x86_64-unknown-linux-gnu-g++: fatal error: Killed signal terminated program cc1plus",
                "out-of-memory",
            ),
            (
                "clang++: error: unable to execute command: Killed",
                "out-of-memory",
            ),
            ("error: write failed: No space left on device", "disk-full"),
            (
                "bash: ./Setup.sh: Permission denied",
                "script-permission-denied",
            ),
            (
                "Process terminated. Couldn't find a valid ICU package installed on the system.",
                "missing-icu",
            ),
            (
                "Failed to download 'http://cdn.unrealengine.com/x': timeout",
                "dependency-download",
            ),
            (
                "Engine/Source/Foo.cpp:12:3: error: use of undeclared identifier 'x'",
                "compile-error",
            ),
            ("Too many open files", "too-many-open-files"),
        ];
        for (line, id) in cases {
            let found = diagnose([line]);
            assert!(found.iter().any(|d| d.id == id), "{line:?} -> {found:?}");
        }
    }

    #[test]
    fn ordinary_lines_are_not_errors() {
        let lines = [
            "[12/3456] Compile [x64] Module.Core.cpp",
            "Running UnrealHeaderTool UnrealEditor",
            "Using clang toolchain at /UE/Engine/Extras/ThirdPartyNotUE/SDKs",
            "Result: Succeeded",
        ];
        assert_eq!(diagnose(lines), Vec::new());
    }

    #[test]
    fn reports_each_entry_once_with_first_line() {
        let found = diagnose(["No space left on device (1)", "No space left on device (2)"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, "No space left on device (1)");
    }
}
