// SPDX-License-Identifier: GPL-3.0-or-later

//! What the interface does, without the interface: loading engines and
//! projects and changing them. Every function here may block on disk I/O, so
//! the interface only calls them from worker threads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::{Overwrite, UserFacing};
use system::{DesktopShortcut, install_shortcut};
use system::{HostCommand, Sandbox};
use ue_core::{
    DiscoveredEngine, Engine, EngineAssociation, EngineId, EngineMetadata, EngineRegistry, Project,
    ProjectTemplate, discover_engines, discover_projects, resolve_engine,
};

/// How deep to look below each search path.
const ENGINE_SEARCH_DEPTH: usize = 3;
const PROJECT_SEARCH_DEPTH: usize = 3;

/// Icon used for engine shortcuts in the applications menu.
const SHORTCUT_ICON: &[u8] = include_bytes!("../assets/icon-256.png");

/// An error ready to show: what happened and what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError {
    pub summary: String,
    pub hint: Option<String>,
}

impl UiError {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            hint: None,
        }
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl<E: UserFacing> From<E> for UiError {
    fn from(error: E) -> Self {
        Self {
            summary: error.summary(),
            hint: error.hint(),
        }
    }
}

/// Where the backend reads and writes.
#[derive(Debug, Clone)]
pub struct Env {
    pub install_ini: PathBuf,
    /// UnLauncher's record of how each engine was installed.
    pub metadata: PathBuf,
    /// Persistent state of the task queue.
    pub tasks_state: PathBuf,
    /// Build logs.
    pub logs_dir: PathBuf,
    /// Downloads (Fab chunks, engine zips) that can be resumed.
    pub cache_dir: PathBuf,
    /// Cookies and cache of the built-in web window (`unlauncher-web`).
    pub webview_dir: PathBuf,
    /// Cached Fab thumbnails (can be downloaded again).
    pub thumbnails_dir: PathBuf,
    /// The user's downloads folder, watched when the engine is downloaded in
    /// the browser.
    pub downloads_dir: PathBuf,
    pub home: PathBuf,
    pub applications_dir: PathBuf,
    pub sandbox: Sandbox,
}

/// Everything the Engines and Projects screens show.
#[derive(Debug)]
pub struct Snapshot {
    pub registry: Result<EngineRegistry, UiError>,
    pub unregistered: Vec<DiscoveredEngine>,
    pub projects: Vec<Result<Project, UiError>>,
    /// The picture of each project that has one (found here, off the
    /// drawing thread), by the project's `.uproject` path.
    pub thumbnails: HashMap<PathBuf, PathBuf>,
}

/// Looks for the picture of every project (`ue_core::project_thumbnail`).
fn project_thumbnails(projects: &[Result<Project, UiError>]) -> HashMap<PathBuf, PathBuf> {
    projects
        .iter()
        .filter_map(|project| project.as_ref().ok())
        .filter_map(|project| {
            ue_core::project_thumbnail(project).map(|image| (project.path.clone(), image))
        })
        .collect()
}

impl Snapshot {
    /// Registered, valid engines (empty if `Install.ini` could not be read).
    pub fn engines(&self) -> &[Engine] {
        self.registry.as_ref().map_or(&[], EngineRegistry::engines)
    }

    /// The engine a project resolves to.
    pub fn engine_for(&self, project: &Project) -> Option<&Engine> {
        self.registry
            .as_ref()
            .ok()
            .and_then(|registry| resolve_engine(project, registry))
    }
}

