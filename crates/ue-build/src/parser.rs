// SPDX-License-Identifier: GPL-3.0-or-later

//! Progress parsing of build output.

use std::sync::OnceLock;

use regex::Regex;

/// Progress information found in one output line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineProgress {
    /// Completion in `0.0..=1.0`.
    pub fraction: f32,
    /// What is being done, e.g. `"Compile [x64] Module.Core.cpp"`.
    pub message: String,
}

/// `UnrealBuildTool`'s action lines: `[12/3456] Compile [x64] Module.Core.cpp`.
fn ubt_action() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*\[(\d+)/(\d+)\]\s*(.*?)\s*$").ok())
        .as_ref()
}

/// `GitDependencies` (run by `Setup.sh`):
/// `Updating dependencies:  45% (12345/27500), 1234.5/2700.0 MiB | 25.00 MiB/s`.
fn git_dependencies() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(Updating dependencies):\s*(\d{1,3})%(.*?)\s*$").ok())
        .as_ref()
}

/// Extracts progress from one line of `Setup.sh` or `Build.sh` output.
///
/// Recognises `UnrealBuildTool`'s `[n/total]` lines and `GitDependencies`'
/// `Updating dependencies: NN%` lines. Returns `None` for any other line.
#[must_use]
pub fn parse_progress(line: &str) -> Option<LineProgress> {
    if let Some(caps) = ubt_action().and_then(|re| re.captures(line)) {
        let done: u64 = caps.get(1)?.as_str().parse().ok()?;
        let total: u64 = caps.get(2)?.as_str().parse().ok()?;
        if total == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let fraction = (done as f64 / total as f64).clamp(0.0, 1.0) as f32;
        return Some(LineProgress {
            fraction,
            message: caps.get(3).map_or("", |m| m.as_str()).to_owned(),
        });
    }
    if let Some(caps) = git_dependencies().and_then(|re| re.captures(line)) {
        let percent: u16 = caps.get(2)?.as_str().parse().ok()?;
        let detail = caps
            .get(3)
            .map_or("", |m| m.as_str())
            .trim_start_matches([',', ' ']);
        let message = if detail.is_empty() {
            format!("Updating dependencies: {percent}%")
        } else {
            format!("Updating dependencies: {percent}% {detail}")
        };
        return Some(LineProgress {
            fraction: f32::from(percent.min(100)) / 100.0,
            message,
        });
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn parses_ubt_action_lines() {
        let p = parse_progress("[25/100] Compile [x64] Module.Core.1.cpp").unwrap();
        assert_eq!(p.fraction, 0.25);
        assert_eq!(p.message, "Compile [x64] Module.Core.1.cpp");
    }

    #[test]
    fn parses_git_dependencies_percentage() {
        let p = parse_progress(
            "Updating dependencies:  45% (12345/27500), 1234.5/2700.0 MiB | 25.00 MiB/s",
        )
        .unwrap();
        assert_eq!(p.fraction, 0.45);
        assert!(
            p.message
                .starts_with("Updating dependencies: 45% (12345/27500)")
        );
    }

    #[test]
    fn ignores_other_lines() {
        assert_eq!(parse_progress("Running UnrealHeaderTool"), None);
        assert_eq!(parse_progress("[0/0] nothing"), None);
        assert_eq!(parse_progress("array[1/2] = 3"), None);
    }

    #[test]
    fn clamps_overshoot() {
        assert_eq!(parse_progress("[5/4] Link").unwrap().fraction, 1.0);
    }
}
