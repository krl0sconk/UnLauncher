// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub's OAuth Device Flow.
//!
//! <https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow>

use std::fmt;
use std::time::Duration;

use common::Secret;
use serde::Deserialize;
use tokio::time::Instant;

use crate::error::{Error, Result};

/// Client ID of UnLauncher's GitHub OAuth App. Public by design: the Device
/// Flow needs no client secret.
pub const CLIENT_ID: &str = "Ov23liMYLA7zWmWYXqiv";

/// The scope UnLauncher asks for: `EpicGames/UnrealEngine` is private.
pub const SCOPE: &str = "repo";

/// GitHub's website, where the Device Flow endpoints live.
pub const GITHUB_WEB_BASE: &str = "https://github.com";

/// The grant type of the token request.
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// What GitHub adds to the polling interval on `slow_down`.
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);

/// Timeout of each request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Content type of the request bodies.
const FORM: &str = "application/x-www-form-urlencoded";

/// URL-encodes `pairs` as a form body.
fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

/// A code the user types on GitHub to authorize UnLauncher.
#[derive(Clone)]
pub struct DeviceCode {
    /// The short code to show (e.g. `WDJB-MJHT`).
    pub user_code: String,
    /// Where to type it, normally `https://github.com/login/device`.
    pub verification_uri: String,
    /// How long to wait between polls.
    pub interval: Duration,
    /// How long the code is valid.
    pub expires_in: Duration,
    secret: Secret,
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("interval", &self.interval)
            .field("expires_in", &self.expires_in)
            .field("device_code", &self.secret)
            .finish()
    }
}

/// GitHub's answer to `POST /login/device/code`.
#[derive(Deserialize)]
struct CodeAnswer {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// GitHub's answer to `POST /login/oauth/access_token`: a token or an error.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

/// The Device Flow with UnLauncher's OAuth App.
///
/// ```no_run
/// # async fn demo(http: reqwest::Client) -> github_auth::Result<()> {
/// let flow = github_auth::DeviceFlow::new(http);
/// let code = flow.start().await?;
/// println!("Type {} on {}", code.user_code, code.verification_uri);
/// let token = flow.wait_for_token(&code).await?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct DeviceFlow {
    http: reqwest::Client,
    web_base: String,
    client_id: String,
    slow_down_step: Duration,
}

impl DeviceFlow {
    /// The flow against `github.com` with [`CLIENT_ID`].
    #[must_use]
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            web_base: GITHUB_WEB_BASE.to_owned(),
            client_id: CLIENT_ID.to_owned(),
            slow_down_step: SLOW_DOWN_STEP,
        }
    }

    /// Uses another base URL instead of `https://github.com` (tests).
    #[must_use]
    pub fn with_web_base(mut self, web_base: impl Into<String>) -> Self {
        web_base
            .into()
            .trim_end_matches('/')
            .clone_into(&mut self.web_base);
        self
    }

    /// Changes what a `slow_down` answer adds to the interval (tests).
    #[must_use]
    pub fn with_slow_down_step(mut self, step: Duration) -> Self {
        self.slow_down_step = step;
        self
    }

    /// Asks GitHub for a device code to show to the user.
    ///
    /// # Errors
    ///
    /// [`Error::Http`], [`Error::UnexpectedStatus`] or
    /// [`Error::UnexpectedAnswer`].
    pub async fn start(&self) -> Result<DeviceCode> {
        const WHAT: &str = "the device code request";
        let response = self
            .http
            .post(format!("{}/login/device/code", self.web_base))
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, FORM)
            .body(form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", SCOPE),
            ]))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Error::UnexpectedStatus {
                what: WHAT,
                status: response.status().as_u16(),
            });
        }
        let answer: CodeAnswer = response
            .json()
            .await
            .map_err(|_| Error::UnexpectedAnswer { what: WHAT })?;
        tracing::info!(expires_in = answer.expires_in, "GitHub device code issued");
        Ok(DeviceCode {
            user_code: answer.user_code,
            verification_uri: answer.verification_uri,
            interval: Duration::from_secs(answer.interval),
            expires_in: Duration::from_secs(answer.expires_in),
            secret: Secret::new(answer.device_code),
        })
    }

    /// Polls GitHub until the user authorizes UnLauncher, and returns the
    /// token. Honours the interval and `slow_down`. Cancel it by dropping the
    /// future (for example, aborting its task).
    ///
    /// # Errors
    ///
    /// [`Error::Expired`] if the code expires first, [`Error::Denied`] if the
    /// user declines, [`Error::DeviceFlow`] for other GitHub errors, and
    /// [`Error::Http`], [`Error::UnexpectedStatus`] or
    /// [`Error::UnexpectedAnswer`].
    pub async fn wait_for_token(&self, code: &DeviceCode) -> Result<Secret> {
        const WHAT: &str = "the token request";
        let deadline = Instant::now() + code.expires_in;
        let mut interval = code.interval;
        loop {
            tokio::time::sleep(interval).await;
            if Instant::now() >= deadline {
                return Err(Error::Expired);
            }
            let response = self
                .http
                .post(format!("{}/login/oauth/access_token", self.web_base))
                .header(reqwest::header::ACCEPT, "application/json")
                .header(reqwest::header::CONTENT_TYPE, FORM)
                .body(form(&[
                    ("client_id", self.client_id.as_str()),
                    ("device_code", code.secret.expose()),
                    ("grant_type", GRANT_TYPE),
                ]))
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(Error::UnexpectedStatus {
                    what: WHAT,
                    status: response.status().as_u16(),
                });
            }
            let answer: TokenAnswer = response
                .json()
                .await
                .map_err(|_| Error::UnexpectedAnswer { what: WHAT })?;
            if let Some(token) = answer.access_token.filter(|t| !t.is_empty()) {
                tracing::info!("GitHub authorized UnLauncher");
                return Ok(Secret::new(token));
            }
            match answer.error.as_deref() {
                Some("authorization_pending") => {}
                Some("slow_down") => {
                    interval = answer
                        .interval
                        .map_or(interval + self.slow_down_step, Duration::from_secs);
                    tracing::debug!(?interval, "GitHub asked to slow down");
                }
                Some("expired_token") => return Err(Error::Expired),
                Some("access_denied") => return Err(Error::Denied),
                Some(other) => {
                    return Err(Error::DeviceFlow {
                        code: other.to_owned(),
                    });
                }
                None => return Err(Error::UnexpectedAnswer { what: WHAT }),
            }
        }
    }
}
