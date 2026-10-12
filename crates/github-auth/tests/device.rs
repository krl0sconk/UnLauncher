// SPDX-License-Identifier: GPL-3.0-or-later

//! Device Flow against a fake GitHub.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use github_auth::{CLIENT_ID, DeviceFlow, Error};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn server_with_code(interval: u64, expires_in: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/device/code"))
        .and(header("accept", "application/json"))
        .and(body_string_contains(format!("client_id={CLIENT_ID}")))
        .and(body_string_contains("scope=repo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_code": "dev-secret-123",
            "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device",
            "expires_in": expires_in,
            "interval": interval,
        })))
        .mount(&server)
        .await;
    server
}

fn token_answer(body: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn flow(server: &MockServer) -> DeviceFlow {
    DeviceFlow::new(reqwest::Client::new())
        .with_web_base(server.uri())
        .with_slow_down_step(Duration::from_millis(10))
}

/// Mounts `answers` for the token endpoint, one per poll, in order.
async fn mount_answers(server: &MockServer, answers: Vec<serde_json::Value>) {
    let last = answers.len() - 1;
    for (i, answer) in answers.into_iter().enumerate() {
        let mock = Mock::given(method("POST"))
            .and(path("/login/oauth/access_token"))
            .and(body_string_contains("device_code=dev-secret-123"))
            .and(body_string_contains(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code",
            ))
            .respond_with(token_answer(answer));
        if i == last {
            mock.mount(server).await;
        } else {
            mock.up_to_n_times(1).mount(server).await;
        }
    }
}

#[tokio::test]
async fn starts_with_a_user_code() {
    let server = server_with_code(5, 900).await;
    let code = flow(&server).start().await.unwrap();
    assert_eq!(code.user_code, "WDJB-MJHT");
    assert_eq!(code.verification_uri, "https://github.com/login/device");
    assert_eq!(code.interval, Duration::from_secs(5));
    assert_eq!(code.expires_in, Duration::from_mins(15));
    assert!(!format!("{code:?}").contains("dev-secret-123"));
}

#[tokio::test]
async fn waits_while_pending_then_returns_the_token() {
    let server = server_with_code(0, 900).await;
    mount_answers(
        &server,
        vec![
            serde_json::json!({"error": "authorization_pending"}),
            serde_json::json!({"error": "authorization_pending"}),
            serde_json::json!({"access_token": "gho_t0ken", "token_type": "bearer", "scope": "repo"}),
        ],
    )
    .await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    let token = flow.wait_for_token(&code).await.unwrap();
    assert_eq!(token.expose(), "gho_t0ken");
}

#[tokio::test]
async fn slows_down_when_asked() {
    let server = server_with_code(0, 900).await;
    mount_answers(
        &server,
        vec![
            serde_json::json!({"error": "slow_down"}),
            serde_json::json!({"access_token": "gho_t0ken"}),
        ],
    )
    .await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    let started = std::time::Instant::now();
    flow.wait_for_token(&code).await.unwrap();
    // The second poll waited the added step.
    assert!(started.elapsed() >= Duration::from_millis(10));
}

#[tokio::test]
async fn reports_an_expired_code() {
    let server = server_with_code(0, 900).await;
    mount_answers(&server, vec![serde_json::json!({"error": "expired_token"})]).await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    assert!(matches!(
        flow.wait_for_token(&code).await,
        Err(Error::Expired)
    ));
}

#[tokio::test]
async fn gives_up_when_the_code_runs_out() {
    let server = server_with_code(1, 0).await;
    mount_answers(
        &server,
        vec![serde_json::json!({"error": "authorization_pending"})],
    )
    .await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    assert!(matches!(
        flow.wait_for_token(&code).await,
        Err(Error::Expired)
    ));
}

#[tokio::test]
async fn reports_a_denied_authorization() {
    let server = server_with_code(0, 900).await;
    mount_answers(&server, vec![serde_json::json!({"error": "access_denied"})]).await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    assert!(matches!(
        flow.wait_for_token(&code).await,
        Err(Error::Denied)
    ));
}

#[tokio::test]
async fn reports_other_errors_by_code() {
    let server = server_with_code(0, 900).await;
    mount_answers(
        &server,
        vec![serde_json::json!({"error": "device_flow_disabled"})],
    )
    .await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    let error = flow.wait_for_token(&code).await.unwrap_err();
    assert!(matches!(&error, Error::DeviceFlow { code } if code == "device_flow_disabled"));
}

#[tokio::test]
async fn reports_a_failed_start() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/device/code"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let error = flow(&server).start().await.unwrap_err();
    assert!(matches!(error, Error::UnexpectedStatus { status: 503, .. }));
}

#[tokio::test]
async fn can_be_cancelled_by_dropping_it() {
    let server = server_with_code(0, 900).await;
    mount_answers(
        &server,
        vec![serde_json::json!({"error": "authorization_pending"})],
    )
    .await;
    let flow = flow(&server);
    let code = flow.start().await.unwrap();
    let waiting =
        tokio::time::timeout(Duration::from_millis(100), flow.wait_for_token(&code)).await;
    assert!(waiting.is_err(), "still waiting after 100 ms");
}
