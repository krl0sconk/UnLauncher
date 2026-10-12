// SPDX-License-Identifier: GPL-3.0-or-later

//! Long tasks run by the Tasks screen: importing and building engines.

use std::path::{Path, PathBuf};

use common::Overwrite;
use jobs::{Reporter, Task, TaskError, TaskFuture, TaskManager, TaskOutcome};
use serde::{Deserialize, Serialize};
use system::Sandbox;
use ue_core::{EngineId, EngineMetadata, EngineRegistry};

/// Paths a task needs, recorded in its spec so it can be retried after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPaths {
    pub install_ini: PathBuf,
    pub metadata: PathBuf,
}

/// Imports a prebuilt engine zip and registers it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportTask {
    pub zip: PathBuf,
    pub destination: PathBuf,
    pub id: Option<String>,
    pub paths: TaskPaths,
}

impl ImportTask {
    pub const KIND: &'static str = "import-engine";
}

impl Task for ImportTask {
    fn kind(&self) -> &'static str {
        Self::KIND
    }

    fn title(&self) -> String {
        format!(
            "Import {}",
            self.zip.file_name().map_or_else(
                || self.zip.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
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
            let result = tokio::task::spawn_blocking(move || self.run_blocking(&reporter))
                .await
                .unwrap_or_else(|_| {
                    Err(TaskError::failed("the import stopped unexpectedly", None))
                });
            notify_finished(&title, &result).await;
            result
        })
    }
}

impl ImportTask {
    fn run_blocking(&self, reporter: &Reporter) -> Result<TaskOutcome, TaskError> {
        let imported = match ue_prebuilt::import_zip(&self.zip, &self.destination, reporter) {
            Ok(imported) => imported,
            Err(ue_prebuilt::Error::Cancelled) => return Err(TaskError::Cancelled),
            Err(error) => return Err(error.into()),
        };
        reporter.stage("Registering");
        let id = self.id.clone().map(EngineId::new).transpose()?;
        let mut registry = EngineRegistry::load(&self.paths.install_ini)?;
        let mut metadata = EngineMetadata::load(&self.paths.metadata)?;
        let engine = ue_prebuilt::register_imported(&mut registry, &mut metadata, &imported, id)?;
        // The user confirmed the import, which includes registering the engine.
        registry.save(Overwrite::Yes)?;
        metadata.save()?;
        Ok(TaskOutcome::new(format!(
            "Unreal Engine {} installed at {} as {}",
            engine.version,
            engine.path.display(),
            engine.id
        )))
    }
}

/// Shows a desktop notification when a task ends. Failures are only logged.
pub async fn notify_finished(title: &str, result: &Result<TaskOutcome, TaskError>) {
    let body = match result {
        Ok(outcome) => outcome.message.clone(),
        Err(TaskError::Cancelled) => return,
        Err(TaskError::Failed { summary, .. }) => format!("Failed: {summary}"),
    };
    let title = title.to_owned();
    // A notification must never keep a finished task from ending.
    let shown = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::task::spawn_blocking(move || system::notify(Sandbox::detect(), &title, &body)),
    )
    .await;
    match shown {
        Ok(Ok(Err(error))) => tracing::warn!(%error, "cannot show notification"),
        Err(_) => tracing::warn!("the notification did not answer; the task ends anyway"),
        _ => {}
    }
}

/// Registers how to recreate each task kind, so interrupted tasks can be retried.
pub fn register_factories(manager: &TaskManager) {
    manager.register_factory(ImportTask::KIND, |spec| {
        serde_json::from_value::<ImportTask>(spec)
            .map(|task| Box::new(task) as Box<dyn Task>)
            .map_err(|error| {
                TaskError::failed(format!("cannot read the saved task: {error}"), None)
            })
    });
}

