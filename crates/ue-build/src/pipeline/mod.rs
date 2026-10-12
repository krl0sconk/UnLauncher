// SPDX-License-Identifier: GPL-3.0-or-later

//! The build pipeline: `Setup.sh` → `GenerateProjectFiles.sh` → `Build.sh`.

mod process;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jobs::Reporter;
use serde::{Deserialize, Serialize};
use system::{HostCommand, Sandbox};

use crate::error::{Error, Result};
use crate::known_errors::Diagnoser;
use crate::parallel::detect_parallel_jobs;
use process::{BuildLog, run_step};

/// `Setup.sh`, relative to the engine root.
pub const SETUP_SCRIPT: &str = "Setup.sh";
/// `GenerateProjectFiles.sh`, relative to the engine root.
pub const GENERATE_PROJECT_FILES_SCRIPT: &str = "GenerateProjectFiles.sh";
/// `UnrealBuildTool`'s Linux entry point, relative to the engine root.
pub const BUILD_SCRIPT: &str = "Engine/Build/BatchFiles/Linux/Build.sh";

/// Targets built by default, in order. `ShaderCompileWorker` is built
/// explicitly because the editor needs it to compile shaders and building
/// `UnrealEditor` alone does not always build it (the engine's `Makefile`
/// lists both in its standard set); building it again is a cheap no-op.
pub const DEFAULT_TARGETS: [&str; 2] = ["ShaderCompileWorker", "UnrealEditor"];

/// What [`run_build`] builds and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOptions {
    /// The engine source root (the folder with `Setup.sh`).
    pub engine_root: PathBuf,
    /// Parallel compile jobs (`-MaxParallelActions`). `None` uses
    /// [`crate::parallel_jobs`] for this machine.
    pub jobs: Option<usize>,
    /// Where the full log of every step is written (overwritten).
    pub log_path: PathBuf,
    /// `UnrealBuildTool` targets to build, in order.
    pub targets: Vec<String>,
    /// Build configuration, normally `"Development"`.
    pub configuration: String,
    /// The kernel's memory report used when `jobs` is `None`, normally
    /// [`crate::PROC_MEMINFO`].
    pub meminfo_path: PathBuf,
}

impl BuildOptions {
    /// Default options: [`DEFAULT_TARGETS`], `Development`, automatic jobs.
    #[must_use]
    pub fn new(engine_root: impl Into<PathBuf>, log_path: impl Into<PathBuf>) -> Self {
        Self {
            engine_root: engine_root.into(),
            jobs: None,
            log_path: log_path.into(),
            targets: DEFAULT_TARGETS.iter().map(|&t| t.to_owned()).collect(),
            configuration: "Development".to_owned(),
            meminfo_path: PathBuf::from(crate::PROC_MEMINFO),
        }
    }
}

/// The result of a successful build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildReport {
    /// The full build log.
    pub log_path: PathBuf,
    /// Parallel jobs used.
    pub jobs: usize,
    /// Wall-clock time of the whole pipeline.
    pub duration: Duration,
    /// The editor executable, if it is where `ue-core` expects it.
    pub editor: Option<PathBuf>,
}

/// One step of the pipeline.
struct Step {
    stage: String,
    script: PathBuf,
    args: Vec<String>,
}

fn steps(opts: &BuildOptions, jobs: usize) -> Vec<Step> {
    let root = &opts.engine_root;
    // Setup.sh runs without arguments and with stdin at /dev/null. It calls
    // GitDependencies with `--prompt`; with no terminal to prompt on and
    // without `--force`, GitDependencies keeps files the user modified
    // instead of overwriting them. UnLauncher never passes `--force`.
    let mut steps = vec![
        Step {
            stage: SETUP_SCRIPT.to_owned(),
            script: root.join(SETUP_SCRIPT),
            args: Vec::new(),
        },
        Step {
            stage: GENERATE_PROJECT_FILES_SCRIPT.to_owned(),
            script: root.join(GENERATE_PROJECT_FILES_SCRIPT),
            args: Vec::new(),
        },
    ];
    for target in &opts.targets {
        steps.push(Step {
            stage: format!("Build {target}"),
            script: root.join(BUILD_SCRIPT),
            args: vec![
                target.clone(),
                "Linux".to_owned(),
                opts.configuration.clone(),
                format!("-MaxParallelActions={jobs}"),
            ],
        });
    }
    steps
}

