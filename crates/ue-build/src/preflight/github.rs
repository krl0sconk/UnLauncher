// SPDX-License-Identifier: GPL-3.0-or-later

//! Access to the private `EpicGames/UnrealEngine` repository, through
//! `github_auth::check_access`.

use common::{Secret, UserFacing};
use github_auth::{Access, Error};

use super::{CheckResult, CheckStatus};

pub use github_auth::{GITHUB_API_BASE, UE_REPO};

const NAME: &str = "GitHub access";

/// Maps the outcome of `check_access` to a pre-flight result.
pub(super) fn github_result(access: Result<Access, Error>) -> CheckResult {
    match access {
        Ok(access @ Access::Ready { .. }) => CheckResult::new(
            NAME,
            CheckStatus::Ok,
            format!("ok: {}", access.summary()),
            None,
        ),
        Ok(access) => CheckResult::new(NAME, CheckStatus::Fail, access.summary(), access.hint()),
        // GitHub itself failed (5xx, rate limit): building may still work later.
        Err(error @ (Error::UnexpectedStatus { .. } | Error::UnexpectedAnswer { .. })) => {
            CheckResult::new(NAME, CheckStatus::Warn, error.summary(), error.hint())
        }
        Err(error) => {
            tracing::warn!(%error, "GitHub access check failed");
            CheckResult::new(NAME, CheckStatus::Fail, error.summary(), error.hint())
        }
    }
}

pub(super) async fn check_github(
    http: &reqwest::Client,
    api_base: &str,
    token: Option<&Secret>,
) -> CheckResult {
    let Some(token) = token else {
        return CheckResult::new(
            NAME,
            CheckStatus::Fail,
            "GitHub is not connected",
            Some(
                "Connect GitHub in Install engine → From source (no token needed), or run \
                 `unlauncher-cli github login`."
                    .to_owned(),
            ),
        );
    };
    github_result(github_auth::check_access(http, api_base, token).await)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn check_with(repo_status: u16, membership: Option<&str>) -> CheckResult {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"login":"octo"}"#))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/EpicGames/UnrealEngine"))
            .respond_with(ResponseTemplate::new(repo_status))
            .mount(&server)
            .await;
        if let Some(body) = membership {
            Mock::given(method("GET"))
                .and(path("/user/memberships/orgs/EpicGames"))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&server)
                .await;
        }
        let client = reqwest::Client::new();
        check_github(&client, &server.uri(), Some(&Secret::new("t0ken"))).await
    }

    #[tokio::test]
    async fn ok_when_repo_is_visible() {
        let r = check_with(200, None).await;
        assert_eq!(r.status, CheckStatus::Ok);
        assert!(r.message.starts_with("ok: @octo"), "{}", r.message);
    }

    #[tokio::test]
    async fn pending_invitation_fails_with_a_hint() {
        let r = check_with(404, Some(r#"{"state":"pending"}"#)).await;
        assert_eq!(r.status, CheckStatus::Fail);
        assert!(r.hint.unwrap().contains("invitation"));
    }

    #[tokio::test]
    async fn server_error_is_a_warning() {
        assert_eq!(check_with(502, None).await.status, CheckStatus::Warn);
    }

    #[tokio::test]
    async fn missing_token_fails_without_request() {
        let r = check_github(&reqwest::Client::new(), "http://127.0.0.1:9", None).await;
        assert_eq!(r.status, CheckStatus::Fail);
        assert!(r.hint.unwrap().contains("github login"));
    }

    #[tokio::test]
    async fn messages_never_contain_the_token() {
        for (status, membership) in [(200, None), (404, Some(r#"{"state":"active"}"#))] {
            let r = check_with(status, membership).await;
            assert!(!format!("{r:?}").contains("t0ken"));
        }
    }
}
