// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for finding the editor's log and diagnosing why it closed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use ue_core::{
    CrashKind, EngineVersion, diagnose_crash, engine_log_path, project_log_path, read_crash_cause,
};

const GPU_LOST: &str = include_str!("fixtures/editor_logs/gpu_device_lost.log");
const NORMAL: &str = include_str!("fixtures/editor_logs/normal_exit.log");
const FATAL: &str = include_str!("fixtures/editor_logs/synthetic_fatal.log");
const OOM: &str = include_str!("fixtures/editor_logs/synthetic_oom.log");

#[test]
fn recognizes_a_lost_gpu() {
    let cause = diagnose_crash(GPU_LOST).unwrap();
    assert_eq!(cause.kind, CrashKind::GpuLost);
    assert!(
        cause.line.contains("VK_ERROR_DEVICE_LOST"),
        "{}",
        cause.line
    );
}

#[test]
fn a_normal_exit_has_no_cause() {
    assert_eq!(diagnose_crash(NORMAL), None);
}

#[test]
fn reports_the_exception_of_a_fatal_error() {
    let cause = diagnose_crash(FATAL).unwrap();
    assert_eq!(cause.kind, CrashKind::Fatal);
    assert!(
        cause.line.starts_with("Unhandled Exception: SIGSEGV"),
        "{}",
        cause.line
    );
}

#[test]
fn recognizes_running_out_of_memory() {
    let cause = diagnose_crash(OOM).unwrap();
    assert_eq!(cause.kind, CrashKind::OutOfMemory);
    assert!(cause.line.contains("Ran out of memory"), "{}", cause.line);
}

#[test]
fn log_paths() {
    assert_eq!(
        project_log_path(Path::new("/data/Games/Kart/Kart.uproject")),
        Path::new("/data/Games/Kart/Saved/Logs/Kart.log")
    );
    assert_eq!(
        engine_log_path(
            Path::new("/home/me/.config/Epic/UnrealEngine"),
            EngineVersion::new(5, 8, 3)
        ),
        Path::new("/home/me/.config/Epic/UnrealEngine/5.8/Saved/Logs/Unreal.log")
    );
}

#[test]
fn reads_a_log_file_and_ignores_a_missing_one() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("Game.log");
    std::fs::write(&log, GPU_LOST).unwrap();
    assert_eq!(read_crash_cause(&log).unwrap().kind, CrashKind::GpuLost);
    assert_eq!(read_crash_cause(&dir.path().join("missing.log")), None);
}
