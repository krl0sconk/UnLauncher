// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `WebBridge`, against fake `unlauncher-web` programs written as
//! shell scripts that speak the same protocol.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use system::HostCommand;
use tempfile::TempDir;
use web_bridge::{Error, Event, WebBridge, clear_data};

/// A folder with a fake `unlauncher-web` running `body` after printing
/// `ready`. The script writes the `--data-dir` it got to `args.txt`.
fn fake_window(body: &str) -> (TempDir, HostCommand) {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let script =
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args.txt\"\n{body}\n");
    let path = dir.path().join("unlauncher-web");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let command = HostCommand::bundled_in(dir.path(), "unlauncher-web").unwrap();
    (dir, command)
}

const READY: &str = r#"echo '{"type":"ready"}'"#;

/// Starts the fake window. Retries while the script is "busy": another test
/// thread may fork while this one writes its script, and the child holds the
/// file open for writing until it execs (rust-lang/rust#114554).
async fn spawn(command: HostCommand, data_dir: &Path) -> Result<WebBridge, Error> {
    for _ in 0..50 {
        match WebBridge::spawn_with(command.clone(), data_dir).await {
            Err(Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::ExecutableFileBusy =>
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            result => return result,
        }
    }
    WebBridge::spawn_with(command, data_dir).await
}

/// Answers `open` with a load of the Linux download page; the first `fetch`
/// gets Cloudflare's check (and the page reloads), the next ones the JSON.
const DOWNLOAD_PAGE: &str = r#"
n=0
while IFS= read -r line; do
  case "$line" in
    *'"type":"open"'*) echo '{"type":"navigated","url":"https://www.unrealengine.com/linux"}' ;;
    *'"type":"fetch"'*)
      n=$((n+1))
      if [ $n -eq 1 ]; then
        echo '{"type":"fetch_result","status":403,"body":"<html>Just a moment...</html>"}'
        echo '{"type":"navigated","url":"https://www.unrealengine.com/linux"}'
      else
        echo '{"type":"fetch_result","status":200,"body":"{\"blobs\":[]}"}'
      fi ;;
    *'"type":"close"'*) echo '{"type":"closed"}'; exit 0 ;;
  esac
done
"#;

/// Answers `open` with the login form and then the authorization code.
const LOGIN: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"type":"open"'*)
      echo '{"type":"navigated","url":"https://www.epicgames.com/id/login"}'
      echo '{"type":"navigated","url":"https://www.epicgames.com/id/api/redirect"}'
      echo 'not a protocol line with code 0123456789abcdef0123456789abcdef'
      echo '{"type":"auth_code","code":"0123456789abcdef0123456789abcdef"}' ;;
    *'"type":"close"'*) echo '{"type":"closed"}'; exit 0 ;;
  esac
done
"#;

#[tokio::test]
async fn starts_and_passes_the_data_dir() {
    let (dir, command) = fake_window(&format!("{READY}\nsleep 5"));
    let data = dir.path().join("webview");
    let bridge = spawn(command, &data).await.unwrap();
    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    assert_eq!(args, format!("--data-dir\n{}\n", data.display()));
    assert!(bridge.loaded_page().is_none());
}

