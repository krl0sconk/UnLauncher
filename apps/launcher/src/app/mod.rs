// SPDX-License-Identifier: GPL-3.0-or-later

//! Application state. Drawing code returns [`Intent`]s; this module turns
//! them into work on background threads, so the UI thread never does I/O.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use jobs::TaskManager;
use ue_core::{DiscoveredEngine, Engine, EngineId, ProjectTemplate};

use crate::backend::{self, Env, Snapshot, UiError};
use crate::images::Images;
use crate::settings::Settings;
use crate::text;
use crate::ui;

mod epic;
mod github;
mod handle;
mod install;

pub use github::{GhLogin, GithubState};

mod demo;
mod fab_window;
pub use fab_window::FabWindow;

/// The screen shown in the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Screen {
    /// Engines and projects.
    Library,
    /// The Fab library.
    Fab,
    /// Downloads, imports and builds.
    Tasks,
    /// Theme, folders, accounts and About.
    Settings,
}

/// The Epic session, as far as the interface knows.
pub enum EpicState {
    Restoring,
    SignedOut,
    SigningIn,
    SignedIn(epic_auth::EpicSession),
    /// Only with the hidden `--demo` flag: a made-up signed-in user, to take
    /// screenshots and measure the interface without a real account.
    Demo(String),
}

impl EpicState {
    /// Whether the Fab library can be shown.
    pub fn is_signed_in(&self) -> bool {
        matches!(self, Self::SignedIn(_) | Self::Demo(_))
    }

    /// The name of the signed-in account.
    pub fn display_name(&self) -> Option<&str> {
        match self {
            Self::SignedIn(session) => Some(session.display_name()),
            Self::Demo(name) => Some(name),
            Self::Restoring | Self::SignedOut | Self::SigningIn => None,
        }
    }
}

/// The built-in Epic sign-in window.
pub enum LoginWindow {
    Closed,
    Starting,
    Open(web_bridge::BridgeSender),
}

/// Where the user signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMode {
    /// The built-in window.
    Window,
    /// The system browser: a third-party login (Google, Apple…) or the user
    /// asked for it. The code comes back from the clipboard or is pasted.
    Browser,
    /// The built-in window cannot start (no WebKit): sign-in and engine
    /// downloads go through the browser.
    NoWindow,
}

impl LoginMode {
    pub fn is_browser(self) -> bool {
        self != Self::Window
    }
}

/// Which kinds of Fab assets the grid shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    /// Everything.
    All,
    /// Asset packs and complete projects.
    AssetPacks,
    /// Plugins.
    Plugins,
}

impl KindFilter {
    /// Whether an asset of `kind` is shown.
    pub fn accepts(self, kind: fab::AssetKind) -> bool {
        match self {
            Self::All => true,
            Self::AssetPacks => kind != fab::AssetKind::Plugin,
            Self::Plugins => kind == fab::AssetKind::Plugin,
        }
    }
}

/// What the Fab screen shows and edits.
pub struct LibraryState {
    pub epic: EpicState,
    pub error: Option<UiError>,
    pub login_window: LoginWindow,
    pub login_mode: LoginMode,
    /// A note above the sign-in steps (why the browser is used).
    pub notice: Option<String>,
    pub login_code: String,
    pub filter: String,
    pub kind_filter: KindFilter,
    pub assets: Option<Result<Vec<fab::LibraryAsset>, UiError>>,
    pub loading: bool,
    /// Linux engine downloads Epic offers (newest first), once listed.
    pub engine_downloads: Option<Result<Vec<ue_prebuilt::EngineDownload>, UiError>>,
    /// The web window is open to list the engine downloads.
    pub listing_downloads: bool,
    /// The web window is open on fab.com ("Get more on Fab").
    pub fab_window: FabWindow,
}

/// A request about a task in the queue.
#[derive(Debug, Clone, Copy)]
pub enum TaskOp {
    Pause,
    Resume,
    Cancel,
    Retry,
    Remove,
}

