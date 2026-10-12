// SPDX-License-Identifier: GPL-3.0-or-later

//! Epic session, Fab library and engine downloads.

#[allow(clippy::wildcard_imports, reason = "a part of the app module")]
use super::*;

impl LauncherApp {
    pub(super) fn session_changed(
        &mut self,
        result: Result<Option<epic_auth::EpicSession>, UiError>,
    ) {
        // The demo has its own made-up account.
        if matches!(self.library.epic, EpicState::Demo(_)) {
            return;
        }
        match result {
            Ok(Some(session)) => {
                self.library.error = None;
                self.library.epic = EpicState::SignedIn(session);
                self.library.notice = None;
                self.handle_epic(Intent::RefreshLibrary);
            }
            Ok(None) => self.library.epic = EpicState::SignedOut,
            Err(error) => {
                self.library.epic = EpicState::SignedOut;
                self.library.error = Some(error);
            }
        }
    }

    pub(super) fn handle_epic(&mut self, intent: Intent) {
        match intent {
            Intent::OpenEngineDownloadPage => self.open_engine_download_page(),
            intent @ (Intent::OpenLoginPage
            | Intent::SignInWithWindow
            | Intent::CancelWindowLogin
            | Intent::UseBrowserLogin
            | Intent::UseWindowLogin
            | Intent::SignIn
            | Intent::SignOut) => self.handle_login(&intent),
            Intent::RefreshLibrary => {
                if let EpicState::SignedIn(session) = &self.library.epic {
                    self.library.loading = true;
                    let source = fab::EpicAssetSource::new(session.clone());
                    self.spawn_async(async move {
                        use fab::AssetSource;
                        Event::Library(source.list().await.map_err(UiError::from))
                    });
                }
            }
            Intent::ListEngineDownloads => self.list_engine_downloads(),
            Intent::InstallAsset(asset) => {
                self.dialog = Some(Dialog::FabInstall(FabForm {
                    asset,
                    target: None,
                    location: self
                        .env
                        .home
                        .join("Documents/Unreal Projects")
                        .display()
                        .to_string(),
                }));
            }
            Intent::StartFabInstall => self.start_fab_install(),
            Intent::StartEngineDownload(version) => self.start_engine_download(version),
            _ => {}
        }
    }

    /// Signing in and out.
    fn handle_login(&mut self, intent: &Intent) {
        match intent {
            Intent::OpenLoginPage => self.open_login_page(epic_auth::LOGIN_URL),
            Intent::SignInWithWindow => self.start_window_login(),
            Intent::CancelWindowLogin => {
                if let LoginWindow::Open(window) = &self.library.login_window {
                    let _ = window.close();
                }
            }
            Intent::UseBrowserLogin => {
                if let LoginWindow::Open(window) = &self.library.login_window {
                    let _ = window.close();
                }
                self.use_browser();
                self.library.notice = None;
                self.open_login_page(epic_auth::LOGIN_URL);
            }
            Intent::UseWindowLogin => {
                self.library.login_mode = LoginMode::Window;
                self.library.notice = None;
                self.library.error = None;
            }
            Intent::SignIn => match epic_auth::parse_authorization_code(&self.library.login_code) {
                Ok(code) => {
                    self.library.login_code.clear();
                    self.sign_in_with(code);
                }
                Err(error) => self.library.error = Some(error.into()),
            },
            Intent::SignOut => {
                let state = std::mem::replace(&mut self.library.epic, EpicState::SignedOut);
                self.library.assets = None;
                self.library.engine_downloads = None;
                // The window's web session goes too: an open window is told to
                // clear its data, and the folder is removed.
                if let LoginWindow::Open(window) = &self.library.login_window {
                    let _ = window.send(web_bridge::Command::ClearData);
                    let _ = window.close();
                }
                let webview_dir = self.env.webview_dir.clone();
                self.spawn_async(async move {
                    let cleared =
                        tokio::task::spawn_blocking(move || web_bridge::clear_data(&webview_dir))
                            .await;
                    if let Ok(Err(error)) = cleared {
                        tracing::warn!(%error, "cannot clear the web window's data");
                    }
                    Event::WebLogin(WebLogin::Closed)
                });
                if let EpicState::SignedIn(session) = state {
                    self.spawn_async(async move {
                        Event::Session(session.logout().await.map(|()| None).map_err(UiError::from))
                    });
                }
            }
            _ => {}
        }
    }

    fn use_browser(&mut self) {
        if self.library.login_mode == LoginMode::Window {
            self.library.login_mode = LoginMode::Browser;
        }
    }

    fn open_login_page(&mut self, url: &str) {
        if let Err(error) = system::open_url(url) {
            self.library.error = Some(error.into());
        }
    }

