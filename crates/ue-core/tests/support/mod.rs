// SPDX-License-Identifier: GPL-3.0-or-later

//! Helpers shared by the `ue-core` integration tests.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// Path to a file under `tests/fixtures/`.
pub fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

/// Reads a fixture as a string.
pub fn read_fixture(relative: &str) -> String {
    fs::read_to_string(fixture(relative)).unwrap()
}

/// Creates a minimal engine tree at `root`: the editor binary and `Build.version`.
pub fn fake_engine(root: &Path, major: u32, minor: u32, patch: u32) -> PathBuf {
    fake_engine_with_editor(root, "UnrealEditor", major, minor, patch)
}

/// Like [`fake_engine`] but with a custom editor binary name (e.g. `UE4Editor`).
pub fn fake_engine_with_editor(
    root: &Path,
    editor: &str,
    major: u32,
    minor: u32,
    patch: u32,
) -> PathBuf {
    let binaries = root.join("Engine/Binaries/Linux");
    fs::create_dir_all(&binaries).unwrap();
    fs::write(binaries.join(editor), b"").unwrap();
    let build = root.join("Engine/Build");
    fs::create_dir_all(&build).unwrap();
    fs::write(
        build.join("Build.version"),
        format!(
            "{{\n\t\"MajorVersion\": {major},\n\t\"MinorVersion\": {minor},\n\t\"PatchVersion\": {patch},\n\t\"BranchName\": \"++UE5+Release-{major}.{minor}\"\n}}\n"
        ),
    )
    .unwrap();
    root.to_path_buf()
}

/// Creates `<dir>/<name>.uproject` with the given association.
pub fn fake_project(dir: &Path, name: &str, association: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{name}.uproject"));
    fs::write(
        &path,
        format!("{{\n\t\"FileVersion\": 3,\n\t\"EngineAssociation\": \"{association}\"\n}}\n"),
    )
    .unwrap();
    path
}

/// The `.uproject` Unreal 5.7 ships in `Templates/TP_BlankBP`.
pub const TEMPLATE_UPROJECT: &str = "{\n\t\"FileVersion\": 3,\n\t\"EngineAssociation\": \"\",\n\t\"Category\": \"\",\n\t\"Description\": \"\",\n\t\"Modules\": [],\n\t\"Plugins\": [\n\t\t{\n\t\t\t\"Name\": \"ModelingToolsEditorMode\",\n\t\t\t\"Enabled\": true,\n\t\t\t\"TargetAllowList\": [\n\t\t\t\t\"Editor\"\n\t\t\t]\n\t\t}\n\t]\n}";

/// Creates `<engine_root>/Templates/<id>/<uproject_stem>.uproject`, with
/// `Config/TemplateDefs.ini` set to `defs` if given and a `Source/` folder if
/// `has_code`. Returns the template folder.
pub fn fake_template(
    engine_root: &Path,
    id: &str,
    uproject_stem: &str,
    defs: Option<&str>,
    has_code: bool,
) -> PathBuf {
    let dir = engine_root.join("Templates").join(id);
    fs::create_dir_all(dir.join("Config")).unwrap();
    fs::write(
        dir.join(format!("{uproject_stem}.uproject")),
        TEMPLATE_UPROJECT,
    )
    .unwrap();
    if let Some(defs) = defs {
        fs::write(dir.join("Config/TemplateDefs.ini"), defs).unwrap();
    }
    if has_code {
        fs::create_dir_all(dir.join(format!("Source/{uproject_stem}"))).unwrap();
        fs::write(
            dir.join(format!("Source/{uproject_stem}/{uproject_stem}.Build.cs")),
            "",
        )
        .unwrap();
    }
    dir
}

/// Writes `contents` to `root/relative`, creating parent folders.
pub fn write_file(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// Every file under `dir`, as paths relative to it with `/`, sorted.
pub fn files_under(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path.strip_prefix(base).unwrap();
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}
