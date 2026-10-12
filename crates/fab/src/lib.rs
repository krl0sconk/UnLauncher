// SPDX-License-Identifier: GPL-3.0-or-later

//! Fab marketplace access through Epic's unofficial API, isolated behind
//! [`AssetSource`].
//!
//! - [`AssetSource::list`]: the user's library as [`LibraryAsset`]s;
//!   [`compatible_version`] picks the build for an engine.
//! - [`AssetSource::download`]: resumable, verified chunk download into a
//!   cache folder, giving a [`DownloadedAsset`].
//! - [`install`]: copies a download into a project or an engine.
//!
//! [`EpicAssetSource`] uses `egs-api` for the Epic calls and manifest parsing.

mod asset;
mod download;
mod error;
mod install;
mod source;

pub use asset::{AssetKind, AssetVersion, LibraryAsset, closest_version, compatible_version};
pub use download::DownloadedAsset;
pub use error::{Error, Result};
pub use install::{InstallTarget, install};
pub use source::{AssetSource, BoxFuture, EpicAssetSource};
