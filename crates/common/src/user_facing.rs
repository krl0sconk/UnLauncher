// SPDX-License-Identifier: GPL-3.0-or-later

/// An error or condition that is shown to the user.
///
/// Every error the interface or the CLI displays implements this trait, so the
/// user always gets a short description and, when possible, a next step.
pub trait UserFacing {
    /// Short description of what went wrong.
    fn summary(&self) -> String;
    /// What the user can do about it, if anything.
    fn hint(&self) -> Option<String>;
}