/// Loads the registry, scans for unregistered engines and finds projects in
/// the default folders plus `project_folders`.
pub fn load_snapshot(env: &Env, project_folders: &[PathBuf]) -> Snapshot {
    let registry = EngineRegistry::load(&env.install_ini)
        .map(|mut registry| {
            match EngineMetadata::load(&env.metadata) {
                Ok(metadata) => registry.apply_metadata(&metadata),
                Err(error) => tracing::warn!(%error, "ignoring engine metadata"),
            }
            registry
        })
        .map_err(UiError::from);

    let engine_paths = system::paths::default_engine_search_paths(&env.home);
    let unregistered = discover_engines(&engine_paths, ENGINE_SEARCH_DEPTH)
        .into_iter()
        .filter(|found| {
            registry.as_ref().map_or(true, |registry| {
                let registered = registry
                    .engines()
                    .iter()
                    .map(|e| &e.path)
                    .chain(registry.invalid_entries().iter().map(|e| &e.path));
                !registered
                    .into_iter()
                    .any(|path| same_dir(path, &found.path))
            })
        })
        .collect();

    let mut project_paths = system::paths::default_project_search_paths(&env.home);
    project_paths.extend(project_folders.iter().cloned());
    let projects: Vec<Result<Project, UiError>> =
        discover_projects(&project_paths, PROJECT_SEARCH_DEPTH)
            .into_iter()
            .map(|result| result.map_err(UiError::from))
            .collect();

    let thumbnails = project_thumbnails(&projects);
    Snapshot {
        registry,
        unregistered,
        projects,
        thumbnails,
    }
}

/// Registers the engines at `paths` with default ids, in one write.
pub fn register_engines(env: &Env, paths: &[PathBuf]) -> Result<String, UiError> {
    let mut registry = EngineRegistry::load(&env.install_ini)?;
    let mut ids = Vec::new();
    for path in paths {
        ids.push(registry.register(path, None)?.id.to_string());
    }
    registry.save(Overwrite::Yes)?;
    Ok(format!("Registered {}", ids.join(", ")))
}

/// Removes an engine from `Install.ini`. Its files are not touched.
pub fn unregister_engine(env: &Env, id: &EngineId) -> Result<String, UiError> {
    let mut registry = EngineRegistry::load(&env.install_ini)?;
    let path = registry.unregister(id)?;
    registry.save(Overwrite::Yes)?;
    let mut metadata = EngineMetadata::load(&env.metadata)?;
    metadata.remove(&path);
    metadata.save()?;
    Ok(format!("Unregistered {id}; its files were not touched"))
}

/// Makes the project at `project` use the engine `engine_id`.
pub fn associate(env: &Env, project: &Path, engine_id: &EngineId) -> Result<String, UiError> {
    let registry = EngineRegistry::load(&env.install_ini)?;
    let engine = registry.find(engine_id.as_str()).ok_or_else(|| {
        UiError::new(format!("Engine {engine_id} is no longer registered"))
            .with_hint("Refresh the Engines screen.")
    })?;
    let mut project = Project::load(project)?;
    project.set_association(EngineAssociation::for_engine(engine));
    project.save(Overwrite::Yes)?;
    Ok(format!("{} now uses {}", project.name, engine.id))
}

/// What the editor was started with, to start it again after a crash.
#[derive(Debug, Clone)]
pub enum EditorTarget {
    Project(PathBuf),
    Engine(EngineId),
}

/// The editor closed with an error.
#[derive(Debug, Clone)]
pub struct EditorCrash {
    pub target: EditorTarget,
    /// What was open, e.g. `Kart (UE_5.8)`.
    pub label: String,
    /// How it ended, e.g. "exited with code 1".
    pub exit: String,
    /// The editor's log for that session.
    pub log: PathBuf,
    /// Why, if the log tells.
    pub cause: Option<ue_core::CrashCause>,
}

/// The command that starts `engine`'s editor, with `project` if given.
fn editor_command(engine: &Engine, project: Option<&Path>) -> Result<HostCommand, UiError> {
    let mut command = HostCommand::new(engine.editor_path()?);
    if let Some(project) = project {
        command = command.arg(project);
    }
    Ok(command)
}

/// Starts the editor from the interface.
///
/// It goes through a background copy of UnLauncher (`unlauncher <project>`
/// or `unlauncher --launch-engine <id>`, without a window) that waits for the
/// editor and opens UnLauncher if it crashes, even after this window is
/// closed. If that copy cannot be found, the editor starts directly.
pub fn launch_editor(
    env: &Env,
    engine: &Engine,
    project: Option<&Path>,
) -> Result<String, UiError> {
    match HostCommand::bundled("unlauncher") {
        Ok(watcher) => {
            let watcher = match project {
                Some(project) => watcher.arg(project),
                None => watcher.args(["--launch-engine", engine.id.as_str()]),
            };
            watcher.spawn_detached_in(env.sandbox)?;
        }
        Err(error) => {
            tracing::warn!(%error, "cannot watch the editor; starting it directly");
            editor_command(engine, project)?.spawn_detached_in(env.sandbox)?;
        }
    }
    Ok(match project {
        Some(project) => format!("Opening {} with {}", project.display(), engine.id),
        None => format!("Starting the {} editor", engine.id),
    })
}