/// A change to user files, run after the user confirms it.
#[derive(Debug, Clone)]
pub enum Action {
    Register(Vec<PathBuf>),
    Update {
        engine_root: PathBuf,
        git_ref: String,
    },
    CleanCache(PathBuf),
    Uninstall(EngineId),
    DeleteProject(PathBuf),
    Unregister(EngineId),
    Associate {
        project: PathBuf,
        engine: EngineId,
    },
}

/// Something the user asked for while drawing a frame.
#[derive(Debug)]
pub enum Intent {
    Navigate(Screen),
    PickEngineFolder,
    PickProjectFolder,
    PickNewProjectLocation,
    RemoveProjectFolder(PathBuf),
    Ask {
        title: String,
        body: String,
        action: Action,
    },
    Execute(Action),
    /// Ask for confirmation by typing `expected` (for deletions).
    AskTyped {
        title: String,
        body: String,
        expected: String,
        action: Action,
    },
    /// Open a web page in the system browser.
    OpenUrl(String),
    LaunchEngine(Engine),
    AddShortcut(Engine),
    OpenProject(PathBuf),
    ChangeEngine {
        project: PathBuf,
        name: String,
        current: Option<EngineId>,
    },
    NewProject,
    LoadTemplates(Engine),
    CreateProject {
        engine: Engine,
        template: Box<ProjectTemplate>,
        parent: PathBuf,
        name: String,
    },
    FinishFirstRun(Vec<PathBuf>),
    Maintenance(Engine),
    InstallEngine,
    PickZip,
    PickInstallDestination,
    StartImport {
        zip: PathBuf,
        destination: PathBuf,
    },
    SetInstallMode(InstallMode),
    PickSourceDestination,
    SaveGitHubToken(String),
    /// Check the saved GitHub token (and the steps after it) again.
    GithubRefresh,
    /// Start the Device Flow.
    GithubConnect,
    GithubCancelDevice,
    /// Sign in with the host's GitHub CLI, guided.
    GithubGhLogin,
    /// Use the existing credential at this index.
    GithubUseExisting(usize),
    /// Forget the saved GitHub token.
    GithubDisconnect,
    /// Open the Epic account's connections page to link GitHub.
    OpenEpicConnections,
    /// Open the EpicGames organization's invitation on GitHub.
    OpenGithubInvitation,
    RunPreflight,
    StartBuild,
    Task(jobs::TaskId, TaskOp),
    /// Remove every finished task from the list.
    ClearFinished,
    /// Show why a task failed.
    TaskDetails {
        title: String,
        error: UiError,
    },
    OpenLogsFolder,
    SetTheme(crate::settings::ThemeChoice),
    /// Hide the new-version banner until the next start.
    DismissNewVersion,
    /// Open fab.com in the web window; reload the library when it closes.
    GetMoreOnFab,
    OpenLoginPage,
    OpenEngineDownloadPage,
    /// Sign in with the built-in web window.
    SignInWithWindow,
    CancelWindowLogin,
    /// Switch to the system browser and the pasted code.
    UseBrowserLogin,
    /// Switch back to the built-in window.
    UseWindowLogin,
    SignIn,
    SignOut,
    RefreshLibrary,
    InstallAsset(fab::LibraryAsset),
    StartFabInstall,
    ListEngineDownloads,
    StartEngineDownload(String),
    /// Open the Install engine dialog and list Epic's downloads.
    DownloadNewEngine,
    /// Open the Install engine dialog with this zip chosen.
    ChooseZip(PathBuf),
    /// Start the editor again after a crash.
    Relaunch(backend::EditorTarget),
    /// Open a file (an editor log) with the default app.
    OpenFile(PathBuf),
    CloseDialog,
}

/// What a folder picker was opened for.
#[derive(Debug, Clone, Copy)]
enum Pick {
    Engine,
    ProjectFolder,
    NewProjectLocation,
    Zip,
    InstallDestination,
    SourceDestination,
}

/// Progress of the built-in sign-in window.
enum WebLogin {
    Opened(web_bridge::BridgeSender),
    /// The user chose a third-party login; it continues in the browser.
    ThirdParty(web_bridge::Provider),
    CodeReceived,
    Unavailable(UiError),
    Closed,
}

