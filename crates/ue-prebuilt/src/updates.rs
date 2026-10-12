// SPDX-License-Identifier: GPL-3.0-or-later

//! Telling the user when a newer Unreal Engine is out, from Fab's public list
//! of engine versions.
//!
//! fab.com sits behind Cloudflare like unrealengine.com, so the interface
//! reads [`FAB_VERSIONS_PATH`] from inside the (hidden) web window and hands
//! the body to [`fab_versions_from_json`].

use ue_core::EngineVersion;

use crate::error::{Error, Result};

/// The page the web window loads before asking for the versions.
pub const FAB_HOME_URL: &str = "https://www.fab.com/";

/// Fab's public list of engine versions, relative to [`FAB_HOME_URL`].
pub const FAB_VERSIONS_PATH: &str = "/i/unreal-engine/versions";

/// Reads the answer of [`FAB_VERSIONS_PATH`]: `{"results": ["UE_4.0", …,
/// "UE_5.8"]}` (a bare list is accepted too).
///
/// # Errors
///
/// [`Error::UnexpectedVersions`] if the body has another shape.
pub fn fab_versions_from_json(body: &str) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Answer {
        Wrapped { results: Vec<String> },
        Bare(Vec<String>),
    }
    match serde_json::from_str::<Answer>(body) {
        Ok(Answer::Wrapped { results } | Answer::Bare(results)) => Ok(results),
        Err(_) => Err(Error::UnexpectedVersions),
    }
}

/// The newest `major.minor` in `versions` if it is newer than every installed
/// engine (or if none is installed), else `None`.
///
/// Accepts `"5.7"`, `"UE_5.7"`, `"UE 5.7"` and `"5.7.4"`; skips anything
/// else. Patch versions are ignored: Fab lists `major.minor` only.
#[must_use]
pub fn newer_than_installed(versions: &[String], installed: &[EngineVersion]) -> Option<String> {
    let newest = versions.iter().filter_map(|v| major_minor(v)).max()?;
    let installed_max = installed.iter().map(|v| (v.major, v.minor)).max();
    match installed_max {
        Some(current) if current >= newest => None,
        _ => Some(format!("{}.{}", newest.0, newest.1)),
    }
}

/// `(major, minor)` from a version name, or `None`.
fn major_minor(name: &str) -> Option<(u32, u32)> {
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let mut parts = name[start..].split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((major, minor))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_fab_answers() {
        let body = r#"{"results":["UE_4.27","UE_5.0","UE_5.7","UE_5.8"]}"#;
        assert_eq!(
            fab_versions_from_json(body).unwrap(),
            ["UE_4.27", "UE_5.0", "UE_5.7", "UE_5.8"]
        );
        assert_eq!(fab_versions_from_json(r#"["5.7"]"#).unwrap(), ["5.7"]);
        assert!(matches!(
            fab_versions_from_json("<html>Just a moment</html>"),
            Err(Error::UnexpectedVersions)
        ));
        assert!(matches!(
            fab_versions_from_json(r#"{"error":"x"}"#),
            Err(Error::UnexpectedVersions)
        ));
    }

    #[test]
    fn reads_major_and_minor() {
        assert_eq!(major_minor("5.7"), Some((5, 7)));
        assert_eq!(major_minor("UE_5.10"), Some((5, 10)));
        assert_eq!(major_minor("5.7.4"), Some((5, 7)));
        assert_eq!(major_minor("5.7-preview"), Some((5, 7)));
        assert_eq!(major_minor("5"), None);
        assert_eq!(major_minor("UE"), None);
    }
}
