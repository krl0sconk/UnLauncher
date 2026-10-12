// SPDX-License-Identifier: GPL-3.0-or-later

//! `check_access` against a fake GitHub API.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use common::{Secret, UserFacing};
use github_auth::{Access, Error, check_access};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RESTRICTED: &str = r#"{"message":"Although you appear to have the correct authorization credentials, the `EpicGames` organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited."}"#;

async fn api(user: u16, repo: (u16, &str), membership: Option<(u16, &str)>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("authorization", "Bearer t0ken"))
        .respond_with(ResponseTemplate::new(user).set_body_string(r#"{"login":"octo"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/EpicGames/UnrealEngine"))
        .respond_with(ResponseTemplate::new(repo.0).set_body_string(repo.1))
        .mount(&server)
        .await;
    if let Some((status, body)) = membership {
        Mock::given(method("GET"))
            .and(path("/user/memberships/orgs/EpicGames"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
    }
    server
}

async fn check(server: &MockServer) -> Result<Access, Error> {
    check_access(
        &reqwest::Client::new(),
        &server.uri(),
        &Secret::new("t0ken"),
    )
    .await
}

#[tokio::test]
async fn ready_when_the_repository_is_visible() {
    let server = api(200, (200, "{}"), None).await;
    let access = check(&server).await.unwrap();
    assert_eq!(
        access,
        Access::Ready {
            login: "octo".into()
        }
    );
    assert!(access.is_ready());
    assert_eq!(access.hint(), None);
}

#[tokio::test]
async fn revoked_when_github_rejects_the_token() {
    let server = api(401, (401, ""), None).await;
    let access = check(&server).await.unwrap();
    assert_eq!(access, Access::Revoked);
    assert!(access.hint().unwrap().contains("Connect GitHub again"));
}

#[tokio::test]
async fn invitation_pending() {
    let server = api(
        200,
        (404, r#"{"message":"Not Found"}"#),
        Some((200, r#"{"state":"pending","role":"member"}"#)),
    )
    .await;
    let access = check(&server).await.unwrap();
    assert_eq!(
        access,
        Access::InvitationPending {
            login: "octo".into()
        }
    );
    assert!(
        access
            .hint()
            .unwrap()
            .contains("github.com/orgs/EpicGames/invitation")
    );
}

#[tokio::test]
async fn not_linked_without_membership() {
    let server = api(
        200,
        (404, r#"{"message":"Not Found"}"#),
        Some((404, r#"{"message":"Not Found"}"#)),
    )
    .await;
    let access = check(&server).await.unwrap();
    assert_eq!(
        access,
        Access::NotLinked {
            login: "octo".into()
        }
    );
    assert!(access.hint().unwrap().contains("ue-on-github"));
}

#[tokio::test]
async fn blocked_when_the_organization_restricts_oauth_apps() {
    let server = api(200, (403, RESTRICTED), None).await;
    assert_eq!(
        check(&server).await.unwrap(),
        Access::Blocked {
            login: "octo".into()
        }
    );

    let server = api(
        200,
        (404, r#"{"message":"Not Found"}"#),
        Some((403, RESTRICTED)),
    )
    .await;
    assert_eq!(
        check(&server).await.unwrap(),
        Access::Blocked {
            login: "octo".into()
        }
    );
}

#[tokio::test]
async fn blocked_when_a_member_cannot_see_the_repository() {
    let server = api(200, (404, "{}"), Some((200, r#"{"state":"active"}"#))).await;
    let access = check(&server).await.unwrap();
    assert_eq!(
        access,
        Access::Blocked {
            login: "octo".into()
        }
    );
    assert!(access.summary().contains("blocks"));
}

#[tokio::test]
async fn server_errors_are_errors() {
    let server = api(200, (502, ""), None).await;
    assert!(matches!(
        check(&server).await,
        Err(Error::UnexpectedStatus { status: 502, .. })
    ));
}

#[test]
fn messages_never_contain_the_token() {
    for access in [
        Access::Ready {
            login: "octo".into(),
        },
        Access::NotLinked {
            login: "octo".into(),
        },
        Access::InvitationPending {
            login: "octo".into(),
        },
        Access::Blocked {
            login: "octo".into(),
        },
        Access::Revoked,
    ] {
        let text = format!("{access:?} {} {:?}", access.summary(), access.hint());
        assert!(!text.contains("t0ken"), "{text}");
    }
}
