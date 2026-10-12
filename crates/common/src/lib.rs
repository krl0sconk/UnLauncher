// SPDX-License-Identifier: GPL-3.0-or-later

//! Types shared by every UnLauncher crate.
//!
//! - [`UserFacing`]: errors that are shown to the user with a hint.
//! - [`Secret`]: a token or credential that never shows up in logs.
//! - [`Overwrite`]: explicit permission to replace an existing file.
//! - [`Delete`]: explicit permission to delete files.

mod delete;
mod overwrite;
mod secret;
mod user_facing;

pub use delete::Delete;
pub use overwrite::Overwrite;
pub use secret::Secret;
pub use user_facing::UserFacing;
