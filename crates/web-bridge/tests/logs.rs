// SPDX-License-Identifier: GPL-3.0-or-later

//! The authorization code never reaches the logs. In its own test binary
//! because it installs a global `tracing` subscriber.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};

use system::HostCommand;
use tempfile::TempDir;
use web_bridge::WebBridge;

fn fake_window(body: &str) -> (TempDir, HostCommand) {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unlauncher-web");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let command = HostCommand::bundled_in(dir.path(), "unlauncher-web").unwrap();
    (dir, command)
}

const LOGIN: &str = r#"
echo '{"type":"ready"}'
while IFS= read -r line; do
  case "$line" in
    *'"type":"open"'*)
      echo '{"type":"navigated","url":"https://www.epicgames.com/id/api/redirect"}'
      echo 'not a protocol line with code 0123456789abcdef0123456789abcdef'
      echo '{"type":"auth_code","code":"0123456789abcdef0123456789abcdef"}' ;;
    *'"type":"close"'*) echo '{"type":"closed"}'; exit 0 ;;
  esac
done
"#;

/// Collects everything `tracing` writes while a test runs.
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
async fn the_auth_code_never_reaches_the_logs() {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let (dir, command) = fake_window(LOGIN);
    let mut bridge = WebBridge::spawn_with(command, &dir.path().join("webview"))
        .await
        .unwrap();
    bridge.open("https://www.epicgames.com/id/login").unwrap();
    let code = bridge.wait_auth_code().await.unwrap();
    tracing::debug!(?code, "received");
    bridge.close().await;

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("ignoring an unknown message"), "{logs}");
    assert!(!logs.contains("0123456789abcdef"), "{logs}");
}
