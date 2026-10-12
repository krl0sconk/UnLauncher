// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli projects …`

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::Subcommand;
use common::{Overwrite, UserFacing};
use ue_core::{EngineAssociation, EngineRegistry, Project, discover_projects, resolve_engine};

use crate::confirm;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List projects and the engine each one uses.
    List {
        /// Also look for projects here (repeatable). Defaults:
        /// ~/Documents/Unreal Projects and ~/Unreal Projects.
        #[arg(long = "path", value_name = "DIR")]
        paths: Vec<PathBuf>,
        /// How many folder levels to look below each path.
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    /// Open a project in the editor of its engine.
    Open {
        /// The .uproject file.
        project: PathBuf,
    },
    /// Make a project use another registered engine.
    Associate {
        /// The .uproject file.
        project: PathBuf,
        /// Id of the engine, as shown by `engines list`.
        engine_id: String,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

pub fn run(command: Command, install_ini: &Path) -> anyhow::Result<()> {
    match command {
        Command::List { paths, depth } => list(install_ini, paths, depth),
        Command::Open { project } => open(install_ini, &project),
        Command::Associate {
            project,
            engine_id,
            yes,
        } => associate(install_ini, &project, &engine_id, yes),
    }
}

fn list(install_ini: &Path, mut paths: Vec<PathBuf>, depth: usize) -> anyhow::Result<()> {
    let registry = EngineRegistry::load(install_ini)?;
    paths.extend(system::paths::default_project_search_paths(
        &system::paths::home_dir()?,
    ));
    let results = discover_projects(&paths, depth);
    if results.is_empty() {
        println!("No projects found. Look elsewhere with --path <dir>.");
        return Ok(());
    }
    for result in results {
        match result {
            Ok(project) => {
                let association = match &project.association {
                    EngineAssociation::None => "(none)".to_owned(),
                    other => other.to_string(),
                };
                let engine = match resolve_engine(&project, &registry) {
                    Some(engine) => format!("-> {} ({})", engine.id, engine.version),
                    None => "-> no registered engine".to_owned(),
                };
                println!(
                    "{:<24} {:<40} {:<32} {}",
                    project.name,
                    association,
                    engine,
                    project.path.display()
                );
            }
            Err(error) => println!("[invalid] {}", error.summary()),
        }
    }
    Ok(())
}

fn open(install_ini: &Path, path: &Path) -> anyhow::Result<()> {
    let registry = EngineRegistry::load(install_ini)?;
    let project = Project::load(path)?;
    let engine = resolve_engine(&project, &registry).ok_or_else(|| {
        anyhow!(
            "{} uses engine {:?}, which is not registered; change it with `projects associate`",
            project.name,
            project.association.to_string()
        )
    })?;
    system::HostCommand::new(engine.editor_path()?)
        .arg(&project.path)
        .spawn_detached()?;
    println!(
        "Opening {} with {} ({})",
        project.name, engine.id, engine.version
    );
    Ok(())
}

fn associate(install_ini: &Path, path: &Path, engine_id: &str, yes: bool) -> anyhow::Result<()> {
    let registry = EngineRegistry::load(install_ini)?;
    let engine = registry.find(engine_id).ok_or_else(|| {
        let ids: Vec<_> = registry.engines().iter().map(|e| e.id.as_str()).collect();
        anyhow!(
            "no valid engine registered as {engine_id}; registered: {}",
            if ids.is_empty() {
                "(none)".to_owned()
            } else {
                ids.join(", ")
            }
        )
    })?;

    let mut project = Project::load(path)?;
    let old = project.association.clone();
    project.set_association(EngineAssociation::for_engine(engine));
    let question = format!(
        "Change {} from {:?} to {:?} ({} at {})?",
        project.path.display(),
        old.to_string(),
        project.association.to_string(),
        engine.version,
        engine.path.display()
    );
    if confirm::ask(&question, yes)? == Overwrite::No {
        anyhow::bail!("cancelled; nothing was changed");
    }
    let backup = project
        .save(Overwrite::Yes)
        .with_context(|| format!("could not update {}", project.path.display()))?;
    println!("{} now uses {}", project.name, engine.id);
    if let Some(backup) = backup {
        println!("Backup of the previous file: {}", backup.display());
    }
    Ok(())
}
