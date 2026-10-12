// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli fab …` and `download-engine`.

use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand};
use common::Overwrite;
use epic_auth::{EpicSession, KeyringStore};
use fab::{AssetSource, EpicAssetSource, InstallTarget};
use ue_core::{EngineRegistry, Project, resolve_engine};

use crate::{confirm, progress};

#[derive(Debug, Subcommand)]
pub enum FabCommand {
    /// Sign in to Epic: opens the login page and reads the code from stdin.
    Login,
    /// Sign out and forget the saved login.
    Logout,
    /// List the assets in your Fab library.
    List {
        /// Only assets with a version for this engine (e.g. 5.7).
        #[arg(long)]
        engine: Option<String>,
    },
    /// Download an asset and install it into a project or engine.
    Install(InstallArgs),
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Asset id, as shown by `fab list`.
    asset: String,
    /// The .uproject to install into.
    #[arg(long, conflicts_with = "engine", required_unless_present = "engine")]
    project: Option<PathBuf>,
    /// Id of the engine to install a plugin into.
    #[arg(long)]
    engine: Option<String>,
    /// Do not ask for confirmation.
    #[arg(long, short)]
    yes: bool,
}

#[derive(Debug, Args)]
pub struct DownloadEngineArgs {
    /// Ignored: kept so old scripts get the explanation instead of a usage error.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    _rest: Vec<String>,
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Runtime::new()?)
}

fn session(rt: &tokio::runtime::Runtime) -> anyhow::Result<EpicSession> {
    rt.block_on(EpicSession::restore(Arc::new(KeyringStore)))?
        .ok_or_else(|| anyhow!("not signed in to Epic; run `unlauncher-cli fab login`"))
}

fn data_dir() -> anyhow::Result<PathBuf> {
    Ok(system::paths::app_data_dir()?)
}

pub fn run(command: &FabCommand, install_ini: &Path) -> anyhow::Result<()> {
    let rt = runtime()?;
    match command {
        FabCommand::Login => login(&rt),
        FabCommand::Logout => {
            if let Some(session) = rt.block_on(EpicSession::restore(Arc::new(KeyringStore)))? {
                rt.block_on(session.logout())?;
            }
            println!("Signed out.");
            Ok(())
        }
        FabCommand::List { engine } => list(&rt, engine.as_deref()),
        FabCommand::Install(args) => install(&rt, args, install_ini),
    }
}

fn login(rt: &tokio::runtime::Runtime) -> anyhow::Result<()> {
    eprintln!(
        "UnLauncher uses Epic's private launcher API, which Epic does not support for \
         third-party apps; it may stop working at any time.\n"
    );
    eprintln!("1. Sign in at:\n   {}", epic_auth::LOGIN_URL);
    if let Err(error) = system::open_url(epic_auth::LOGIN_URL) {
        tracing::debug!(%error, "cannot open the browser");
    }
    if std::io::stdin().is_terminal() {
        eprintln!("2. Paste the text Epic shows (with \"authorizationCode\") and press Enter:");
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let code = epic_auth::parse_authorization_code(&line)?;
    let session = rt.block_on(EpicSession::login(&code, Arc::new(KeyringStore)))?;
    println!("Signed in as {}.", session.display_name());
    Ok(())
}

fn parse_engine(version: &str) -> anyhow::Result<ue_core::EngineVersion> {
    let (major, minor) = version
        .split_once('.')
        .context("engine version must look like 5.7")?;
    Ok(ue_core::EngineVersion::new(
        major.parse()?,
        minor.split('.').next().unwrap_or("0").parse()?,
        0,
    ))
}

fn list(rt: &tokio::runtime::Runtime, engine: Option<&str>) -> anyhow::Result<()> {
    let engine = engine.map(parse_engine).transpose()?;
    let source = EpicAssetSource::new(session(rt)?);
    let assets = rt.block_on(source.list())?;
    for asset in &assets {
        if let Some(engine) = engine
            && fab::compatible_version(asset, engine).is_none()
        {
            continue;
        }
        let mut versions: Vec<_> = asset
            .versions
            .iter()
            .flat_map(|v| v.engine_versions.iter())
            .map(|(a, b)| format!("{a}.{b}"))
            .collect();
        versions.sort();
        versions.dedup();
        println!(
            "{:<36} {:<8} {}  [{}]",
            asset.id,
            format!("{:?}", asset.kind),
            asset.title,
            versions.join(", ")
        );
    }
    Ok(())
}

fn install(
    rt: &tokio::runtime::Runtime,
    args: &InstallArgs,
    install_ini: &Path,
) -> anyhow::Result<()> {
    let registry = EngineRegistry::load(install_ini)?;
    let (target, engine_version) = if let Some(project) = &args.project {
        let loaded = Project::load(project)?;
        let engine = resolve_engine(&loaded, &registry)
            .with_context(|| format!("{} has no installed engine", loaded.name))?;
        (InstallTarget::Project(project.clone()), engine.version)
    } else {
        let id = args.engine.as_deref().unwrap_or_default();
        let engine = registry
            .find(id)
            .with_context(|| format!("no engine registered as {id}"))?;
        (InstallTarget::Engine(engine.path.clone()), engine.version)
    };

    let session = session(rt)?;
    let source = EpicAssetSource::new(session);
    let assets = rt.block_on(source.list())?;
    let asset = assets
        .into_iter()
        .find(|a| a.id == args.asset)
        .with_context(|| format!("asset {} is not in your library", args.asset))?;
    let version = fab::compatible_version(&asset, engine_version)
        .cloned()
        .with_context(|| {
            format!(
                "{} has no version for Unreal Engine {engine_version}",
                asset.title
            )
        })?;
    if confirm::ask(&format!("Download and install {}?", asset.title), args.yes)? == Overwrite::No {
        bail!("cancelled; nothing was changed");
    }

    let cache = data_dir()?.join("cache/fab");
    let downloaded = {
        let (asset, version) = (asset.clone(), version.clone());
        progress::run(move |reporter| async move {
            source.download(&asset, &version, &cache, &reporter).await
        })??
    };
    let installed = fab::install(&downloaded, &asset.title, &target, Overwrite::No)?;
    println!("Installed {} in {}", asset.title, installed.display());
    Ok(())
}

/// The official prebuilt engine can only be listed from inside a browser
/// (Cloudflare, Epic's web session), so the CLI explains the two ways that
/// work.
pub fn download_engine(_args: &DownloadEngineArgs) -> anyhow::Result<()> {
    bail!(
        "the official prebuilt engine cannot be downloaded from the command line: \
         unrealengine.com only answers real browsers.\n\
         - In UnLauncher: Engines → Install engine… → List available versions.\n\
         - Or download the Linux zip from https://www.unrealengine.com/linux in your browser \
         and run `unlauncher-cli import <zip> <destination>`."
    )
}
