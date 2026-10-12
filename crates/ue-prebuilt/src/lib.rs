// SPDX-License-Identifier: GPL-3.0-or-later

//! Importing and downloading prebuilt Unreal Engine releases.
//!
//! - [`import_zip`]: extracts a Linux engine zip the user already downloaded,
//!   with progress, pause and cancel through a [`jobs::Reporter`].
//! - [`register_imported`]: registers the result in `Install.ini` and records
//!   it as [`ue_core::EngineKind::Prebuilt`].
//! - [`engines_from_links`]: the Linux releases among the links of Epic's
//!   download page, read by the built-in web window.
//! - [`download_file`]: resumable download of one of them, to import it next.
//! - [`fab_versions_from_json`], [`newer_than_installed`]: whether a newer
//!   engine is out, from Fab's public list of versions.
//! - [`DownloadsWatcher`], [`watch_for_engine_zip`]: notice an engine zip
//!   downloaded in the system browser, to offer installing it.

mod download;
mod error;
mod import;
mod space;
mod updates;
mod watch;

pub use download::{
    BLOBS_PATH, DOWNLOADING_STAGE, EngineDownload, download_engine, download_file,
    engines_from_blobs_json, engines_from_links,
};

pub use error::{Error, Result};
pub use import::{EXTRACTING_STAGE, ImportedEngine, import_zip, register_imported};
pub use updates::{FAB_HOME_URL, FAB_VERSIONS_PATH, fab_versions_from_json, newer_than_installed};
pub use watch::{DownloadsWatcher, FoundEngineZip, watch_for_engine_zip};
