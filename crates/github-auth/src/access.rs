// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether a GitHub token can read `EpicGames/UnrealEngine`, and if not, why.

use std::time::Duration;

use common::{Secret, UserFacing};
use reqwest::StatusCode;
use serde::Deserialize;

use crate::error::{Error, Result};

/// The public GitHub API.
pub const GITHUB_API_BASE: &str = "https://api.github.com";
/// The organization that owns the engine repository.
pub const UE_ORG: &str = "EpicGames";
/// The engine repository, as `owner/name`.
pub const UE_REPO: &str = "EpicGames/UnrealEngine";

/// Epic's instructions to link GitHub and get access.
const UE_ON_GITHUB: &str = "https://www.unrealengine.com/en-US/ue-on-github";

/// What GitHub says when an organization blocks unapproved OAuth Apps.
const OAUTH_RESTRICTION: &str = "OAuth App access restriction";

/// Timeout of each request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// What a GitHub token can do with the engine repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// It can read `EpicGames/UnrealEngine`.
    Ready {
        /// The GitHub account.
        login: String,
    },
    /// The account is not linked to an Epic Games account (or has no
    /// invitation to the EpicGames organization yet).
    NotLinked {
        /// The GitHub account.
        login: String,
    },
    /// The account was invited to the EpicGames organization but has not
    /// accepted yet.
    InvitationPending {
        /// The GitHub account.
        login: String,
    },
    /// The account belongs to the EpicGames organization, but this token
    /// cannot see the repository: the organization blocks OAuth Apps it has
    /// not approved (such as UnLauncher's). Credentials from `gh` or a
    /// personal token still work.
    Blocked {
        /// The GitHub account.
        login: String,
    },
    /// GitHub rejected the token: revoked, or never valid.
    Revoked,
}

impl Access {
    /// Whether the repository can be cloned.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// The GitHub account, when the token is valid.
    #[must_use]
    pub fn login(&self) -> Option<&str> {
        match self {
            Self::Ready { login }
            | Self::NotLinked { login }
            | Self::InvitationPending { login }
            | Self::Blocked { login } => Some(login),
            Self::Revoked => None,
        }
    }
}

impl UserFacing for Access {
    fn summary(&self) -> String {
        match self {
            Self::Ready { login } => format!("@{login} can read {UE_REPO}"),
            Self::NotLinked { login } => {
                format!("@{login} is not linked to your Epic Games account yet")
            }
            Self::InvitationPending { login } => {
                format!("@{login} has a pending invitation to the {UE_ORG} organization")
            }
            Self::Blocked { login } => {
                format!("@{login} is linked to Epic, but {UE_ORG} blocks UnLauncher's GitHub app")
            }
            Self::Revoked => "GitHub rejected the saved access (revoked or not valid)".to_owned(),
        }
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::Ready { .. } => return None,
            Self::NotLinked { .. } => format!(
                "Link GitHub in your Epic account's connections, then accept the invitation \
                 to the {UE_ORG} organization that GitHub sends. Instructions: {UE_ON_GITHUB}"
            ),
            Self::InvitationPending { .. } => format!(
                "Accept the invitation at https://github.com/orgs/{UE_ORG}/invitation (also \
                 sent by email)."
            ),
            Self::Blocked { .. } => "Use this computer's GitHub account (from the GitHub CLI or \
                 git), or a personal token under Advanced options: they are not affected."
                .to_owned(),
            Self::Revoked => "Connect GitHub again.".to_owned(),
        };
        Some(hint)
    }
}

/// `GET /user`.
#[derive(Deserialize)]
struct User {
    login: String,
}

/// `GET /user/memberships/orgs/{org}`.
#[derive(Deserialize)]
struct Membership {
    state: String,
}

/// Checks what `token` can do with `EpicGames/UnrealEngine`. `api_base` is
/// normally [`GITHUB_API_BASE`].
///
/// # Errors
///
/// [`Error::Http`] if GitHub cannot be reached, [`Error::UnexpectedStatus`] or
/// [`Error::UnexpectedAnswer`] if it answers something unexpected (a server
/// error, the rate limit).
pub async fn check_access(
    http: &reqwest::Client,
    api_base: &str,
    token: &Secret,
) -> Result<Access> {
    let api = api_base.trim_end_matches('/');
    let get = |path: String| {
        http.get(format!("{api}{path}"))
            .bearer_auth(token.expose())
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .timeout(REQUEST_TIMEOUT)
            .send()
    };

    let user = get("/user".to_owned()).await?;
    let login = match user.status() {
        StatusCode::UNAUTHORIZED => return Ok(Access::Revoked),
        status if status.is_success() => {
            user.json::<User>()
                .await
                .map_err(|_| Error::UnexpectedAnswer {
                    what: "the user request",
                })?
                .login
        }
        status => {
            return Err(Error::UnexpectedStatus {
                what: "the user request",
                status: status.as_u16(),
            });
        }
    };

    let repo = get(format!("/repos/{UE_REPO}")).await?;
    match repo.status() {
        StatusCode::OK => return Ok(Access::Ready { login }),
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN => {
            if repo
                .text()
                .await
                .unwrap_or_default()
                .contains(OAUTH_RESTRICTION)
            {
                return Ok(Access::Blocked { login });
            }
        }
        status => {
            return Err(Error::UnexpectedStatus {
                what: "the repository request",
                status: status.as_u16(),
            });
        }
    }

    // The repository is hidden: find out whether the invitation is pending
    // or the organization blocks this app.
    let membership = get(format!("/user/memberships/orgs/{UE_ORG}")).await?;
    let status = membership.status();
    let body = membership.text().await.unwrap_or_default();
    if body.contains(OAUTH_RESTRICTION) {
        return Ok(Access::Blocked { login });
    }
    if status.is_success() {
        let state = serde_json::from_str::<Membership>(&body)
            .map_err(|_| Error::UnexpectedAnswer {
                what: "the membership request",
            })?
            .state;
        return Ok(match state.as_str() {
            "pending" => Access::InvitationPending { login },
            // A member that cannot see the repository: this app is not allowed.
            "active" => Access::Blocked { login },
            _ => Access::NotLinked { login },
        });
    }
    Ok(Access::NotLinked { login })
}