/// Runs the editor and waits for it. `None` if it closed normally, or what
/// happened if it crashed (used without a window: a double click on a
/// `.uproject`, a menu shortcut, or [`launch_editor`]'s background copy).
pub fn run_editor(
    env: &Env,
    engine: &Engine,
    project: Option<&Path>,
) -> Result<Option<EditorCrash>, UiError> {
    let status = editor_command(engine, project)?.wait_quiet_in(env.sandbox)?;
    if status.success() {
        return Ok(None);
    }
    let (target, label, log) = match project {
        Some(path) => (
            EditorTarget::Project(path.to_path_buf()),
            format!(
                "{} ({})",
                path.file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                engine.id
            ),
            ue_core::project_log_path(path),
        ),
        None => (
            EditorTarget::Engine(engine.id.clone()),
            format!("Unreal Editor {} ({})", engine.version, engine.id),
            ue_core::engine_log_path(
                &system::paths::unreal_config_dir_in(&env.home),
                engine.version,
            ),
        ),
    };
    let cause = ue_core::read_crash_cause(&log);
    tracing::warn!(%label, ?status, ?cause, "the editor crashed");
    Ok(Some(EditorCrash {
        target,
        label,
        exit: describe_exit(status),
        log,
        cause,
    }))
}

/// "exited with code 1" or "was stopped by signal 11".
fn describe_exit(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited with code {code}"),
        (None, Some(signal)) => format!("was stopped by signal {signal}"),
        (None, None) => "ended unexpectedly".to_owned(),
    }
}

/// Opens a project with the engine it is associated with.
///
/// Fails with a hint to change the engine if no registered engine matches.
pub fn open_project(env: &Env, path: &Path) -> Result<String, UiError> {
    let registry = EngineRegistry::load(&env.install_ini)?;
    let project = Project::load(path)?;
    let engine = resolve_engine(&project, &registry).ok_or_else(|| missing_engine(&project))?;
    launch_editor(env, engine, Some(path))
}

/// Opens a project with its engine and waits for the editor (see
/// [`run_editor`]).
pub fn watch_project(env: &Env, path: &Path) -> Result<Option<EditorCrash>, UiError> {
    let registry = EngineRegistry::load(&env.install_ini)?;
    let project = Project::load(path)?;
    let engine = resolve_engine(&project, &registry).ok_or_else(|| missing_engine(&project))?;
    run_editor(env, engine, Some(path))
}

/// The error shown when a project's engine is not installed.
pub fn missing_engine(project: &Project) -> UiError {
    let wanted = match &project.association {
        EngineAssociation::None => "no engine".to_owned(),
        other => format!("engine {other}"),
    };
    UiError::new(format!(
        "{} uses {wanted}, which is not installed",
        project.name
    ))
    .with_hint("Install that engine, or use “Change engine…” to open it with another one.")
}

/// A registered engine by id.
fn engine_by_id(env: &Env, id: &str) -> Result<Engine, UiError> {
    let registry = EngineRegistry::load(&env.install_ini)?;
    registry.find(id).cloned().ok_or_else(|| {
        UiError::new(format!("No engine is registered as {id}"))
            .with_hint("The engine may have been unregistered; remove the old menu entry.")
    })
}

/// Starts the editor of a registered engine by id and waits for it (used by
/// menu shortcuts; see [`run_editor`]).
pub fn watch_engine_by_id(env: &Env, id: &str) -> Result<Option<EditorCrash>, UiError> {
    let engine = engine_by_id(env, id)?;
    run_editor(env, &engine, None)
}

/// Starts the editor again after a crash.
pub fn relaunch(env: &Env, target: &EditorTarget) -> Result<String, UiError> {
    match target {
        EditorTarget::Project(path) => open_project(env, path),
        EditorTarget::Engine(id) => {
            let engine = engine_by_id(env, id.as_str())?;
            launch_editor(env, &engine, None)
        }
    }
}