    fn sign_in_with(&mut self, code: common::Secret) {
        self.library.error = None;
        self.library.epic = EpicState::SigningIn;
        self.spawn_async(async move {
            Event::Session(
                epic_auth::EpicSession::login(&code, Arc::new(epic_auth::KeyringStore))
                    .await
                    .map(Some)
                    .map_err(UiError::from),
            )
        });
    }

    /// When the user comes back from the browser with Epic's code copied,
    /// signs in with it: the browser cannot hand the code back by itself
    /// (Epic only redirects the launcher client to its own code page).
    pub(super) fn code_from_clipboard(&mut self) {
        if !self.library.login_mode.is_browser()
            || !matches!(self.library.epic, EpicState::SignedOut)
        {
            return;
        }
        let Some(text) = self.clipboard.get() else {
            return;
        };
        // Epic's page is a few hundred bytes; anything big is something else.
        if text.len() > 4096 {
            return;
        }
        let Ok(code) = epic_auth::parse_authorization_code(&text) else {
            return;
        };
        if self.clipboard_code.as_ref() == Some(&code) {
            return;
        }
        tracing::info!("signing in with the code copied from the browser");
        self.clipboard_code = Some(code.clone());
        self.sign_in_with(code);
    }

    /// Opens Epic's login page in the built-in window; the session is created
    /// when the window reads the authorization code.
    fn start_window_login(&mut self) {
        if !matches!(self.library.login_window, LoginWindow::Closed) {
            return;
        }
        self.library.error = None;
        self.library.login_window = LoginWindow::Starting;
        let webview_dir = self.env.webview_dir.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let report = move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        };
        self.spawn_async(window_login(webview_dir, report));
    }

    pub(super) fn web_login(&mut self, progress: WebLogin) {
        match progress {
            WebLogin::Opened(window) => self.library.login_window = LoginWindow::Open(window),
            WebLogin::ThirdParty(provider) => {
                self.library.login_window = LoginWindow::Closed;
                self.use_browser();
                self.library.notice = Some(format!(
                    "{} {provider} {}",
                    crate::text::THIRD_PARTY_PREFIX,
                    crate::text::THIRD_PARTY_IN_BROWSER
                ));
                self.open_login_page(&epic_auth::provider_login_url(provider.epic_path()));
            }
            WebLogin::CodeReceived => {
                self.library.login_window = LoginWindow::Closed;
                self.library.epic = EpicState::SigningIn;
            }
            WebLogin::Unavailable(error) => {
                tracing::warn!(error = %error.summary, "built-in sign-in window unavailable");
                self.library.login_window = LoginWindow::Closed;
                self.library.login_mode = LoginMode::NoWindow;
                self.library.notice = Some(crate::text::WEB_LOGIN_UNAVAILABLE.to_owned());
                self.library.error = Some(error);
                self.open_login_page(epic_auth::LOGIN_URL);
            }
            WebLogin::Closed => self.library.login_window = LoginWindow::Closed,
        }
    }

    pub(super) fn start_fab_install(&mut self) {
        let Some(Dialog::FabInstall(form)) = &self.dialog else {
            return;
        };
        let Some(mut target) = form.target.clone() else {
            return;
        };
        if let crate::epic_tasks::Target::NewProject { parent, .. } = &mut target {
            *parent = PathBuf::from(form.location.trim());
        }
        let engine_version = self.target_engine_version(&target);
        // Like the Epic launcher, a build made for another engine can be
        // installed; the dialog warns that it may not work.
        let Some(version) = engine_version
            .and_then(|v| fab::closest_version(&form.asset, v))
            .map(|(version, _)| version.clone())
        else {
            return;
        };
        let task = crate::epic_tasks::FabTask {
            asset: form.asset.clone(),
            version,
            target,
            cache_dir: self.env.cache_dir.join("fab"),
            paths: Some(backend::task_paths(&self.env)),
        };
        self.dialog = None;
        self.tasks.submit(Box::new(task));
        self.screen = Screen::Tasks;
    }

    /// The engine version an install target uses, if known.
    pub fn target_engine_version(
        &self,
        target: &crate::epic_tasks::Target,
    ) -> Option<ue_core::EngineVersion> {
        let snapshot = self.snapshot.as_ref()?;
        match target {
            crate::epic_tasks::Target::Engine(path)
            | crate::epic_tasks::Target::NewProject { engine: path, .. } => snapshot
                .engines()
                .iter()
                .find(|e| &e.path == path)
                .map(|e| e.version),
            crate::epic_tasks::Target::Project(path) => snapshot
                .projects
                .iter()
                .filter_map(|p| p.as_ref().ok())
                .find(|p| &p.path == path)
                .and_then(|p| snapshot.engine_for(p))
                .map(|e| e.version),
        }
    }

    /// Lists the Linux engine downloads by opening Epic's page in the web
    /// window, signed in with the current session (whichever way it started).
    /// Without WebKit, sends the user to the browser instead.
    fn list_engine_downloads(&mut self) {
        if self.library.listing_downloads {
            return;
        }
        if self.library.login_mode == LoginMode::NoWindow {
            self.open_engine_download_page();
            return;
        }
        self.library.listing_downloads = true;
        self.library.engine_downloads = None;
        let webview_dir = self.env.webview_dir.clone();
        let session = match &self.library.epic {
            EpicState::SignedIn(session) => Some(session.clone()),
            _ => None,
        };
        self.spawn_async(list_with_window(webview_dir, session));
    }

    /// Opens Epic's download page in the system browser and watches the
    /// downloads folder for the engine zip.
    fn open_engine_download_page(&mut self) {
        if let Err(error) = system::open_url(crate::text::ENGINE_DOWNLOAD_URL) {
            self.dialog = Some(Dialog::Error(error.into()));
            return;
        }
        self.watch_downloads();
    }

    fn watch_downloads(&mut self) {
        if self.watching_downloads {
            return;
        }
        self.watching_downloads = true;
        let watcher = ue_prebuilt::DownloadsWatcher::new(
            self.env.downloads_dir.clone(),
            std::time::SystemTime::now(),
        );
        tracing::info!(dir = %self.env.downloads_dir.display(), "watching downloads for an engine zip");
        self.spawn_async(async move {
            Event::EngineZipFound(
                ue_prebuilt::watch_for_engine_zip(
                    watcher,
                    DOWNLOADS_POLL_INTERVAL,
                    DOWNLOADS_WATCH_LIMIT,
                )
                .await,
            )
        });
    }

    /// Offers to install an engine zip found in the downloads folder, once
    /// no other dialog is open.
    pub(super) fn offer_found_zip(&mut self) {
        let can_show = matches!(self.dialog, None | Some(Dialog::InstallEngine(_)));
        if !can_show {
            return;
        }
        if let Some(found) = self.found_zip.take() {
            let destination = self
                .env
                .home
                .join("UnrealEngine")
                .join(format!("UE_{}", found.version));
            self.dialog = Some(Dialog::ZipFound { found, destination });
        }
    }

    pub(super) fn downloads_window_unavailable(&mut self, error: UiError) {
        tracing::warn!(error = %error.summary, "web window unavailable for engine downloads");
        self.library.listing_downloads = false;
        self.library.login_mode = LoginMode::NoWindow;
        self.library.engine_downloads = Some(Err(error));
        self.open_engine_download_page();
    }

    pub(super) fn start_engine_download(&mut self, version: String) {
        let destination = match &self.dialog {
            Some(Dialog::InstallEngine(form)) => form.destination.clone(),
            _ => self
                .env
                .home
                .join("UnrealEngine")
                .join(format!("UE_{version}")),
        };
        let download = match &self.library.engine_downloads {
            Some(Ok(downloads)) => downloads.iter().find(|d| d.version == version).cloned(),
            _ => None,
        };
        // The zip goes next to the engine, on the disk the user chose, not
        // in UnLauncher's cache (often a small home disk).
        let downloads_dir = destination
            .parent()
            .map_or_else(|| self.env.home.clone(), std::path::Path::to_path_buf);
        let task = crate::epic_tasks::DownloadEngineTask {
            version,
            destination,
            downloads_dir,
            paths: backend::task_paths(&self.env),
            download,
        };
        self.dialog = None;
        self.tasks.submit(Box::new(task));
        self.screen = Screen::Tasks;
    }
}

