// SPDX-License-Identifier: GPL-3.0-or-later

use egs_api::api::types::fab_library::FabAsset;
use serde::{Deserialize, Serialize};
use ue_core::EngineVersion;

/// What a Fab asset contains, which decides where it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    /// A code or content plugin (`<Name>.uplugin`).
    Plugin,
    /// A content pack (`Content/…`).
    Content,
    /// A complete project (`<Name>.uproject`).
    Project,
    /// Not declared; decided from the downloaded files.
    Unknown,
}

/// One downloadable build of an asset, valid for some engine versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetVersion {
    /// Artifact id Fab uses to deliver this build.
    pub id: String,
    /// Compatible engine versions as `(major, minor)`, sorted, no duplicates.
    pub engine_versions: Vec<(u32, u32)>,
    /// Short label for the interface, e.g. `"UE 5.3, 5.4"`.
    pub title: Option<String>,
}

/// An asset in the user's Fab library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryAsset {
    /// Asset (catalog item) id.
    pub id: String,
    /// Catalog namespace of the asset; needed to request its manifest.
    pub namespace: String,
    /// Title shown on Fab.
    pub title: String,
    /// Declared kind.
    pub kind: AssetKind,
    /// Downloadable builds.
    pub versions: Vec<AssetVersion>,
    /// Thumbnail image, if Fab lists one.
    pub thumbnail_url: Option<String>,
}

/// The build of `asset` made for `engine`'s `major.minor`, if any (the
/// patch number is ignored).
#[must_use]
pub fn compatible_version(asset: &LibraryAsset, engine: EngineVersion) -> Option<&AssetVersion> {
    asset.versions.iter().find(|version| {
        version
            .engine_versions
            .contains(&(engine.major, engine.minor))
    })
}

/// The build to install into `engine` when there may be no exact match, as
/// the Epic launcher allows: the compatible build if there is one; otherwise
/// the build for the newest engine older than `engine`; otherwise the build for
/// the oldest newer engine. The flag says whether the build is compatible.
/// `None` only if the asset has no builds.
#[must_use]
pub fn closest_version(
    asset: &LibraryAsset,
    engine: EngineVersion,
) -> Option<(&AssetVersion, bool)> {
    if let Some(exact) = compatible_version(asset, engine) {
        return Some((exact, true));
    }
    let target = (engine.major, engine.minor);
    let candidates = || {
        asset.versions.iter().flat_map(|version| {
            version
                .engine_versions
                .iter()
                .map(move |supported| (*supported, version))
        })
    };
    let older = candidates()
        .filter(|(supported, _)| *supported < target)
        .max_by_key(|(supported, _)| *supported);
    let newer = candidates()
        .filter(|(supported, _)| *supported > target)
        .min_by_key(|(supported, _)| *supported);
    older
        .or(newer)
        .map(|(_, version)| version)
        .or_else(|| asset.versions.first())
        .map(|version| (version, false))
}

/// Converts a Fab library entry from `egs-api`.
pub(crate) fn from_fab(asset: &FabAsset) -> LibraryAsset {
    let versions = asset
        .project_versions
        .iter()
        .map(|project| {
            let mut engine_versions: Vec<(u32, u32)> = project
                .engine_versions
                .iter()
                .filter_map(|text| parse_engine_version(text))
                .collect();
            engine_versions.sort_unstable();
            engine_versions.dedup();
            let title = (!engine_versions.is_empty()).then(|| {
                let list: Vec<String> = engine_versions
                    .iter()
                    .map(|(major, minor)| format!("{major}.{minor}"))
                    .collect();
                format!("UE {}", list.join(", "))
            });
            AssetVersion {
                id: project.artifact_id.clone(),
                engine_versions,
                title,
            }
        })
        .collect();
    let thumbnail_url = asset
        .images
        .iter()
        .find(|image| image.type_field.eq_ignore_ascii_case("thumbnail"))
        .or_else(|| asset.images.first())
        .map(|image| image.url.clone())
        .filter(|url| !url.is_empty());
    LibraryAsset {
        id: asset.asset_id.clone(),
        namespace: asset.asset_namespace.clone(),
        title: asset.title.clone(),
        kind: kind_from_distribution(&asset.distribution_method),
        versions,
        thumbnail_url,
    }
}