fn resolve_jobs(opts: &BuildOptions) -> usize {
    opts.jobs.filter(|&j| j > 0).unwrap_or_else(|| {
        detect_parallel_jobs(&opts.meminfo_path).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not detect memory; building with 1 job");
            1
        })
    })
}

/// Runs `Setup.sh`, `GenerateProjectFiles.sh` and `Build.sh` for each target,
/// each as a `reporter` stage.
///
/// Output is streamed line by line into `opts.log_path`, the progress parser
/// (`[n/total]` and `Setup.sh` percentages) and the known-errors table.
/// Pausing is honoured only between steps (a running compiler cannot be
/// paused safely); cancelling stops the running step at once by sending
/// SIGTERM, then SIGKILL after a grace period, to its whole process group.
/// Registering the engine afterwards is up to the caller (`ue-core`).
///
/// # Errors
///
/// - [`Error::MissingScript`] if a script is missing (checked before running
///   anything).
/// - [`Error::BuildFailed`] if a step exits with an error, with the
///   recognised [`crate::Diagnosis`] list and the log path.
/// - [`Error::Cancelled`] if cancelled.
/// - [`Error::Spawn`], [`Error::WriteFile`] if a step cannot start or the log
///   cannot be written.
pub async fn run_build(
    opts: &BuildOptions,
    sandbox: Sandbox,
    reporter: &Reporter,
) -> Result<BuildReport> {
    let started = Instant::now();
    let jobs = resolve_jobs(opts);
    let steps = steps(opts, jobs);
    for step in &steps {
        if !tokio::fs::try_exists(&step.script).await.unwrap_or(false) {
            return Err(Error::MissingScript {
                path: step.script.clone(),
            });
        }
    }
    let mut log = BuildLog::create(&opts.log_path).await?;
    tracing::info!(engine_root = %opts.engine_root.display(), jobs, "build started");
    for step in &steps {
        reporter.checkpoint().await?;
        reporter.stage(&step.stage);
        run_one(step, opts, sandbox, reporter, &mut log).await?;
    }
    log.flush().await?;
    let editor = ue_core::editor_executable(&opts.engine_root).ok();
    let duration = started.elapsed();
    tracing::info!(engine_root = %opts.engine_root.display(), ?duration, "build finished");
    Ok(BuildReport {
        log_path: opts.log_path.clone(),
        jobs,
        duration,
        editor,
    })
}

async fn run_one(
    step: &Step,
    opts: &BuildOptions,
    sandbox: Sandbox,
    reporter: &Reporter,
    log: &mut BuildLog,
) -> Result<()> {
    // Through bash, so a script that lost its executable bit still runs.
    let command = HostCommand::new("bash")
        .arg(&step.script)
        .args(&step.args)
        .current_dir(script_dir(&step.script, &opts.engine_root));
    log.line(&format!(
        "==> {} {}",
        step.script.display(),
        step.args.join(" ")
    ))
    .await?;
    let mut diagnoser = Diagnoser::new();
    let exit = run_step(
        &command,
        &step.stage,
        sandbox,
        reporter,
        log,
        &mut diagnoser,
    )
    .await;
    let exit = match exit {
        Ok(exit) => exit,
        Err(error) => {
            log.flush().await?;
            return Err(error);
        }
    };
    if exit.success {
        return Ok(());
    }
    if exit.killed {
        diagnoser.add_inferred("out-of-memory");
    }
    log.line(&format!("==> {} failed: {:?}", step.stage, exit.code))
        .await?;
    log.flush().await?;
    tracing::error!(stage = %step.stage, code = ?exit.code, "build step failed");
    Err(Error::BuildFailed {
        stage: step.stage.clone(),
        exit_code: exit.code,
        diagnosis: diagnoser.diagnoses(),
        log_path: opts.log_path.clone(),
    })
}

fn script_dir<'a>(script: &'a Path, fallback: &'a Path) -> &'a Path {
    script.parent().unwrap_or(fallback)
}
