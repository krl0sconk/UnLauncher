// SPDX-License-Identifier: GPL-3.0-or-later

/// Explicit permission to delete files.
///
/// Functions that delete user files take this parameter so deleting is always
/// a decision the user made. The interface and the CLI ask before passing
/// [`Delete::Yes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delete {
    /// Do not delete anything; fail instead.
    No,
    /// The user confirmed the deletion.
    Yes,
}