/// Maps Epic's `distributionMethod`.
fn kind_from_distribution(method: &str) -> AssetKind {
    match method.to_ascii_uppercase().as_str() {
        "CODE_PLUGIN" | "ENGINE_PLUGIN" | "PLUGIN" => AssetKind::Plugin,
        "ASSET_PACK" | "CONTENT" => AssetKind::Content,
        "COMPLETE_PROJECT" | "PROJECT" => AssetKind::Project,
        _ => AssetKind::Unknown,
    }
}

/// Parses `"UE_5.3"`, `"5.3"` or `"5.3.2"` into `(5, 3)`.
fn parse_engine_version(text: &str) -> Option<(u32, u32)> {
    let digits = text
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit());
    let mut parts = digits.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.trim().parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use egs_api::api::types::fab_library::FabLibrary;

    use super::*;

    /// Hand-made fixture shaped like `GET /e/accounts/<id>/ue/library`.
    const LIBRARY: &str = include_str!("../tests/fixtures/library_page.json");

    fn library() -> Vec<LibraryAsset> {
        let library: FabLibrary = serde_json::from_str(LIBRARY).unwrap();
        library.results.iter().map(from_fab).collect()
    }

    #[test]
    fn parses_engine_versions() {
        assert_eq!(parse_engine_version("UE_5.3"), Some((5, 3)));
        assert_eq!(parse_engine_version("5.4.1"), Some((5, 4)));
        assert_eq!(parse_engine_version("UE_5"), None);
        assert_eq!(parse_engine_version("garbage"), None);
    }

    #[test]
    fn converts_a_plugin() {
        let assets = library();
        let plugin = &assets[0];
        assert_eq!(plugin.id, "a1b2c3d4e5f60718293a4b5c6d7e8f90");
        assert_eq!(plugin.namespace, "ue");
        assert_eq!(plugin.title, "Handy Plugin");
        assert_eq!(plugin.kind, AssetKind::Plugin);
        assert_eq!(plugin.versions.len(), 2);
        assert_eq!(plugin.versions[0].id, "HandyPlugin53");
        assert_eq!(plugin.versions[0].engine_versions, vec![(5, 2), (5, 3)]);
        assert_eq!(plugin.versions[0].title.as_deref(), Some("UE 5.2, 5.3"));
        assert_eq!(
            plugin.thumbnail_url.as_deref(),
            Some("https://media.example.invalid/handy-thumb.png")
        );
    }

    #[test]
    fn converts_content_with_null_lists() {
        let assets = library();
        let pack = &assets[1];
        assert_eq!(pack.kind, AssetKind::Content);
        assert!(pack.versions.is_empty());
        assert_eq!(pack.thumbnail_url, None);
        assert_eq!(assets[2].kind, AssetKind::Project);
        assert_eq!(assets[3].kind, AssetKind::Unknown);
    }

    #[test]
    fn picks_compatible_version() {
        let assets = library();
        let plugin = &assets[0];
        let v = |major, minor, patch| EngineVersion::new(major, minor, patch);
        assert_eq!(
            compatible_version(plugin, v(5, 3, 2)).map(|x| x.id.as_str()),
            Some("HandyPlugin53")
        );
        assert_eq!(
            compatible_version(plugin, v(5, 5, 0)).map(|x| x.id.as_str()),
            Some("HandyPlugin55")
        );
        assert!(compatible_version(plugin, v(5, 1, 0)).is_none());
        assert!(compatible_version(plugin, v(4, 27, 2)).is_none());
    }

    #[test]
    fn closest_version_falls_back_like_the_epic_launcher() {
        let assets = library();
        let plugin = &assets[0];
        let v = |major, minor| EngineVersion::new(major, minor, 0);
        let pick =
            |engine| closest_version(plugin, engine).map(|(x, exact)| (x.id.as_str(), exact));
        assert_eq!(pick(v(5, 3)), Some(("HandyPlugin53", true)));
        // Newer engine than any build: the newest older build.
        assert_eq!(pick(v(5, 9)), Some(("HandyPlugin55", false)));
        // 5.4 is listed by the second build.
        assert_eq!(pick(v(5, 4)), Some(("HandyPlugin55", true)));
        // Older engine than any build: the oldest newer one.
        assert_eq!(pick(v(4, 27)), Some(("HandyPlugin53", false)));
        // No builds at all.
        assert!(closest_version(&assets[1], v(5, 7)).is_none());
    }
}