/// Opens a file (such as an editor log) with the host's default app.
pub fn open_file(env: &Env, path: &Path) -> Result<String, UiError> {
    HostCommand::new("xdg-open")
        .arg(path)
        .spawn_detached_in(env.sandbox)?;
    Ok(format!("Opening {}", path.display()))
}

/// Adds a menu entry that starts `engine`'s editor through UnLauncher.
pub fn add_engine_shortcut(env: &Env, engine: &Engine) -> Result<String, UiError> {
    let launcher = match env.sandbox {
        // Inside Flatpak the portal makes the entry run inside the sandbox.
        Sandbox::Flatpak => PathBuf::from("unlauncher"),
        Sandbox::Host => std::env::current_exe().map_err(|error| {
            UiError::new(format!("Cannot find the UnLauncher executable: {error}"))
        })?,
    };
    let shortcut = DesktopShortcut {
        id: engine.id.as_str().to_owned(),
        name: format!("Unreal Editor {}", engine.version),
        comment: Some(format!("Unreal Engine {} ({})", engine.version, engine.id)),
        exec: vec![
            launcher.to_string_lossy().into_owned(),
            "--launch-engine".to_owned(),
            engine.id.as_str().to_owned(),
        ],
        icon_png: SHORTCUT_ICON.to_vec(),
    };
    install_shortcut(
        env.sandbox,
        &shortcut,
        &env.applications_dir,
        Overwrite::Yes,
    )?;
    Ok(format!(
        "Added “{}” to the applications menu",
        shortcut.name
    ))
}

/// The paths tasks record in their specs.
pub fn task_paths(env: &Env) -> crate::tasks::TaskPaths {
    crate::tasks::TaskPaths {
        install_ini: env.install_ini.clone(),
        metadata: env.metadata.clone(),
    }
}

/// Lists the project templates of an engine.
pub fn templates(engine: &Engine) -> Result<Vec<ProjectTemplate>, UiError> {
    Ok(ue_core::list_templates(&engine.path)?)
}

