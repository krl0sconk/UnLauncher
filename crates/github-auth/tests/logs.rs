// SPDX-License-Identifier: GPL-3.0-or-later

//! Tokens never reach the logs. In its own test binary because it installs a
//! global `tracing` subscriber.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::Secret;
use github_auth::{DeviceFlow, check_access, detect_with};
use system::Sandbox;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn tokens_never_reach_the_logs() {
    let captured = Captured::default();
    let writer = captured.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish(),
    )
    .unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/device/code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_code": "dev-SECRET", "user_code": "ABCD-EFGH",
            "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 0
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"access_token": "gho_SECRET"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"login":"octo"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/EpicGames/UnrealEngine"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let flow = DeviceFlow::new(reqwest::Client::new())
        .with_web_base(server.uri())
        .with_slow_down_step(Duration::from_millis(1));
    let code = flow.start().await.unwrap();
    let token = flow.wait_for_token(&code).await.unwrap();
    tracing::debug!(?token, ?code, "flow done");
    check_access(&reqwest::Client::new(), &server.uri(), &token)
        .await
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let gh = dir.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\necho gho_SECRET_GH\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let found = detect_with(Sandbox::Host, &gh, &dir.path().join("no-git"));
    tracing::debug!(?found, "existing");
    let _ = Secret::new("x");

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("GitHub authorized UnLauncher"), "{logs}");
    assert!(!logs.contains("SECRET"), "{logs}");
}