/// Whether `path` can receive an engine: missing, or an empty directory.
pub fn is_free_destination(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Reads the GitHub token from the keyring.
async fn github_token() -> Result<common::Secret, TaskError> {
    use epic_auth::{CredentialKey, KeyringStore, SecretStore};
    let token = tokio::task::spawn_blocking(|| KeyringStore.get(CredentialKey::GitHubToken))
        .await
        .map_err(|_| TaskError::failed("cannot read the keyring", None))??;
    token.ok_or_else(|| {
        TaskError::failed(
            "no GitHub token saved",
            Some("Open Install engine → From source and save a GitHub token.".to_owned()),
        )
    })
}

fn build_error(error: ue_build::Error) -> TaskError {
    match error {
        ue_build::Error::Cancelled => TaskError::Cancelled,
        other => other.into(),
    }
}

fn log_path(logs_dir: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    logs_dir.join(format!("build-{stamp}.log"))
}

/// Registers (or refreshes) a source engine and saves Install.ini and metadata.
async fn register_source(
    paths: TaskPaths,
    root: PathBuf,
    git_ref: String,
) -> Result<ue_core::Engine, TaskError> {
    tokio::task::spawn_blocking(move || -> Result<ue_core::Engine, TaskError> {
        let mut registry = EngineRegistry::load(&paths.install_ini)?;
        let mut metadata = EngineMetadata::load(&paths.metadata)?;
        let engine = ue_build::register_built(
            &mut registry,
            &mut metadata,
            &root,
            ue_build::UE_REMOTE,
            &git_ref,
            None,
        )
        .map_err(build_error)?;
        // The user confirmed the install, which includes registering the engine.
        registry.save(Overwrite::Yes)?;
        metadata.save()?;
        Ok(engine)
    })
    .await
    .map_err(|_| TaskError::failed("registering the engine stopped unexpectedly", None))?
}

/// Clones Unreal Engine from GitHub, builds it and registers it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTask {
    pub git_ref: String,
    pub destination: PathBuf,
    pub jobs: Option<usize>,
    pub paths: TaskPaths,
    pub logs_dir: PathBuf,
}

impl BuildTask {
    pub const KIND: &'static str = "build-engine";

    async fn execute(self, reporter: Reporter) -> Result<TaskOutcome, TaskError> {
        let token = github_token().await?;
        let request = ue_build::SourceInstall {
            remote: ue_build::UE_REMOTE.to_owned(),
            git_ref: self.git_ref.clone(),
            destination: self.destination.clone(),
            jobs: self.jobs,
            log_path: log_path(&self.logs_dir),
        };
        let report = ue_build::install_from_source(&request, &token, Sandbox::detect(), &reporter)
            .await
            .map_err(build_error)?;
        reporter.stage("Registering");
        let engine = register_source(self.paths, self.destination, self.git_ref).await?;
        Ok(TaskOutcome::new(format!(
            "Unreal Engine {} built in {} min and registered as {} (log: {})",
            engine.version,
            report.duration.as_secs() / 60,
            engine.id,
            report.log_path.display()
        )))
    }
}

impl Task for BuildTask {
    fn kind(&self) -> &'static str {
        Self::KIND
    }

    fn title(&self) -> String {
        format!("Build Unreal Engine {} from source", self.git_ref)
    }

    fn spec(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    fn pausable(&self) -> bool {
        false
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

/// Fetches a newer revision of a source engine and rebuilds it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTask {
    pub engine_root: PathBuf,
    pub git_ref: String,
    pub jobs: Option<usize>,
    pub paths: TaskPaths,
    pub logs_dir: PathBuf,
}

impl UpdateTask {
    pub const KIND: &'static str = "update-engine";

    async fn execute(self, reporter: Reporter) -> Result<TaskOutcome, TaskError> {
        let token = github_token().await?;
        let mut options = ue_build::BuildOptions::new(&self.engine_root, log_path(&self.logs_dir));
        options.jobs = self.jobs;
        let (outcome, _report) = ue_build::update_from_source(
            &self.engine_root,
            &self.git_ref,
            &token,
            &options,
            Sandbox::detect(),
            &reporter,
        )
        .await
        .map_err(build_error)?;
        let engine = register_source(self.paths, self.engine_root, self.git_ref).await?;
        Ok(TaskOutcome::new(if outcome.is_up_to_date() {
            format!("{} was already up to date; rebuilt", engine.id)
        } else {
            format!(
                "{} updated to {} and rebuilt",
                engine.id, outcome.new_commit
            )
        }))
    }
}

impl Task for UpdateTask {
    fn kind(&self) -> &'static str {
        Self::KIND
    }

    fn title(&self) -> String {
        format!("Update {} ({})", self.engine_root.display(), self.git_ref)
    }

    fn spec(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    fn pausable(&self) -> bool {
        false
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
