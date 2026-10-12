// SPDX-License-Identifier: GPL-3.0-or-later

//! The parser and the known-errors table against log fixtures.

#![allow(clippy::unwrap_used)]

use ue_build::{diagnose, parse_progress};

const SYNTHETIC: &str = include_str!("fixtures/logs/synthetic-failed-build.log");

#[test]
fn synthetic_log_progress_ends_where_the_build_stopped() {
    let last = SYNTHETIC.lines().rev().find_map(parse_progress).unwrap();
    assert!((last.fraction - 0.001).abs() < 1e-6);
    assert_eq!(last.message, "Compile [x64] Module.Engine.1.cpp");
}

#[test]
fn synthetic_log_setup_progress_is_parsed() {
    let fractions: Vec<f32> = SYNTHETIC
        .lines()
        .filter(|l| l.starts_with("Updating dependencies"))
        .filter_map(parse_progress)
        .map(|p| p.fraction)
        .collect();
    assert_eq!(fractions, vec![0.0, 0.5, 1.0]);
}

#[test]
fn synthetic_log_is_diagnosed_as_out_of_memory() {
    let found = diagnose(SYNTHETIC.lines());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].id, "out-of-memory");
    assert_eq!(
        found[0].line,
        "clang++: error: unable to execute command: Killed"
    );
}