#[tokio::test]
async fn fetch_json_retries_until_the_page_answers_json() {
    let (dir, command) = fake_window(&format!("{READY}\n{DOWNLOAD_PAGE}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge.open("https://www.unrealengine.com/linux").unwrap();
    let body = bridge
        .fetch_json(
            "/api/blobs/linux",
            "www.unrealengine.com",
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert_eq!(body, r#"{"blobs":[]}"#);
    assert_eq!(
        bridge.loaded_page().unwrap().as_str(),
        "https://www.unrealengine.com/linux"
    );
    bridge.close().await;
}

#[tokio::test]
async fn fetch_json_gives_up_with_the_last_status() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"type":"open"'*) echo '{"type":"navigated","url":"https://www.unrealengine.com/linux"}' ;;
    *'"type":"fetch"'*) echo '{"type":"fetch_result","status":401,"body":"<html></html>"}' ;;
  esac
done
"#;
    let (dir, command) = fake_window(&format!("{READY}\n{script}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge.open("https://www.unrealengine.com/linux").unwrap();
    let error = bridge
        .fetch_json(
            "/api/blobs/linux",
            "www.unrealengine.com",
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotJson { status: 401 }), "{error:?}");
}

#[tokio::test]
async fn fetch_json_fails_when_the_window_is_closed() {
    let script = r#"read -r line; echo '{"type":"closed"}'"#;
    let (dir, command) = fake_window(&format!("{READY}\n{script}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge.open("https://www.unrealengine.com/linux").unwrap();
    let error = bridge
        .fetch_json(
            "/api/blobs/linux",
            "www.unrealengine.com",
            Duration::from_secs(10),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Closed), "{error:?}");
}

#[tokio::test]
async fn reads_the_auth_code() {
    let (dir, command) = fake_window(&format!("{READY}\n{LOGIN}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge.open("https://www.epicgames.com/id/login").unwrap();
    let code = bridge.wait_auth_code().await.unwrap();
    assert_eq!(code.expose(), "0123456789abcdef0123456789abcdef");
    bridge.close().await;
}

#[tokio::test]
async fn reports_navigation_events() {
    let (dir, command) = fake_window(&format!("{READY}\n{LOGIN}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge
        .sender()
        .open("https://www.epicgames.com/id/login")
        .unwrap();
    let event = bridge.next_event().await.unwrap();
    assert_eq!(
        event,
        Event::Navigated {
            url: "https://www.epicgames.com/id/login".to_owned()
        }
    );
}

#[tokio::test]
async fn waits_for_the_next_page() {
    let script = r#"
read -r line
echo '{"type":"error","message":"ignored"}'
echo '{"type":"navigated","url":"https://www.unrealengine.com/linux"}'
read -r line
"#;
    let (dir, command) = fake_window(&format!("{READY}\n{script}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge.open("https://www.unrealengine.com/linux").unwrap();
    let url = bridge
        .wait_navigation(Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(url.as_str(), "https://www.unrealengine.com/linux");
    let error = bridge
        .wait_navigation(Duration::from_millis(100))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "{error:?}");
}

#[tokio::test]
async fn reads_the_page_links_once_it_has_loaded() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"type":"load"'*)
      echo '{"type":"navigated","url":"https://www.epicgames.com/id/exchange"}'
      echo '{"type":"navigated","url":"https://www.unrealengine.com/linux"}' ;;
    *'"type":"links"'*)
      echo '{"type":"links","links":[{"text":"Download","url":"https://s3.invalid/b?X-Amz-Signature=s"}]}' ;;
  esac
done
"#;
    let (dir, command) = fake_window(&format!("{READY}\n{script}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge
        .load("https://www.epicgames.com/id/exchange")
        .unwrap();
    let links = bridge
        .page_links("www.unrealengine.com", Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].text, "Download");
    assert_eq!(
        links[0].url.expose(),
        "https://s3.invalid/b?X-Amz-Signature=s"
    );
}

#[tokio::test]
async fn closing_before_the_code_is_an_error() {
    let script = r#"read -r line; echo '{"type":"closed"}'"#;
    let (dir, command) = fake_window(&format!("{READY}\n{script}"));
    let mut bridge = spawn(command, &dir.path().join("webview")).await.unwrap();
    bridge.open("https://www.epicgames.com/id/login").unwrap();
    assert!(matches!(bridge.wait_auth_code().await, Err(Error::Closed)));
}

#[tokio::test]
async fn explains_a_window_that_cannot_start() {
    let (dir, command) = fake_window(
        "echo 'unlauncher-web: error while loading shared libraries: libwebkit2gtk-4.1.so.0' >&2\nexit 127",
    );
    let error = spawn(command, &dir.path().join("webview"))
        .await
        .unwrap_err();
    let Error::NotStarted { detail } = &error else {
        panic!("unexpected error: {error:?}");
    };
    assert!(
        detail.as_deref().unwrap().contains("libwebkit2gtk-4.1"),
        "{detail:?}"
    );
    assert!(error.to_string().contains("libwebkit2gtk-4.1"));
}

#[tokio::test]
async fn reports_a_missing_program() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        HostCommand::bundled_in(dir.path(), "unlauncher-web"),
        Err(system::Error::BundledProgramMissing { .. })
    ));
}

#[test]
fn clear_data_removes_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("webview");
    std::fs::create_dir_all(data.join("cookies")).unwrap();
    std::fs::write(data.join("cookies/db"), "x").unwrap();
    clear_data(&data).unwrap();
    assert!(!data.exists());
    clear_data(&data).unwrap();
    assert!(Path::new(dir.path()).exists());
}