/// Results coming back from worker threads.
enum Event {
    Snapshot(Snapshot),
    Done {
        result: Result<String, UiError>,
        reload: bool,
    },
    Picked(Pick, Option<PathBuf>),
    Templates(EngineId, Result<Vec<ProjectTemplate>, UiError>),
    Created(Result<PathBuf, UiError>),
    Github(github::GithubEvent),
    Usage(EngineId, backend::Usage),
    Session(Result<Option<epic_auth::EpicSession>, UiError>),
    WebLogin(WebLogin),
    Library(Result<Vec<fab::LibraryAsset>, UiError>),
    EngineDownloads(Result<Vec<ue_prebuilt::EngineDownload>, UiError>),
    /// The web window could not start to list the downloads.
    DownloadsWindowUnavailable(UiError),
    /// The downloads folder watch ended: an engine zip, or nothing in time.
    EngineZipFound(Option<ue_prebuilt::FoundEngineZip>),
    /// Fab's list of engine versions, read at startup.
    FabVersions(Result<Vec<String>, UiError>),
    Preflight(Vec<ue_build::CheckResult>),
    /// What the fab.com window is doing.
    FabWindow(fab_window::FabWindowEvent),
}

/// A modal dialog.
pub enum Dialog {
    FirstRun(Vec<(DiscoveredEngine, bool)>),
    Confirm {
        title: String,
        body: String,
        action: Action,
    },
    Error(UiError),
    /// Why a task failed.
    TaskDetails {
        title: String,
        error: UiError,
        /// Offer to open the folder with the build logs.
        has_logs: bool,
    },
    ChangeEngine {
        project: PathBuf,
        name: String,
        selected: Option<EngineId>,
    },
    NewProject(NewProjectForm),
    Created(PathBuf),
    InstallEngine(InstallForm),
    Maintenance(MaintenanceForm),
    FabInstall(FabForm),
    /// The editor crashed (shown by the background copy that watched it).
    EditorCrashed(backend::EditorCrash),
    /// An engine zip finished downloading in the browser.
    ZipFound {
        found: ue_prebuilt::FoundEngineZip,
        destination: PathBuf,
    },
    TypedConfirm {
        title: String,
        body: String,
        expected: String,
        typed: String,
        action: Action,
    },
}

/// State of the Install asset dialog.
pub struct FabForm {
    pub asset: fab::LibraryAsset,
    pub target: Option<crate::epic_tasks::Target>,
    /// Folder for a new project (complete-project assets).
    pub location: String,
}

/// State of an engine's Maintenance dialog.
pub struct MaintenanceForm {
    pub engine: Engine,
    pub usage: Option<backend::Usage>,
}

/// How to install an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Prebuilt,
    Source,
}

/// State of the Install engine dialog.
pub struct InstallForm {
    pub mode: InstallMode,
    pub zip: Option<PathBuf>,
    pub destination: PathBuf,
    pub git_ref: String,
    pub source_destination: PathBuf,
    pub jobs: String,
    /// The personal token typed under Advanced options.
    pub token_input: String,
    pub preflight: Option<Vec<ue_build::CheckResult>>,
    pub checking: bool,
}

/// State of the New project dialog.
pub struct NewProjectForm {
    pub engine: Option<Engine>,
    pub templates: Option<Result<Vec<ProjectTemplate>, UiError>>,
    pub template: Option<usize>,
    pub name: String,
    pub location: PathBuf,
    pub creating: bool,
    pub error: Option<UiError>,
}

pub struct LauncherApp {
    env: Env,
    runtime: tokio::runtime::Runtime,
    http: reqwest::Client,
    pub tasks: TaskManager,
    pub images: Images,
    settings: Settings,
    settings_path: PathBuf,
    ctx: egui::Context,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    pub screen: Screen,
    pub snapshot: Option<Snapshot>,
    pub loading: bool,
    pub busy: usize,
    pub status: Option<Result<String, UiError>>,
    pub dialog: Option<Dialog>,
    pub library: LibraryState,
    pub github: GithubState,
    /// The downloads folder is being watched for an engine zip.
    watching_downloads: bool,
    /// An engine zip found while another dialog was open.
    found_zip: Option<ue_prebuilt::FoundEngineZip>,
    /// Reads the authorization code the user copied in the browser.
    clipboard: egui_winit::clipboard::Clipboard,
    /// The last code taken from the clipboard, so a rejected one is not
    /// tried again on every focus change.
    clipboard_code: Option<common::Secret>,
    /// Finished tasks already seen, to reload the lists when a new one ends.
    /// `None` until the first frame, so tasks finished before startup do not
    /// trigger a reload.
    seen_done: Option<std::collections::HashSet<jobs::TaskId>>,
    /// The new-version banner was closed with "Later".
    new_version_dismissed: bool,
}

