// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli engines …`

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Subcommand;
use common::{Overwrite, UserFacing};
use ue_core::{EngineId, EngineRegistry, discover_engines};

use crate::confirm;

/// How deep to look for engines below each search path.
const ENGINE_SEARCH_DEPTH: usize = 3;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List registered engines and engines found on disk that are not registered.
    List {
        /// Also look for engines here (repeatable). Defaults: ~ and /opt.
        #[arg(long = "path", value_name = "DIR")]
        paths: Vec<PathBuf>,
        /// Only list registered engines; do not scan the disk.
        #[arg(long)]
        no_scan: bool,
    },
    /// Add an engine to Install.ini.
    Register {
        /// Engine root: the folder that contains `Engine/`.
        path: PathBuf,
        /// Id to use. Default: UE_<major>.<minor>, or a new GUID if taken.
        #[arg(long)]
        id: Option<String>,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Show the disk space an engine and its caches use.
    Usage {
        /// Id of the engine, as shown by `engines list`.
        id: String,
    },
    /// Empty an engine's caches (and the shared ones with --shared).
    Clean {
        /// Id of the engine, as shown by `engines list`.
        id: String,
        /// Also clean the caches shared by every engine.
        #[arg(long)]
        shared: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Delete an engine's folder and remove it from Install.ini. Projects are not touched.
    Uninstall {
        /// Id of the engine, as shown by `engines list`.
        id: String,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Remove an engine from Install.ini. The engine files are not touched.
    Unregister {
        /// Id of the engine, as shown by `engines list`.
        id: String,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

pub fn run(command: Command, install_ini: &Path) -> anyhow::Result<()> {
    match command {
        Command::List { paths, no_scan } => list(install_ini, paths, no_scan),
        Command::Register { path, id, yes } => register(install_ini, &path, id, yes),
        Command::Unregister { id, yes } => unregister(install_ini, &id, yes),
        Command::Usage { id } => usage(install_ini, &id),
        Command::Clean { id, shared, yes } => clean(install_ini, &id, shared, yes),
        Command::Uninstall { id, yes } => uninstall(install_ini, &id, yes),
    }
}

fn list(install_ini: &Path, mut paths: Vec<PathBuf>, no_scan: bool) -> anyhow::Result<()> {
    let registry = EngineRegistry::load(install_ini)?;
    println!("Registered in {}:", install_ini.display());
    if registry.engines().is_empty() && registry.invalid_entries().is_empty() {
        println!("  (none)");
    }
    for engine in registry.engines() {
        println!(
            "  {:<40} {:<8} {}",
            engine.id,
            engine.version,
            engine.path.display()
        );
    }
    for entry in registry.invalid_entries() {
        println!(
            "  {:<40} {:<8} {}  [invalid: {}]",
            entry.id,
            "?",
            entry.path.display(),
            entry.error.summary()
        );
    }

    if no_scan {
        return Ok(());
    }
    paths.extend(system::paths::default_engine_search_paths(
        &system::paths::home_dir()?,
    ));
    let unregistered: Vec<_> = discover_engines(&paths, ENGINE_SEARCH_DEPTH)
        .into_iter()
        .filter(|found| {
            !registry
                .engines()
                .iter()
                .any(|e| same_dir(&e.path, &found.path))
        })
        .collect();
    if !unregistered.is_empty() {
        println!("\nFound on disk, not registered:");
        for engine in unregistered {
            println!("  {:<8} {}", engine.version, engine.path.display());
        }
        println!("\nRegister one with: unlauncher-cli engines register <path>");
    }
    Ok(())
}

fn register(install_ini: &Path, path: &Path, id: Option<String>, yes: bool) -> anyhow::Result<()> {
    let mut registry = EngineRegistry::load(install_ini)?;
    let id = id.map(EngineId::new).transpose()?;
    let engine = registry.register(path, id)?.clone();

    let question = format!(
        "Register {} {} as {} in {}?",
        engine.version,
        engine.path.display(),
        engine.id,
        install_ini.display()
    );
    save(&registry, &question, yes)?;
    println!("Registered {} ({})", engine.id, engine.version);
    Ok(())
}

fn unregister(install_ini: &Path, id: &str, yes: bool) -> anyhow::Result<()> {
    let mut registry = EngineRegistry::load(install_ini)?;
    let id = EngineId::new(id)?;
    let path = registry.unregister(&id)?;

    let question = format!(
        "Remove {id} ({}) from {}? The engine files are not deleted.",
        path.display(),
        install_ini.display()
    );
    save(&registry, &question, yes)?;
    println!("Unregistered {id}");
    Ok(())
}

/// Asks for confirmation and writes `Install.ini`, reporting the backup.
fn save(registry: &EngineRegistry, question: &str, yes: bool) -> anyhow::Result<()> {
    if confirm::ask(question, yes)? == Overwrite::No {
        anyhow::bail!("cancelled; nothing was changed");
    }
    let backup = registry
        .save(Overwrite::Yes)
        .with_context(|| format!("could not update {}", registry.path().display()))?;
    if let Some(backup) = backup {
        println!("Backup of the previous file: {}", backup.display());
    }
    Ok(())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn find_engine(install_ini: &Path, id: &str) -> anyhow::Result<ue_core::Engine> {
    EngineRegistry::load(install_ini)?
        .find(id)
        .cloned()
        .with_context(|| format!("no valid engine registered as {id}"))
}

fn unreal_config_dir() -> anyhow::Result<PathBuf> {
    Ok(system::paths::unreal_config_dir_in(
        &system::paths::home_dir()?,
    ))
}

fn usage(install_ini: &Path, id: &str) -> anyhow::Result<()> {
    let engine = find_engine(install_ini, id)?;
    println!(
        "{}  {}",
        format_gb(ue_core::dir_size(&engine.path)),
        engine.path.display()
    );
    for cache in ue_core::caches(&engine.path, &unreal_config_dir()?) {
        let scope = if cache.shared { " (shared)" } else { "" };
        println!(
            "  {}  {}{scope}: {}",
            format_gb(cache.size),
            cache.name,
            cache.path.display()
        );
    }
    Ok(())
}

fn clean(install_ini: &Path, id: &str, shared: bool, yes: bool) -> anyhow::Result<()> {
    let engine = find_engine(install_ini, id)?;
    let caches: Vec<_> = ue_core::caches(&engine.path, &unreal_config_dir()?)
        .into_iter()
        .filter(|c| (shared || !c.shared) && c.size > 0)
        .collect();
    if caches.is_empty() {
        println!("Nothing to clean.");
        return Ok(());
    }
    let list: Vec<_> = caches
        .iter()
        .map(|c| format!("{} ({})", c.path.display(), format_gb(c.size)))
        .collect();
    let question = format!(
        "Delete the contents of:\n  {}\nClose every Unreal Editor first. Continue?",
        list.join("\n  ")
    );
    if confirm::ask(&question, yes)? == Overwrite::No {
        anyhow::bail!("cancelled; nothing was deleted");
    }
    let mut freed = 0;
    for cache in &caches {
        freed += ue_core::clean_cache(&cache.path, common::Delete::Yes)?;
    }
    println!("Freed {}", format_gb(freed));
    Ok(())
}

fn uninstall(install_ini: &Path, id: &str, yes: bool) -> anyhow::Result<()> {
    let mut registry = EngineRegistry::load(install_ini)?;
    let id = EngineId::new(id)?;
    let metadata_path = system::paths::app_data_dir()?.join("engines.toml");
    let mut metadata = ue_core::EngineMetadata::load(&metadata_path)?;
    let question =
        format!("PERMANENTLY delete engine {id} and its folder? Projects are not touched.");
    if confirm::ask(&question, yes)? == Overwrite::No {
        anyhow::bail!("cancelled; nothing was deleted");
    }
    let path = ue_core::uninstall_engine(&mut registry, &mut metadata, &id, common::Delete::Yes)?;
    println!("Uninstalled {id} ({})", path.display());
    Ok(())
}

#[allow(clippy::cast_precision_loss, reason = "display only")]
fn format_gb(bytes: u64) -> String {
    format!("{:7.2} GB", bytes as f64 / 1e9)
}
