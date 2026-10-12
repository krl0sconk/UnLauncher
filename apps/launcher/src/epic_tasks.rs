// SPDX-License-Identifier: GPL-3.0-or-later

//! Tasks that talk to Epic: installing Fab assets (with the Epic session) and
//! downloading the official prebuilt engine (with a link listed by the web
//! window).

use std::path::PathBuf;
use std::sync::Arc;

use common::Overwrite;
use epic_auth::{EpicSession, KeyringStore};
use fab::{AssetSource, AssetVersion, EpicAssetSource, InstallTarget, LibraryAsset};
use jobs::{Reporter, Task, TaskError, TaskFuture, TaskManager, TaskOutcome};
use serde::{Deserialize, Serialize};
use ue_core::{EngineMetadata, EngineRegistry};

use crate::tasks::{TaskPaths, notify_finished};

/// Restores the Epic session saved in the keyring.
pub async fn session() -> Result<EpicSession, TaskError> {
    EpicSession::restore(Arc::new(KeyringStore))
        .await?
        .ok_or_else(|| {
            TaskError::failed(
                "not signed in to Epic",
                Some("Sign in from the Library screen, then retry.".to_owned()),
            )
        })
}

/// Where a Fab asset goes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Target {
    /// A `.uproject` file.
    Project(PathBuf),
    /// An engine root.
    Engine(PathBuf),
    /// A new project (from a complete-project asset) in `parent`, using the
    /// engine at `engine`.
    NewProject { parent: PathBuf, engine: PathBuf },
}

impl Target {
    fn to_install(&self) -> InstallTarget {
        match self {
            Self::Project(path) => InstallTarget::Project(path.clone()),
            Self::Engine(path) => InstallTarget::Engine(path.clone()),
            Self::NewProject { parent, .. } => InstallTarget::NewProject {
                parent: parent.clone(),
            },
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::NewProject { parent, .. } => format!("a new project in {}", parent.display()),
            Self::Project(path) | Self::Engine(path) => {
                path.file_stem().or_else(|| path.file_name()).map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            }
        }
    }
}

/// Downloads a Fab asset and installs it into a project or engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabTask {
    pub asset: LibraryAsset,
    pub version: AssetVersion,
    pub target: Target,
    pub cache_dir: PathBuf,
    /// Needed to set the engine of a new project.
    #[serde(default)]
    pub paths: Option<TaskPaths>,
}

impl FabTask {
    pub const KIND: &'static str = "fab-install";

    async fn execute(self, reporter: Reporter) -> Result<TaskOutcome, TaskError> {
        let session = session().await?;
        let source = EpicAssetSource::new(session);
        let downloaded = source
            .download(&self.asset, &self.version, &self.cache_dir, &reporter)
            .await
            .map_err(fab_error)?;
        reporter.stage("Installing");
        let (title, target) = (self.asset.title.clone(), self.target.to_install());
        let installed = tokio::task::spawn_blocking(move || {
            // The user chose the target; an existing folder is never replaced.
            fab::install(&downloaded, &title, &target, Overwrite::No)
        })
        .await
        .map_err(|_| TaskError::failed("the installation stopped unexpectedly", None))?
        .map_err(fab_error)?;
        if let (Target::NewProject { engine, .. }, Some(paths)) = (&self.target, &self.paths) {
            let (engine, paths, uproject) = (engine.clone(), paths.clone(), installed.clone());
            tokio::task::spawn_blocking(move || associate_new_project(&paths, &engine, &uproject))
                .await
                .map_err(|_| {
                    TaskError::failed("setting the project's engine stopped unexpectedly", None)
                })??;
        }
        Ok(TaskOutcome::new(format!(
            "{} installed in {}",
            self.asset.title,
            installed.display()
        )))
    }
}