/// Creates a project from a template and returns its `.uproject` path.
pub fn create_project(
    engine: &Engine,
    template: &ProjectTemplate,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, UiError> {
    Ok(ue_core::create_project(template, engine, parent, name)?.path)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Disk usage of an engine and its caches.
#[derive(Debug, Clone)]
pub struct Usage {
    pub total: u64,
    pub caches: Vec<ue_core::Cache>,
}

/// Measures an engine folder and its caches (slow on big engines).
pub fn engine_usage(env: &Env, engine: &Engine) -> Usage {
    Usage {
        total: ue_core::dir_size(&engine.path),
        caches: ue_core::caches(
            &engine.path,
            &system::paths::unreal_config_dir_in(&env.home),
        ),
    }
}

/// Empties a cache folder.
pub fn clean_cache(path: &Path) -> Result<String, UiError> {
    let freed = ue_core::clean_cache(path, common::Delete::Yes)?;
    Ok(format!(
        "Freed {} from {}",
        format_bytes(freed),
        path.display()
    ))
}

/// Deletes a project's folder.
pub fn delete_project(uproject: &Path) -> Result<String, UiError> {
    let folder = ue_core::delete_project(uproject, common::Delete::Yes)?;
    Ok(format!("Deleted {}", folder.display()))
}

/// Unregisters an engine and deletes its folder.
pub fn uninstall(env: &Env, id: &EngineId) -> Result<String, UiError> {
    let mut registry = EngineRegistry::load(&env.install_ini)?;
    let mut metadata = EngineMetadata::load(&env.metadata)?;
    let path = ue_core::uninstall_engine(&mut registry, &mut metadata, id, common::Delete::Yes)?;
    Ok(format!("Uninstalled {id} ({})", path.display()))
}

/// Human-readable size, e.g. `1.2 GB`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    #[allow(clippy::cast_precision_loss, reason = "display only")]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_pictures_of_the_projects_that_have_one() {
        let root = tempfile::tempdir().unwrap();
        let mut projects = Vec::new();
        for name in ["WithPicture", "Plain"] {
            let dir = root.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let uproject = dir.join(format!("{name}.uproject"));
            std::fs::write(&uproject, "{}").unwrap();
            projects.push(Ok(Project::load(&uproject).unwrap()));
        }
        projects.push(Err(UiError::new("broken")));
        let picture = root.path().join("WithPicture/WithPicture.png");
        std::fs::write(&picture, b"png").unwrap();

        let found = project_thumbnails(&projects);

        assert_eq!(found.len(), 1);
        assert_eq!(
            found.get(&root.path().join("WithPicture/WithPicture.uproject")),
            Some(&picture)
        );
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_200_000_000), "1.2 GB");
    }

    /// A fake engine whose editor runs `script`, and an `Env` in `root`.
    fn fake(root: &Path, script: &str) -> (Env, Engine) {
        use std::os::unix::fs::PermissionsExt;

        let engine_root = root.join("UE_5.8");
        let bin = engine_root.join("Engine/Binaries/Linux");
        std::fs::create_dir_all(&bin).unwrap();
        let editor = bin.join("UnrealEditor");
        std::fs::write(&editor, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = Env {
            install_ini: root.join("Install.ini"),
            metadata: root.join("engines.toml"),
            tasks_state: root.join("tasks.json"),
            logs_dir: root.join("logs"),
            thumbnails_dir: root.join("thumbnails"),
            cache_dir: root.join("cache"),
            webview_dir: root.join("webview"),
            downloads_dir: root.join("Downloads"),
            home: root.to_path_buf(),
            applications_dir: root.join("applications"),
            sandbox: Sandbox::Host,
        };
        let engine = Engine {
            id: EngineId::new("UE_5.8").unwrap(),
            version: ue_core::EngineVersion::new(5, 8, 3),
            path: engine_root,
            kind: ue_core::EngineKind::Prebuilt,
        };
        (env, engine)
    }

    /// Runs `run_editor`, retrying while the fake editor is "busy": another
    /// test thread may fork while it is being written (rust-lang/rust#114554).
    fn run(env: &Env, engine: &Engine, project: Option<&Path>) -> Option<EditorCrash> {
        for _ in 0..50 {
            match run_editor(env, engine, project) {
                Err(error) if error.summary.starts_with("failed to start") => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                result => return result.unwrap(),
            }
        }
        run_editor(env, engine, project).unwrap()
    }

    #[test]
    fn a_normal_exit_is_not_a_crash() {
        let root = tempfile::tempdir().unwrap();
        let (env, engine) = fake(root.path(), "exit 0");
        assert!(run(&env, &engine, None).is_none());
    }

    #[test]
    fn reports_a_crash_with_its_cause() {
        let root = tempfile::tempdir().unwrap();
        // Like Unreal: the project path is the first argument and the log
        // goes to <project>/Saved/Logs/<name>.log.
        let (env, engine) = fake(
            root.path(),
            r#"dir=$(dirname "$1"); mkdir -p "$dir/Saved/Logs"
echo 'LogVulkanRHI: Error: vkQueueSubmit failed with error VK_ERROR_DEVICE_LOST' > "$dir/Saved/Logs/Kart.log"
exit 1"#,
        );
        let project = root.path().join("Kart/Kart.uproject");
        std::fs::create_dir_all(project.parent().unwrap()).unwrap();
        let crash = run(&env, &engine, Some(&project)).unwrap();
        assert_eq!(crash.label, "Kart (UE_5.8)");
        assert_eq!(crash.exit, "exited with code 1");
        assert_eq!(crash.log, root.path().join("Kart/Saved/Logs/Kart.log"));
        assert_eq!(crash.cause.unwrap().kind, ue_core::CrashKind::GpuLost);
        assert!(matches!(crash.target, EditorTarget::Project(p) if p == project));
    }

    #[test]
    fn an_engine_crash_points_to_the_engine_log() {
        let root = tempfile::tempdir().unwrap();
        let (env, engine) = fake(root.path(), "kill -SEGV $$");
        let crash = run(&env, &engine, None).unwrap();
        assert_eq!(crash.exit, "was stopped by signal 11");
        assert_eq!(
            crash.log,
            root.path()
                .join(".config/Epic/UnrealEngine/5.8/Saved/Logs/Unreal.log")
        );
        assert!(crash.cause.is_none());
    }
}