/// Runs the built-in sign-in window until it reads the authorization code,
/// then exchanges it. Intermediate progress goes through `report`.
async fn window_login(webview_dir: PathBuf, report: impl Fn(Event) + Send + 'static) -> Event {
    let mut window = match web_bridge::WebBridge::spawn(&webview_dir).await {
        Ok(window) => window,
        Err(error) => return Event::WebLogin(WebLogin::Unavailable(error.into())),
    };
    if window.open(epic_auth::LOGIN_URL).is_err() {
        return Event::WebLogin(WebLogin::Closed);
    }
    report(Event::WebLogin(WebLogin::Opened(window.sender())));
    let code = loop {
        match window.next_event().await {
            None | Some(web_bridge::Event::Closed) => return Event::WebLogin(WebLogin::Closed),
            Some(web_bridge::Event::AuthCode { code }) => break code,
            // Third-party logins continue in the user's browser. Epic's
            // buttons open them in a popup, which the window blocks.
            Some(
                web_bridge::Event::Navigated { url } | web_bridge::Event::PopupBlocked { url },
            ) if third_party(&url).is_some() => {
                let provider = third_party(&url);
                window.close().await;
                if let Some(provider) = provider {
                    return Event::WebLogin(WebLogin::ThirdParty(provider));
                }
                return Event::WebLogin(WebLogin::Closed);
            }
            // Other popups (privacy policy, help) open in the browser.
            Some(web_bridge::Event::PopupBlocked { url }) => {
                let opened = tokio::task::spawn_blocking(move || system::open_url(&url)).await;
                if let Ok(Err(error)) = opened {
                    tracing::warn!(%error, "cannot open a popup in the browser");
                }
            }
            Some(web_bridge::Event::Error { message }) => {
                tracing::warn!(%message, "sign-in window reported an error");
            }
            Some(_) => {}
        }
    };
    report(Event::WebLogin(WebLogin::CodeReceived));
    window.close().await;
    Event::Session(
        epic_auth::EpicSession::login(&code, Arc::new(epic_auth::KeyringStore))
            .await
            .map(Some)
            .map_err(UiError::from),
    )
}

