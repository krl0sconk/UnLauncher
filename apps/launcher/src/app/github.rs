// SPDX-License-Identifier: GPL-3.0-or-later

//! Connecting GitHub without tokens: the saved token's access,
//! this computer's existing credentials, the Device Flow and the steps to get
//! into the EpicGames organization.

#[allow(clippy::wildcard_imports, reason = "a part of the app module")]
use super::*;

use common::Secret;
use epic_auth::{CredentialKey, KeyringStore, SecretStore};
use github_auth::{Access, ExistingSource};

/// A GitHub token this computer already has, checked.
pub struct ExistingOffer {
    pub source: ExistingSource,
    pub login: String,
    /// Whether it can read the engine repository.
    pub is_ready: bool,
    token: Secret,
}

/// The Device Flow in progress: the code the user types on GitHub.
pub struct DeviceView {
    pub user_code: String,
    pub verification_uri: String,
}

/// Signing in with the GitHub CLI.
pub enum GhLogin {
    /// `gh` is starting.
    Starting,
    /// The code to type on GitHub.
    Code(String),
}

/// Everything the interface knows about GitHub.
#[derive(Default)]
pub struct GithubState {
    /// Whether a token is saved in the keyring; `None` until read.
    pub saved: Option<bool>,
    /// What the saved token can do.
    pub access: Option<Result<Access, UiError>>,
    pub checking: bool,
    /// Credentials found on this computer that GitHub accepts.
    pub existing: Vec<ExistingOffer>,
    pub detecting: bool,
    pub device: Option<DeviceView>,
    device_task: Option<tokio::task::AbortHandle>,
    /// Whether the host has the GitHub CLI; `None` until checked.
    pub gh_installed: Option<bool>,
    /// Signing in with the GitHub CLI.
    pub gh_login: Option<GhLogin>,
    gh_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub error: Option<UiError>,
    /// The Epic connections page is open in the web window.
    pub linking: bool,
}

impl GithubState {
    /// The saved token's access, if it was checked and GitHub answered.
    pub fn access(&self) -> Option<&Access> {
        self.access.as_ref().and_then(|a| a.as_ref().ok())
    }

    /// Whether the engine repository can be cloned with the saved token.
    pub fn is_ready(&self) -> bool {
        self.access().is_some_and(Access::is_ready)
    }

    /// The connected GitHub account, if the saved token is valid.
    pub fn login(&self) -> Option<&str> {
        self.access().and_then(Access::login)
    }
}

/// Progress of GitHub work done off the UI thread.
pub enum GithubEvent {
    /// The keyring was read and, if there is a token, its access checked.
    Status {
        saved: bool,
        access: Option<Result<Access, UiError>>,
    },
    Existing {
        offers: Vec<ExistingOffer>,
        gh_installed: bool,
    },
    /// The GitHub CLI printed its one-time code.
    GhCode(String),
    DeviceCode(Result<DeviceView, UiError>),
    /// A new token was saved (Device Flow, existing credential or pasted) and
    /// checked; or connecting failed.
    Connected(Result<Access, UiError>),
    /// The Epic connections window was closed.
    LinkWindowClosed,
}

impl LauncherApp {
    pub(super) fn handle_github(&mut self, intent: Intent) {
        match intent {
            Intent::GithubRefresh => self.refresh_github(),
            Intent::GithubConnect => self.connect_github(),
            Intent::GithubCancelDevice => self.cancel_device(),
            Intent::GithubGhLogin => self.gh_login(),
            Intent::GithubUseExisting(index) => self.use_existing(index),
            Intent::SaveGitHubToken(token) => {
                if let Some(Dialog::InstallEngine(form)) = &mut self.dialog {
                    form.token_input.clear();
                }
                let token = token.trim().to_owned();
                if !token.is_empty() {
                    self.save_and_check(Secret::new(token));
                }
            }
            Intent::GithubDisconnect => self.disconnect_github(),
            Intent::OpenEpicConnections => self.open_epic_connections(),
            Intent::OpenGithubInvitation => {
                if let Err(error) = system::open_url(crate::text::GITHUB_INVITATION_URL) {
                    self.github.error = Some(error.into());
                }
            }
            _ => {}
        }
    }

