// SPDX-License-Identifier: GPL-3.0-or-later

/// Whether a function may replace a file that already exists.
///
/// Functions that write user files take this parameter so that overwriting is
/// always an explicit decision. The interface and the CLI ask the user before
/// passing [`Overwrite::Yes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Fail if the file already exists.
    No,
    /// Replace the file if it exists (after backing it up where the caller says so).
    Yes,
}