impl LauncherApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        env: Env,
        settings_path: PathBuf,
        startup: Option<Dialog>,
    ) -> anyhow::Result<Self> {
        let (tx, rx) = channel();
        let settings = Settings::load(&settings_path);
        crate::theme::apply(&cc.egui_ctx, settings.theme);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("unlauncher-worker")
            .build()?;
        let ctx = cc.egui_ctx.clone();
        let tasks = {
            let _guard = runtime.enter();
            TaskManager::new(
                env.tasks_state.clone(),
                Arc::new(move || ctx.request_repaint()),
            )?
        };
        crate::tasks::register_factories(&tasks);
        crate::epic_tasks::register_factories(&tasks);
        let http = epic_auth::http_client()?;
        let images = Images::new(
            &cc.egui_ctx,
            runtime.handle().clone(),
            http.clone(),
            env.thumbnails_dir.clone(),
        );
        let mut app = Self {
            env,
            runtime,
            http,
            tasks,
            images,
            settings,
            settings_path,
            ctx: cc.egui_ctx.clone(),
            tx,
            rx,
            screen: Screen::Library,
            snapshot: None,
            loading: false,
            busy: 0,
            status: None,
            dialog: startup,
            seen_done: None,
            new_version_dismissed: false,
            github: GithubState::default(),
            watching_downloads: false,
            found_zip: None,
            clipboard: {
                use egui_winit::winit::raw_window_handle::HasDisplayHandle;
                egui_winit::clipboard::Clipboard::new(cc.display_handle().ok().map(|h| h.as_raw()))
            },
            clipboard_code: None,
            library: LibraryState {
                epic: EpicState::Restoring,
                error: None,
                login_window: LoginWindow::Closed,
                login_mode: LoginMode::Window,
                notice: None,
                login_code: String::new(),
                filter: String::new(),
                kind_filter: KindFilter::All,
                assets: None,
                loading: false,
                engine_downloads: None,
                listing_downloads: false,
                fab_window: FabWindow::Closed,
            },
        };
        app.spawn_async(async {
            Event::Session(
                epic_auth::EpicSession::restore(Arc::new(epic_auth::KeyringStore))
                    .await
                    .map_err(UiError::from),
            )
        });
        app.reload();
        app.check_for_new_engine();
        Ok(app)
    }

    /// Reads Fab's list of engine versions in the background, at most once a
    /// day. A failure only goes to the log.
    fn check_for_new_engine(&self) {
        if !self.settings.update_check_due(unix_now()) {
            return;
        }
        let webview_dir = self.env.webview_dir.clone();
        self.spawn_async(async move { Event::FabVersions(fab_versions(&webview_dir).await) });
    }

    /// A newer engine than every installed one, as `major.minor`, if Fab
    /// lists one.
    pub fn new_engine_version(&self) -> Option<String> {
        let installed: Vec<_> = self
            .snapshot
            .as_ref()?
            .engines()
            .iter()
            .map(|e| e.version)
            .collect();
        ue_prebuilt::newer_than_installed(&self.settings.fab_versions, &installed)
    }

    /// The version banner to show: a newer engine than every installed one,
    /// unless the user closed it with "Later".
    pub fn new_version_banner(&self) -> Option<String> {
        if self.new_version_dismissed {
            None
        } else {
            self.new_engine_version()
        }
    }

    /// The engine whose Launch button is amber: the one launched last, or
    /// the newest installed.
    pub fn selected_engine(&self) -> Option<EngineId> {
        let engines = self.snapshot.as_ref()?.engines();
        let last = self.settings.last_engine.as_deref();
        engines
            .iter()
            .find(|engine| last == Some(engine.id.as_str()))
            .or_else(|| engines.iter().max_by_key(|engine| engine.version))
            .map(|engine| engine.id.clone())
    }

    /// Which theme the user chose.
    pub fn theme(&self) -> crate::settings::ThemeChoice {
        self.settings.theme
    }

    /// Records the engine used last.
    fn remember_engine(&mut self, id: &EngineId) {
        if self.settings.last_engine.as_deref() != Some(id.as_str()) {
            self.settings.last_engine = Some(id.as_str().to_owned());
            self.save_settings();
        }
    }

    pub fn is_flatpak(&self) -> bool {
        self.env.sandbox.is_flatpak()
    }

    /// Reloads engines and projects when a task finishes successfully, since
    /// it may have installed an engine or created or changed a project.
    fn reload_after_tasks(&mut self) {
        let done: Vec<_> = self
            .tasks
            .list()
            .into_iter()
            .filter(|task| task.state == jobs::TaskState::Done)
            .map(|task| task.id)
            .collect();
        match &mut self.seen_done {
            None => self.seen_done = Some(done.into_iter().collect()),
            Some(seen) => {
                let mut changed = false;
                for id in done {
                    changed |= seen.insert(id);
                }
                if changed {
                    self.reload();
                }
            }
        }
    }

    pub fn home(&self) -> &std::path::Path {
        &self.env.home
    }

    pub fn project_folders(&self) -> &[PathBuf] {
        &self.settings.project_folders
    }

    /// Runs `work` on a worker thread and delivers its event to the UI.
    fn spawn(&self, work: impl FnOnce() -> Event + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            // The receiver only disappears when the app is closing.
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    /// Runs an async job on the runtime and delivers its event to the UI.
    fn spawn_async(&self, work: impl std::future::Future<Output = Event> + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(work.await);
            ctx.request_repaint();
        });
    }

    fn reload(&mut self) {
        self.loading = true;
        let env = self.env.clone();
        let folders = self.settings.project_folders.clone();
        self.spawn(move || Event::Snapshot(backend::load_snapshot(&env, &folders)));
    }

    fn run(
        &mut self,
        reload: bool,
        work: impl FnOnce(&Env) -> Result<String, UiError> + Send + 'static,
    ) {
        self.busy += 1;
        let env = self.env.clone();
        self.spawn(move || Event::Done {
            result: work(&env),
            reload,
        });
    }

    fn pick_folder(&self, pick: Pick, title: &'static str) {
        self.spawn(move || {
            let folder = rfd::FileDialog::new().set_title(title).pick_folder();
            Event::Picked(pick, folder)
        });
    }

    fn pick_zip(&self) {
        self.spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Choose the Unreal Engine zip")
                .add_filter("Zip archive", &["zip"])
                .pick_file();
            Event::Picked(Pick::Zip, file)
        });
    }
}

/// Reads Fab's list of engine versions in a hidden web window (fab.com
/// answers only browsers).
async fn fab_versions(webview_dir: &std::path::Path) -> Result<Vec<String>, UiError> {
    let mut window = web_bridge::WebBridge::spawn(webview_dir).await?;
    window.load(ue_prebuilt::FAB_HOME_URL)?;
    let body = window
        .fetch_json(
            ue_prebuilt::FAB_VERSIONS_PATH,
            "www.fab.com",
            std::time::Duration::from_mins(1),
        )
        .await;
    window.close().await;
    Ok(ue_prebuilt::fab_versions_from_json(&body?)?)
}

/// Seconds since the Unix epoch.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl eframe::App for LauncherApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(error) = self.runtime.block_on(self.tasks.flush()) {
            tracing::warn!(%error, "cannot save the task list");
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.images.begin_frame(ui.ctx());
        self.receive();
        self.offer_found_zip();
        let focused = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(true)))
        });
        if focused {
            self.code_from_clipboard();
        }
        self.reload_after_tasks();
        let mut intents = Vec::new();
        ui::draw(self, ui, &mut intents);
        for intent in intents {
            self.handle(intent);
        }
    }
}