    pub(super) fn github_event(&mut self, event: GithubEvent) {
        let state = &mut self.github;
        match event {
            GithubEvent::Status { saved, access } => {
                state.checking = false;
                state.saved = Some(saved);
                state.access = access;
                if !self.github.is_ready() {
                    self.detect_existing();
                }
            }
            GithubEvent::Existing {
                offers,
                gh_installed,
            } => {
                state.detecting = false;
                state.existing = offers;
                state.gh_installed = Some(gh_installed);
            }
            GithubEvent::GhCode(code) => {
                self.ctx.copy_text(code.clone());
                if let Err(error) = system::open_url(crate::text::GITHUB_DEVICE_URL) {
                    state.error = Some(error.into());
                }
                state.gh_login = Some(GhLogin::Code(code));
            }
            GithubEvent::DeviceCode(Ok(view)) => {
                // Copy the code and open GitHub, so the user only has to paste.
                self.ctx.copy_text(view.user_code.clone());
                if let Err(error) = system::open_url(&view.verification_uri) {
                    state.error = Some(error.into());
                }
                state.device = Some(view);
            }
            GithubEvent::DeviceCode(Err(error)) => {
                state.device = None;
                state.device_task = None;
                state.error = Some(error);
            }
            GithubEvent::Connected(result) => {
                state.device = None;
                state.device_task = None;
                state.gh_login = None;
                state.gh_cancel = None;
                state.checking = false;
                match result {
                    Ok(access) => {
                        state.saved = Some(true);
                        state.access = Some(Ok(access));
                        state.error = None;
                        if !self.github.is_ready() {
                            self.detect_existing();
                        }
                    }
                    // A cancel is not an error to show.
                    Err(error) if error.summary == crate::text::CANCELLED => {}
                    Err(error) => state.error = Some(error),
                }
            }
            GithubEvent::LinkWindowClosed => {
                state.linking = false;
                self.refresh_github();
            }
        }
    }

    /// Reads the saved token and checks its access.
    pub(super) fn refresh_github(&mut self) {
        if self.github.checking {
            return;
        }
        self.github.checking = true;
        self.github.error = None;
        let http = self.http.clone();
        self.spawn_async(async move {
            let token =
                tokio::task::spawn_blocking(|| KeyringStore.get(CredentialKey::GitHubToken))
                    .await
                    .map_err(|_| UiError::new("reading the keyring stopped unexpectedly"));
            let token = match token {
                Ok(Ok(token)) => token,
                Ok(Err(error)) => {
                    return Event::Github(GithubEvent::Status {
                        saved: false,
                        access: Some(Err(error.into())),
                    });
                }
                Err(error) => {
                    return Event::Github(GithubEvent::Status {
                        saved: false,
                        access: Some(Err(error)),
                    });
                }
            };
            let access = match &token {
                Some(token) => Some(
                    github_auth::check_access(&http, github_auth::GITHUB_API_BASE, token)
                        .await
                        .map_err(UiError::from),
                ),
                None => None,
            };
            Event::Github(GithubEvent::Status {
                saved: token.is_some(),
                access,
            })
        });
    }

    /// Looks for GitHub credentials this computer already has.
    fn detect_existing(&mut self) {
        if self.github.detecting {
            return;
        }
        self.github.detecting = true;
        let (http, sandbox) = (self.http.clone(), self.env.sandbox);
        self.spawn_async(async move {
            let (found, gh_installed) = tokio::task::spawn_blocking(move || {
                let gh = std::path::Path::new("gh");
                (
                    github_auth::detect_existing(sandbox),
                    github_auth::gh_installed(sandbox, gh),
                )
            })
            .await
            .unwrap_or_default();
            let mut offers = Vec::new();
            for credential in found {
                let access = github_auth::check_access(
                    &http,
                    github_auth::GITHUB_API_BASE,
                    &credential.token,
                )
                .await;
                if let Ok(access) = access
                    && let Some(login) = access.login()
                {
                    offers.push(ExistingOffer {
                        source: credential.source,
                        login: login.to_owned(),
                        is_ready: access.is_ready(),
                        token: credential.token,
                    });
                }
            }
            Event::Github(GithubEvent::Existing {
                offers,
                gh_installed,
            })
        });
    }

    /// Starts the Device Flow: shows a code, waits for the user to authorize
    /// UnLauncher on GitHub, saves the token and checks it.
    fn connect_github(&mut self) {
        if self.github.device_task.is_some() {
            return;
        }
        self.github.error = None;
        let http = self.http.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let report = move |event| {
            let _ = tx.send(Event::Github(event));
            ctx.request_repaint();
        };
        let task = self.runtime.spawn(async move {
            let flow = github_auth::DeviceFlow::new(http.clone());
            let code = match flow.start().await {
                Ok(code) => code,
                Err(error) => return report(GithubEvent::DeviceCode(Err(error.into()))),
            };
            report(GithubEvent::DeviceCode(Ok(DeviceView {
                user_code: code.user_code.clone(),
                verification_uri: code.verification_uri.clone(),
            })));
            let result = match flow.wait_for_token(&code).await {
                Ok(token) => save_and_check(http, token).await,
                Err(error) => Err(error.into()),
            };
            report(GithubEvent::Connected(result));
        });
        self.github.device_task = Some(task.abort_handle());
    }

