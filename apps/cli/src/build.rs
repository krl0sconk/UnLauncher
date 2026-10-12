// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli build …`, `update …` and `github-token …`.

use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand};
use common::{Overwrite, Secret};
use epic_auth::{CredentialKey, KeyringStore, SecretStore};
use system::Sandbox;
use ue_build::{CheckStatus, SourceInstall, UE_REMOTE};
use ue_core::{EngineKind, EngineMetadata, EngineRegistry};

use crate::{confirm, progress};

/// Environment variable that overrides the stored GitHub token.
const TOKEN_ENV: &str = "UNLAUNCHER_GITHUB_TOKEN";

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Folder for the engine source (must not exist, be empty, or hold a previous
    /// UnLauncher clone to resume).
    destination: PathBuf,
    /// Branch or tag of EpicGames/UnrealEngine.
    #[arg(long = "ref", default_value = "release")]
    git_ref: String,
    /// Parallel compile jobs. Default: from cores and available RAM.
    #[arg(long)]
    jobs: Option<usize>,
    /// Build even if the requirement checks fail.
    #[arg(long)]
    skip_checks: bool,
    /// Do not ask for confirmation.
    #[arg(long, short)]
    yes: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Id of an engine built from source, as shown by `engines list`.
    id: String,
    /// Parallel compile jobs. Default: from cores and available RAM.
    #[arg(long)]
    jobs: Option<usize>,
    /// Do not ask for confirmation.
    #[arg(long, short)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Save a GitHub token in the system keyring (read from stdin).
    Set,
    /// Delete the saved GitHub token.
    Forget,
}

fn metadata_path() -> anyhow::Result<PathBuf> {
    Ok(system::paths::app_data_dir()?.join("engines.toml"))
}

fn log_path() -> anyhow::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Ok(system::paths::app_data_dir()?
        .join("logs")
        .join(format!("build-{stamp}.log")))
}

fn github_token() -> anyhow::Result<Secret> {
    if let Some(token) = std::env::var_os(TOKEN_ENV).and_then(|t| t.into_string().ok()) {
        return Ok(Secret::new(token));
    }
    KeyringStore
        .get(CredentialKey::GitHubToken)?
        .ok_or_else(|| {
            anyhow!(
                "GitHub is not connected; run `unlauncher-cli github login` (or set {TOKEN_ENV})"
            )
        })
}

pub fn token(command: &TokenCommand) -> anyhow::Result<()> {
    match command {
        TokenCommand::Set => {
            if std::io::stdin().is_terminal() {
                eprintln!("Paste the GitHub token and press Enter (it will be visible):");
            }
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            let token = line.trim();
            if token.is_empty() {
                bail!("no token given");
            }
            KeyringStore.set(CredentialKey::GitHubToken, &Secret::new(token))?;
            println!("GitHub token saved in the system keyring.");
        }
        TokenCommand::Forget => {
            KeyringStore.delete(CredentialKey::GitHubToken)?;
            println!("GitHub token deleted.");
        }
    }
    Ok(())
}

pub fn build(args: &BuildArgs, install_ini: &Path) -> anyhow::Result<()> {
    let token = github_token()?;
    let sandbox = Sandbox::detect();

    let input = ue_build::PreflightInput::new(&args.destination);
    let http = epic_auth::http_client()?;
    let checks = tokio::runtime::Runtime::new()?.block_on(ue_build::preflight(
        &input,
        sandbox,
        &http,
        Some(&token),
    ));
    for check in &checks {
        let icon = match check.status {
            CheckStatus::Ok => "ok  ",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        println!("[{icon}] {}: {}", check.name, check.message);
        if let Some(hint) = check
            .hint
            .as_ref()
            .filter(|_| check.status != CheckStatus::Ok)
        {
            println!("       {hint}");
        }
    }
    if ue_build::overall_status(&checks) == CheckStatus::Fail && !args.skip_checks {
        bail!("requirements not met; fix them or pass --skip-checks");
    }

    let question = format!(
        "Clone {} ({}) into {} and build it? This takes hours.",
        UE_REMOTE,
        args.git_ref,
        args.destination.display()
    );
    if confirm::ask(&question, args.yes)? == Overwrite::No {
        bail!("cancelled; nothing was changed");
    }

    let request = SourceInstall {
        remote: UE_REMOTE.to_owned(),
        git_ref: args.git_ref.clone(),
        destination: args.destination.clone(),
        jobs: args.jobs,
        log_path: log_path()?,
    };
    let report = progress::run(move |reporter| async move {
        ue_build::install_from_source(&request, &token, sandbox, &reporter).await
    })??;
    println!("Build log: {}", report.log_path.display());

    let mut registry = EngineRegistry::load(install_ini)?;
    let mut metadata = EngineMetadata::load(&metadata_path()?)?;
    let engine = ue_build::register_built(
        &mut registry,
        &mut metadata,
        &args.destination,
        UE_REMOTE,
        &args.git_ref,
        None,
    )?;
    registry.save(Overwrite::Yes)?;
    metadata.save()?;
    println!(
        "Built Unreal Engine {} and registered it as {}",
        engine.version, engine.id
    );
    Ok(())
}

pub fn update(args: &UpdateArgs, install_ini: &Path) -> anyhow::Result<()> {
    let mut registry = EngineRegistry::load(install_ini)?;
    let mut metadata = EngineMetadata::load(&metadata_path()?)?;
    registry.apply_metadata(&metadata);
    let engine = registry
        .find(&args.id)
        .cloned()
        .with_context(|| format!("no engine registered as {}", args.id))?;
    let EngineKind::Source { git_ref, .. } = &engine.kind else {
        bail!(
            "{} was not built from source by UnLauncher; only those can be updated",
            engine.id
        );
    };
    let git_ref = git_ref.clone();
    let token = github_token()?;
    let question = format!(
        "Fetch the latest {git_ref} for {} and rebuild it? Local changes stop the update.",
        engine.id
    );
    if confirm::ask(&question, args.yes)? == Overwrite::No {
        bail!("cancelled; nothing was changed");
    }

    let mut options = ue_build::BuildOptions::new(&engine.path, log_path()?);
    options.jobs = args.jobs;
    let (root, reference) = (engine.path.clone(), git_ref.clone());
    let (outcome, report) = progress::run(move |reporter| async move {
        ue_build::update_from_source(
            &root,
            &reference,
            &token,
            &options,
            Sandbox::detect(),
            &reporter,
        )
        .await
    })??;
    ue_build::register_built(
        &mut registry,
        &mut metadata,
        &engine.path,
        UE_REMOTE,
        &git_ref,
        None,
    )?;
    metadata.save()?;
    println!(
        "{}: {} → {} (log: {})",
        engine.id,
        outcome.old_commit,
        outcome.new_commit,
        report.log_path.display()
    );
    Ok(())
}