/// Points a just-created project at the chosen engine. The project is
/// UnLauncher's own fresh copy, so the backup `save` makes is removed.
fn associate_new_project(
    paths: &TaskPaths,
    engine_root: &std::path::Path,
    uproject: &std::path::Path,
) -> Result<(), TaskError> {
    let registry = EngineRegistry::load(&paths.install_ini)?;
    let Some(engine) = registry.engines().iter().find(|e| e.path == engine_root) else {
        return Err(TaskError::failed(
            format!(
                "the engine at {} is no longer registered",
                engine_root.display()
            ),
            Some("Use Change engine… on the new project.".to_owned()),
        ));
    };
    let mut project = ue_core::Project::load(uproject)?;
    project.set_association(ue_core::EngineAssociation::for_engine(engine));
    if let Some(backup) = project.save(Overwrite::Yes)? {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

fn fab_error(error: fab::Error) -> TaskError {
    match error {
        fab::Error::Cancelled => TaskError::Cancelled,
        other => other.into(),
    }
}

impl Task for FabTask {
    fn kind(&self) -> &'static str {
        Self::KIND
    }

    fn title(&self) -> String {
        format!(
            "Install {} into {}",
            self.asset.title,
            self.target.describe()
        )
    }

    fn spec(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    fn pausable(&self) -> bool {
        true
    }

    fn run(self: Box<Self>, reporter: Reporter) -> TaskFuture {
        Box::pin(async move {
            let title = self.title();
            let result = self.execute(reporter).await;
            notify_finished(&title, &result).await;
            result
        })
    }
}

/// Downloads the official Linux prebuilt engine, imports it and registers it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadEngineTask {
    /// Version as listed by Epic, e.g. `5.7.4`.
    pub version: String,
    pub destination: PathBuf,
    pub downloads_dir: PathBuf,
    pub paths: TaskPaths,
    /// The download as listed by the web window. Its signed link expires and
    /// is never saved, so a task recreated after a restart has none and asks
    /// to list again (the partial download resumes).
    #[serde(skip)]
    pub download: Option<ue_prebuilt::EngineDownload>,
}

impl DownloadEngineTask {
    pub const KIND: &'static str = "download-engine";

    async fn execute(self, reporter: Reporter) -> Result<TaskOutcome, TaskError> {
        let file_name = format!("Linux_Unreal_Engine_{}.zip", self.version);
        let file_name = self
            .download
            .as_ref()
            .map_or(file_name, |d| d.file_name.clone());
        // Hidden, next to the destination: same disk as the engine, and
        // removed once it is extracted.
        let zip = self.downloads_dir.join(format!(".{file_name}"));
        tokio::fs::create_dir_all(&self.downloads_dir)
            .await
            .map_err(|e| {
                TaskError::failed(
                    format!("cannot create {}: {e}", self.downloads_dir.display()),
                    None,
                )
            })?;
        if !zip.exists() {
            let download = self.download.as_ref().ok_or_else(|| {
                TaskError::failed(
                    "the download link has expired",
                    Some(
                        "Open Engines → Install engine… → List available versions and choose \
                         this version again; the download resumes where it stopped."
                            .to_owned(),
                    ),
                )
            })?;
            let http = epic_auth::http_client()?;
            ue_prebuilt::download_engine(&http, download, &zip, &reporter)
                .await
                .map_err(prebuilt_error)?;
        }

        let (paths, destination, zip_path) = (self.paths, self.destination, zip.clone());
        let engine = tokio::task::spawn_blocking(move || -> Result<ue_core::Engine, TaskError> {
            let imported = ue_prebuilt::import_zip(&zip_path, &destination, &reporter)
                .map_err(prebuilt_error)?;
            reporter.stage("Registering");
            let mut registry = EngineRegistry::load(&paths.install_ini)?;
            let mut metadata = EngineMetadata::load(&paths.metadata)?;
            let engine =
                ue_prebuilt::register_imported(&mut registry, &mut metadata, &imported, None)?;
            registry.save(Overwrite::Yes)?;
            metadata.save()?;
            Ok(engine)
        })
        .await
        .map_err(|_| TaskError::failed("the import stopped unexpectedly", None))??;

        // The zip is UnLauncher's own download; it is not needed after importing.
        if let Err(error) = tokio::fs::remove_file(&zip).await {
            tracing::warn!(%error, path = %zip.display(), "cannot remove downloaded zip");
        }
        Ok(TaskOutcome::new(format!(
            "Unreal Engine {} installed at {} as {}",
            engine.version,
            engine.path.display(),
            engine.id
        )))
    }
}

fn prebuilt_error(error: ue_prebuilt::Error) -> TaskError {
    match error {
        ue_prebuilt::Error::Cancelled => TaskError::Cancelled,
        other => other.into(),
    }
}

impl Task for DownloadEngineTask {
    fn kind(&self) -> &'static str {
        Self::KIND
    }

    fn title(&self) -> String {
        format!("Download Unreal Engine {}", self.version)
    }

    fn spec(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    fn pausable(&self) -> bool {
        true
    }

    fn run(self: Box<Self>, reporter: Reporter) -> TaskFuture {
        Box::pin(async move {
            let title = self.title();
            let result = self.execute(reporter).await;
            notify_finished(&title, &result).await;
            result
        })
    }
}

/// Registers how to recreate the Epic tasks after a restart.
pub fn register_factories(manager: &TaskManager) {
    fn factory<T: Task + serde::de::DeserializeOwned>(
        spec: serde_json::Value,
    ) -> Result<Box<dyn Task>, TaskError> {
        serde_json::from_value::<T>(spec)
            .map(|task| Box::new(task) as Box<dyn Task>)
            .map_err(|error| {
                TaskError::failed(format!("cannot read the saved task: {error}"), None)
            })
    }
    manager.register_factory(FabTask::KIND, factory::<FabTask>);
    manager.register_factory(DownloadEngineTask::KIND, factory::<DownloadEngineTask>);
}
