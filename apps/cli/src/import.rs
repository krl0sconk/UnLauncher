// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli import …`

use std::path::{Path, PathBuf};

use anyhow::bail;
use clap::Args;
use common::Overwrite;
use ue_core::{EngineId, EngineMetadata, EngineRegistry};

use crate::{confirm, progress};

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// The Linux Unreal Engine zip downloaded from Epic.
    zip: PathBuf,
    /// Folder to extract it to (must not exist or be empty).
    destination: PathBuf,
    /// Id to register it with. Default: UE_<major>.<minor>, or a new GUID if taken.
    #[arg(long)]
    id: Option<String>,
    /// Do not ask for confirmation.
    #[arg(long, short)]
    yes: bool,
}

pub fn run(args: ImportArgs, install_ini: &Path) -> anyhow::Result<()> {
    let id = args.id.map(EngineId::new).transpose()?;
    let question = format!(
        "Extract {} to {} and register it in {}?",
        args.zip.display(),
        args.destination.display(),
        install_ini.display()
    );
    if confirm::ask(&question, args.yes)? == Overwrite::No {
        bail!("cancelled; nothing was changed");
    }

    let (zip, destination) = (args.zip.clone(), args.destination.clone());
    let imported = progress::run(move |reporter| async move {
        tokio::task::spawn_blocking(move || ue_prebuilt::import_zip(&zip, &destination, &reporter))
            .await
    })???;

    let metadata_path = system::paths::app_data_dir()?.join("engines.toml");
    let mut registry = EngineRegistry::load(install_ini)?;
    let mut metadata = EngineMetadata::load(&metadata_path)?;
    let engine = ue_prebuilt::register_imported(&mut registry, &mut metadata, &imported, id)?;
    if let Some(backup) = registry.save(Overwrite::Yes)? {
        println!("Backup of the previous Install.ini: {}", backup.display());
    }
    metadata.save()?;
    println!(
        "Installed Unreal Engine {} at {} as {}",
        engine.version,
        engine.path.display(),
        engine.id
    );
    Ok(())
}