/// The provider of a third-party login page.
fn third_party(url: &str) -> Option<web_bridge::Provider> {
    let url = url::Url::parse(url).ok()?;
    match web_bridge::classify_url(&url) {
        web_bridge::PageKind::ThirdPartyLogin(provider) => Some(provider),
        _ => None,
    }
}

/// How often the downloads folder is checked for the engine zip.
const DOWNLOADS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// How long the downloads folder is watched (a 30 GB download can be slow).
const DOWNLOADS_WATCH_LIMIT: std::time::Duration = std::time::Duration::from_hours(4);

/// How often the download page is read again while it shows no engines.
const LINKS_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// How long the web window may stay open while the user passes Cloudflare's
/// check or signs in on unrealengine.com.
const LIST_DOWNLOADS_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(10);

/// Opens Epic's Linux download page in the web window, reads the list of
/// downloads from inside it and closes the window.
async fn list_with_window(webview_dir: PathBuf, session: Option<epic_auth::EpicSession>) -> Event {
    let mut window = match web_bridge::WebBridge::spawn(&webview_dir).await {
        Ok(window) => window,
        Err(error) => return Event::DownloadsWindowUnavailable(error.into()),
    };
    let result = read_downloads(&mut window, session.as_ref()).await;
    window.close().await;
    Event::EngineDownloads(result)
}

/// Reads the engine download links of Epic's page in the open window. While
/// the page shows none (no web session yet), the window stays open so the
/// user can sign in on the page.
async fn read_downloads(
    window: &mut web_bridge::WebBridge,
    session: Option<&epic_auth::EpicSession>,
) -> Result<Vec<ue_prebuilt::EngineDownload>, UiError> {
    let deadline = tokio::time::Instant::now() + LIST_DOWNLOADS_TIMEOUT;
    let remaining = || deadline.saturating_duration_since(tokio::time::Instant::now());
    // With a session, sign the window in with a one-time exchange code, so a
    // session started in the browser (Google, Apple…) works here too.
    let exchange = match session {
        Some(session) => match session.web_exchange_code().await {
            Ok(code) => Some(code),
            Err(error) => {
                tracing::warn!(%error, "cannot sign the web window in; opening the page as is");
                None
            }
        },
        None => None,
    };
    match exchange {
        Some(code) => {
            let url = epic_auth::web_login_url(&code, crate::text::ENGINE_DOWNLOAD_URL);
            window.open(url.expose())?;
        }
        None => window.open(crate::text::ENGINE_DOWNLOAD_URL)?,
    }
    loop {
        let links = window
            .page_links("www.unrealengine.com", remaining())
            .await?;
        let engines = ue_prebuilt::engines_from_links(links.iter().map(|l| l.url.expose()));
        if !engines.is_empty() {
            return Ok(engines);
        }
        // Not signed in on the page yet (or it is still rendering): wait for
        // the next page, or look again in a moment.
        tracing::debug!("no engine downloads on the page yet");
        match window
            .wait_navigation(remaining().min(LINKS_RETRY_INTERVAL))
            .await
        {
            Ok(_) => {}
            Err(web_bridge::Error::Timeout { .. }) if !remaining().is_zero() => {}
            Err(web_bridge::Error::Timeout { .. }) => {
                return Err(UiError::new(crate::text::NO_ENGINE_DOWNLOADS));
            }
            Err(error) => return Err(error.into()),
        }
    }
}
