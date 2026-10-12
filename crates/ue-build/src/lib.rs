// SPDX-License-Identifier: GPL-3.0-or-later

//! Pre-flight checks and building Unreal Engine from source.
//!
//! - [`preflight`]: disk, RAM, GitHub access and host tools, as
//!   [`CheckResult`]s.
//! - [`clone_shallow`], [`update`]: the engine source with git (depth 1,
//!   authenticated with a GitHub token).
//! - [`run_build`]: `Setup.sh` → `GenerateProjectFiles.sh` → `Build.sh`, with
//!   streamed progress, a full log and [`Diagnosis`] of known failures.
//! - [`parallel_jobs`]: how many compile jobs fit in RAM and cores.
//! - [`parse_progress`], [`diagnose`]: the output parser and the known-errors
//!   table (`data/known_errors.toml`).
//!
//! Long operations take a [`jobs::Reporter`] for progress and cancel. All
//! processes go through [`system::HostCommand`].

mod error;
mod install;
mod known_errors;
mod meminfo;
mod parallel;
mod parser;
mod pipeline;
mod preflight;
mod source;

pub use error::{Error, Result};
pub use install::{SourceInstall, install_from_source, register_built, update_from_source};
pub use known_errors::{Diagnoser, Diagnosis, diagnose};
pub use meminfo::{MemInfo, PROC_MEMINFO};
pub use parallel::{RAM_PER_JOB_BYTES, detect_parallel_jobs, parallel_jobs};
pub use parser::{LineProgress, parse_progress};
pub use pipeline::{
    BUILD_SCRIPT, BuildOptions, BuildReport, DEFAULT_TARGETS, GENERATE_PROJECT_FILES_SCRIPT,
    SETUP_SCRIPT, run_build,
};
pub use preflight::{
    CheckResult, CheckStatus, DISK_FAIL_BYTES, DISK_WARN_BYTES, GITHUB_API_BASE, PreflightInput,
    RAM_WARN_BYTES, UE_REPO, disk_status, nearest_existing_ancestor, overall_status, preflight,
    ram_status,
};
pub use source::{UE_REMOTE, UpdateOutcome, clone_shallow, update};