    fn cancel_device(&mut self) {
        if let Some(task) = self.github.device_task.take() {
            task.abort();
        }
        self.github.device = None;
        if let Some(cancel) = self.github.gh_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.github.gh_login = None;
    }

    /// Signs the host's GitHub CLI in through the browser (its OAuth App is
    /// approved by EpicGames), then uses its token.
    fn gh_login(&mut self) {
        if self.github.gh_login.is_some() {
            return;
        }
        self.github.error = None;
        self.github.gh_login = Some(GhLogin::Starting);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.github.gh_cancel = Some(std::sync::Arc::clone(&cancel));
        let (http, sandbox) = (self.http.clone(), self.env.sandbox);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        self.spawn_async(async move {
            let gh = std::path::Path::new("gh");
            let token = tokio::task::spawn_blocking(move || {
                github_auth::gh_login(sandbox, gh, &cancel, |code| {
                    let _ = tx.send(Event::Github(GithubEvent::GhCode(code.to_owned())));
                    ctx.request_repaint();
                })?;
                github_auth::gh_token(sandbox, gh).ok_or(github_auth::Error::GhLogin {
                    message: "the GitHub CLI signed in but gave no token".to_owned(),
                })
            })
            .await;
            let result = match token {
                Ok(Ok(token)) => save_and_check(http, token).await,
                Ok(Err(github_auth::Error::Cancelled)) => Err(UiError::new(crate::text::CANCELLED)),
                Ok(Err(error)) => Err(error.into()),
                Err(_) => Err(UiError::new("the GitHub CLI stopped unexpectedly")),
            };
            Event::Github(GithubEvent::Connected(result))
        });
    }

    fn use_existing(&mut self, index: usize) {
        if let Some(offer) = self.github.existing.get(index) {
            self.save_and_check(offer.token.clone());
        }
    }

    /// Saves `token` in the keyring and checks it.
    fn save_and_check(&mut self, token: Secret) {
        self.github.checking = true;
        self.github.error = None;
        let http = self.http.clone();
        self.spawn_async(async move {
            Event::Github(GithubEvent::Connected(save_and_check(http, token).await))
        });
    }

    /// Forgets the saved token. GitHub keeps the authorization until the user
    /// revokes it at github.com/settings/applications.
    fn disconnect_github(&mut self) {
        self.cancel_device();
        self.github.saved = Some(false);
        self.github.access = None;
        self.github.existing.clear();
        self.spawn(|| Event::Done {
            result: KeyringStore
                .delete(CredentialKey::GitHubToken)
                .map(|()| crate::text::GITHUB_DISCONNECTED.to_owned())
                .map_err(UiError::from),
            reload: false,
        });
    }

    /// Opens the connections page of the Epic account in the web window,
    /// signed in with the Epic session when there is one.
    fn open_epic_connections(&mut self) {
        if self.github.linking {
            return;
        }
        self.github.linking = true;
        let webview_dir = self.env.webview_dir.clone();
        let session = match &self.library.epic {
            EpicState::SignedIn(session) => Some(session.clone()),
            _ => None,
        };
        self.spawn_async(async move {
            match web_bridge::WebBridge::spawn(&webview_dir).await {
                Ok(mut window) => {
                    let url = match &session {
                        Some(session) => session.web_exchange_code().await.ok().map(|code| {
                            epic_auth::web_login_url(&code, crate::text::EPIC_CONNECTIONS_URL)
                        }),
                        None => None,
                    };
                    let opened = match &url {
                        Some(url) => window.open(url.expose()),
                        None => window.open(crate::text::EPIC_CONNECTIONS_URL),
                    };
                    if opened.is_ok() {
                        while let Some(event) = window.next_event().await {
                            if event == web_bridge::Event::Closed {
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "web window unavailable; opening Epic connections in the browser");
                    let _ = tokio::task::spawn_blocking(|| {
                        system::open_url(crate::text::EPIC_CONNECTIONS_URL)
                    })
                    .await;
                }
            }
            Event::Github(GithubEvent::LinkWindowClosed)
        });
    }
}

/// Saves `token` in the keyring, then checks what it can do.
async fn save_and_check(http: reqwest::Client, token: Secret) -> Result<Access, UiError> {
    let stored = token.clone();
    tokio::task::spawn_blocking(move || KeyringStore.set(CredentialKey::GitHubToken, &stored))
        .await
        .map_err(|_| UiError::new("saving to the keyring stopped unexpectedly"))??;
    Ok(github_auth::check_access(&http, github_auth::GITHUB_API_BASE, &token).await?)
}
