// SPDX-License-Identifier: GPL-3.0-or-later

//! The build pipeline against fake engine scripts (never a real build).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::{Duration, Instant};

use jobs::{Control, Reporter};
use system::Sandbox;
use ue_build::{BuildOptions, Error, run_build};

/// A fake engine root whose scripts print `build_body` from Build.sh.
fn fake_engine(root: &Path, build_body: &str) {
    let write = |rel: &str, body: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/bash\n{body}\n")).unwrap();
    };
    write(
        "Setup.sh",
        "echo 'Updating dependencies:  50% (1/2), 1.0/2.0 MiB | 1.00 MiB/s'\n\
         read -r answer && echo \"stdin was not closed: $answer\"\n\
         echo \"setup args: $#\"",
    );
    write("GenerateProjectFiles.sh", "echo generating >&2");
    write("Engine/Build/BatchFiles/Linux/Build.sh", build_body);
}

fn options(root: &Path, jobs: usize) -> BuildOptions {
    let mut opts = BuildOptions::new(root, root.join("logs/build.log"));
    opts.jobs = Some(jobs);
    opts
}

#[tokio::test]
async fn runs_every_step_and_writes_the_log() {
    let tmp = tempfile::tempdir().unwrap();
    fake_engine(
        tmp.path(),
        "echo \"args: $*\"\necho '[1/2] Compile a.cpp'\necho '[2/2] Link UnrealEditor'",
    );
    let (reporter, handle) = jobs::channel();
    let report = run_build(&options(tmp.path(), 3), Sandbox::Host, &reporter)
        .await
        .unwrap();
    assert_eq!(report.jobs, 3);
    assert_eq!(report.editor, None);
    let log = std::fs::read_to_string(&report.log_path).unwrap();
    assert!(log.contains("Updating dependencies:  50%"));
    assert!(
        log.contains("setup args: 0"),
        "Setup.sh must get no arguments"
    );
    assert!(!log.contains("stdin was not closed"));
    assert!(log.contains("generating"), "stderr is captured");
    assert!(log.contains("args: ShaderCompileWorker Linux Development -MaxParallelActions=3"));
    assert!(log.contains("args: UnrealEditor Linux Development -MaxParallelActions=3"));
    let last = handle.progress();
    assert_eq!(last.stage, "Build UnrealEditor");
    assert_eq!(last.fraction, Some(1.0));
    assert_eq!(last.message.as_deref(), Some("Link UnrealEditor"));
}

#[tokio::test]
async fn failure_reports_stage_code_and_diagnosis() {
    let tmp = tempfile::tempdir().unwrap();
    fake_engine(
        tmp.path(),
        "echo '[1/9] Compile a.cpp'\necho 'error: No space left on device' >&2\nexit 6",
    );
    let err = run_build(
        &options(tmp.path(), 1),
        Sandbox::Host,
        &Reporter::detached(),
    )
    .await
    .unwrap_err();
    match err {
        Error::BuildFailed {
            stage,
            exit_code,
            diagnosis,
            log_path,
        } => {
            assert_eq!(stage, "Build ShaderCompileWorker");
            assert_eq!(exit_code, Some(6));
            assert_eq!(diagnosis[0].id, "disk-full");
            assert!(log_path.is_file());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn killed_step_is_diagnosed_as_out_of_memory() {
    let tmp = tempfile::tempdir().unwrap();
    fake_engine(tmp.path(), "kill -9 $$");
    let err = run_build(
        &options(tmp.path(), 1),
        Sandbox::Host,
        &Reporter::detached(),
    )
    .await
    .unwrap_err();
    let Error::BuildFailed { diagnosis, .. } = err else {
        panic!("unexpected {err:?}");
    };
    assert_eq!(diagnosis[0].id, "out-of-memory");
}

#[tokio::test]
async fn cancel_kills_the_running_step_and_its_children() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("survived");
    fake_engine(
        tmp.path(),
        &format!(
            "echo '[1/2] Compile a.cpp'\n(sleep 3; touch '{}') &\nsleep 30",
            marker.display()
        ),
    );
    let (reporter, handle) = jobs::channel();
    let opts = options(tmp.path(), 1);
    let build = tokio::spawn(async move { run_build(&opts, Sandbox::Host, &reporter).await });
    let mut progress = handle.subscribe();
    while !progress.borrow_and_update().stage.starts_with("Build") {
        progress.changed().await.unwrap();
    }
    let started = Instant::now();
    handle.send(Control::Cancel);
    let err = build.await.unwrap().unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!marker.exists(), "a child of the build survived cancel");
}

#[tokio::test]
async fn missing_script_fails_before_running_anything() {
    let tmp = tempfile::tempdir().unwrap();
    fake_engine(tmp.path(), "echo ok");
    std::fs::remove_file(tmp.path().join("GenerateProjectFiles.sh")).unwrap();
    let err = run_build(
        &options(tmp.path(), 1),
        Sandbox::Host,
        &Reporter::detached(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::MissingScript { .. }));
    assert!(!tmp.path().join("logs/build.log").exists());
}
