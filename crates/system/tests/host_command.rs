// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `HostCommand`. They inspect the built command; only the last one
//! starts a process, and only the harmless `true`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use system::{Error, HostCommand, Sandbox};

fn sample() -> HostCommand {
    HostCommand::new("/opt/UE/Engine/Binaries/Linux/UnrealEditor")
        .arg("/home/me/My Game/Game.uproject")
        .args(["-log", "-stdout"])
        .current_dir("/home/me/My Game")
        .env("UE_FLAG", "a b")
}

fn args(command: &std::process::Command) -> Vec<&OsStr> {
    command.get_args().collect()
}

#[test]
fn runs_directly_on_host() {
    let command = sample().to_command(Sandbox::Host);
    assert_eq!(
        command.get_program(),
        "/opt/UE/Engine/Binaries/Linux/UnrealEditor"
    );
    assert_eq!(
        args(&command),
        ["/home/me/My Game/Game.uproject", "-log", "-stdout"]
    );
    assert_eq!(
        command.get_current_dir(),
        Some(Path::new("/home/me/My Game"))
    );
    let envs: Vec<_> = command.get_envs().collect();
    assert_eq!(envs, [(OsStr::new("UE_FLAG"), Some(OsStr::new("a b")))]);
}

#[test]
fn goes_through_flatpak_spawn_in_flatpak() {
    let command = sample().to_command(Sandbox::Flatpak);
    assert_eq!(command.get_program(), "flatpak-spawn");
    assert_eq!(
        args(&command),
        [
            "--host",
            "--directory=/home/me/My Game",
            "--env=UE_FLAG=a b",
            "/opt/UE/Engine/Binaries/Linux/UnrealEditor",
            "/home/me/My Game/Game.uproject",
            "-log",
            "-stdout",
        ]
    );
    // Directory and environment go to the host process, not to flatpak-spawn.
    assert_eq!(command.get_current_dir(), None);
    assert_eq!(command.get_envs().count(), 0);
}

#[test]
fn minimal_command_in_flatpak() {
    let command = HostCommand::new("git").to_command(Sandbox::Flatpak);
    assert_eq!(args(&command), ["--host", "git"]);
}

#[test]
fn spawns_a_process_on_host() {
    let status = HostCommand::new("true")
        .spawn_in(Sandbox::Host)
        .unwrap()
        .wait()
        .unwrap();
    assert!(status.success());
}

#[test]
fn reports_missing_program() {
    let err = HostCommand::new("/nonexistent/unlauncher-test-program")
        .spawn_in(Sandbox::Host)
        .unwrap_err();
    assert!(
        matches!(err, Error::SpawnFailed { program, .. } if program == "/nonexistent/unlauncher-test-program")
    );
}

#[test]
fn spawns_a_detached_process_on_host() {
    HostCommand::new("true")
        .spawn_detached_in(Sandbox::Host)
        .unwrap();
}

#[test]
fn waits_and_reports_the_exit_status() {
    assert!(
        HostCommand::new("true")
            .wait_quiet_in(Sandbox::Host)
            .unwrap()
            .success()
    );
    let failed = HostCommand::new("sh")
        .args(["-c", "exit 3"])
        .wait_quiet_in(Sandbox::Host)
        .unwrap();
    assert_eq!(failed.code(), Some(3));
}

#[test]
fn collects_output_and_feeds_stdin() {
    let output = HostCommand::new("sh")
        .args(["-c", "read line; echo \"got $line\"; echo oops >&2; exit 2"])
        .output_in(Sandbox::Host, b"hello\n", Duration::from_secs(10))
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, b"got hello\n");
    assert_eq!(output.stderr, b"oops\n");
}

#[test]
fn stops_a_command_that_takes_too_long() {
    let err = HostCommand::new("sleep")
        .arg("30")
        .output_in(Sandbox::Host, b"", Duration::from_millis(100))
        .unwrap_err();
    assert!(
        matches!(&err, Error::TimedOut { program } if program == "sleep"),
        "{err:?}"
    );
}

#[test]
fn reports_missing_program_when_waiting() {
    let err = HostCommand::new("/nonexistent/unlauncher-test-program")
        .wait_quiet_in(Sandbox::Host)
        .unwrap_err();
    assert!(matches!(err, Error::SpawnFailed { .. }));
}

#[test]
fn reports_missing_program_when_detached() {
    let err = HostCommand::new("/nonexistent/unlauncher-test-program")
        .spawn_detached_in(Sandbox::Host)
        .unwrap_err();
    assert!(matches!(err, Error::SpawnFailed { .. }));
}

/// A folder with an executable file named `name`, standing in for UnLauncher's
/// install folder.
fn install_dir(name: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

#[test]
fn bundled_never_uses_flatpak_spawn() {
    let dir = install_dir("unlauncher-web");
    let bundled = HostCommand::bundled_in(dir.path(), "unlauncher-web")
        .unwrap()
        .arg("--data-dir")
        .arg("/tmp/web");
    assert!(bundled.is_bundled());
    for sandbox in [Sandbox::Host, Sandbox::Flatpak] {
        let command = bundled.to_command(sandbox);
        assert_eq!(command.get_program(), dir.path().join("unlauncher-web"));
        assert_eq!(args(&command), ["--data-dir", "/tmp/web"]);
    }
}

#[test]
fn bundled_runs_in_flatpak_sandbox_mode() {
    let dir = install_dir("unlauncher-web");
    let command = HostCommand::bundled_in(dir.path(), "unlauncher-web").unwrap();
    // Another test thread may fork while the script is being written; the
    // child keeps it "busy" until it execs (rust-lang/rust#114554).
    let mut child = None;
    for _ in 0..50 {
        match command.spawn_in(Sandbox::Flatpak) {
            Err(Error::SpawnFailed { source, .. })
                if source.kind() == std::io::ErrorKind::ExecutableFileBusy =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => {
                child = Some(result.unwrap());
                break;
            }
        }
    }
    let status = child.unwrap().wait().unwrap();
    assert!(status.success());
}

#[test]
fn host_commands_are_not_bundled() {
    assert!(!HostCommand::new("git").is_bundled());
}

#[test]
fn bundled_reports_a_missing_program() {
    let dir = tempfile::tempdir().unwrap();
    let err = HostCommand::bundled_in(dir.path(), "unlauncher-web").unwrap_err();
    assert!(
        matches!(&err, Error::BundledProgramMissing { program, path }
            if program == "unlauncher-web" && path == &dir.path().join("unlauncher-web")),
        "{err:?}"
    );
}

#[test]
fn bundled_rejects_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("unlauncher-web")).unwrap();
    assert!(HostCommand::bundled_in(dir.path(), "unlauncher-web").is_err());
}
