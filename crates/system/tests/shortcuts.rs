// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for desktop shortcuts. Only the host path is exercised, always in a
//! temporary folder; the portal path needs a real desktop session.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use common::{Overwrite, UserFacing};
use system::{
    APP_ID, DesktopShortcut, Error, Sandbox, desktop_file_id, install_shortcut, remove_shortcut,
    render_desktop_entry,
};
use tempfile::tempdir;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

fn shortcut(exec: &[&str]) -> DesktopShortcut {
    DesktopShortcut {
        id: "5.7.1".to_owned(),
        name: "Unreal Engine 5.7".to_owned(),
        comment: Some("Open Unreal Engine 5.7 with UnLauncher".to_owned()),
        exec: exec.iter().map(|&a| a.to_owned()).collect(),
        icon_png: PNG.to_vec(),
    }
}

fn exec_value(entry: &str) -> &str {
    entry
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .unwrap()
}

#[test]
fn app_id_is_the_flatpak_id() {
    assert_eq!(APP_ID, "io.github.krl0sconk.UnLauncher");
}

#[test]
fn desktop_file_id_has_app_id_prefix_and_sanitized_suffix() {
    assert_eq!(
        desktop_file_id("UE_5.7"),
        "io.github.krl0sconk.UnLauncher.engine_UE_5_7"
    );
    assert_eq!(
        desktop_file_id("{A1-b2} é/x"),
        "io.github.krl0sconk.UnLauncher.engine__A1_b2____x"
    );
}

#[test]
fn renders_a_complete_entry() {
    let entry = render_desktop_entry(
        &shortcut(&["unlauncher", "--launch-engine", "5.7.1"]),
        "/icons/x.png",
    );
    assert_eq!(
        entry,
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.5\n\
         Name=Unreal Engine 5.7\n\
         Comment=Open Unreal Engine 5.7 with UnLauncher\n\
         Exec=unlauncher --launch-engine 5.7.1\n\
         Icon=/icons/x.png\n\
         Terminal=false\n\
         Categories=Development;\n\
         StartupNotify=true\n"
    );
}

#[test]
fn omits_comment_when_absent() {
    let mut s = shortcut(&["unlauncher"]);
    s.comment = None;
    assert!(!render_desktop_entry(&s, "icon").contains("Comment="));
}

#[test]
fn quotes_arguments_with_spaces() {
    let entry = render_desktop_entry(&shortcut(&["/opt/My Engine/run", "a b"]), "i");
    assert_eq!(exec_value(&entry), r#""/opt/My Engine/run" "a b""#);
}

#[test]
fn escapes_quotes_dollar_and_backtick_inside_quotes() {
    // Inside quotes `"`, `$` and `` ` `` get a backslash, and every backslash
    // is then doubled by the general string escape.
    let entry = render_desktop_entry(&shortcut(&["echo", r#"say "hi""#, "$HOME", "`x`"]), "i");
    assert_eq!(
        exec_value(&entry),
        r#"echo "say \\"hi\\"" "\\$HOME" "\\`x\\`""#
    );
}

#[test]
fn literal_backslash_becomes_four_backslashes() {
    let entry = render_desktop_entry(&shortcut(&["prog", r"C:\dir"]), "i");
    assert_eq!(exec_value(&entry), r#"prog "C:\\\\dir""#);
}

#[test]
fn percent_is_doubled_quoted_or_not() {
    let entry = render_desktop_entry(&shortcut(&["prog", "100%", "50% off"]), "i");
    assert_eq!(exec_value(&entry), r#"prog 100%% "50%% off""#);
}

#[test]
fn single_quote_and_empty_argument_are_quoted() {
    let entry = render_desktop_entry(&shortcut(&["prog", "it's", ""]), "i");
    assert_eq!(exec_value(&entry), r#"prog "it's" """#);
}

#[test]
fn newlines_in_name_are_escaped() {
    let mut s = shortcut(&["prog"]);
    s.name = "Line1\nLine2".to_owned();
    let entry = render_desktop_entry(&s, "i");
    assert!(entry.contains("Name=Line1\\nLine2\n"));
}

#[test]
fn installs_desktop_file_and_icon_on_host() {
    let dir = tempdir().unwrap();
    let apps = dir.path().join("share/applications");
    let s = shortcut(&["unlauncher", "--launch-engine", "5.7.1"]);

    install_shortcut(Sandbox::Host, &s, &apps, Overwrite::No).unwrap();

    let id = "io.github.krl0sconk.UnLauncher.engine_5_7_1";
    let icon = dir
        .path()
        .join(format!("share/icons/hicolor/256x256/apps/{id}.png"));
    assert_eq!(fs::read(&icon).unwrap(), PNG);
    let entry = fs::read_to_string(apps.join(format!("{id}.desktop"))).unwrap();
    assert_eq!(entry, render_desktop_entry(&s, icon.to_str().unwrap()));
    // No temporary files are left behind.
    assert_eq!(fs::read_dir(&apps).unwrap().count(), 1);
}

#[test]
fn refuses_to_replace_without_overwrite() {
    let dir = tempdir().unwrap();
    let apps = dir.path().join("applications");
    install_shortcut(Sandbox::Host, &shortcut(&["old"]), &apps, Overwrite::No).unwrap();

    let err =
        install_shortcut(Sandbox::Host, &shortcut(&["new"]), &apps, Overwrite::No).unwrap_err();

    assert!(matches!(&err, Error::ShortcutExists { path } if path.starts_with(&apps)));
    assert!(err.hint().is_some());
    let file = apps.join(format!("{}.desktop", desktop_file_id("5.7.1")));
    assert_eq!(exec_value(&fs::read_to_string(&file).unwrap()), "old");
}

#[test]
fn replaces_with_overwrite_yes() {
    let dir = tempdir().unwrap();
    let apps = dir.path().join("applications");
    install_shortcut(Sandbox::Host, &shortcut(&["old"]), &apps, Overwrite::No).unwrap();
    install_shortcut(Sandbox::Host, &shortcut(&["new"]), &apps, Overwrite::Yes).unwrap();

    let file = apps.join(format!("{}.desktop", desktop_file_id("5.7.1")));
    assert_eq!(exec_value(&fs::read_to_string(&file).unwrap()), "new");
}

#[test]
fn rejects_shortcut_without_command() {
    let dir = tempdir().unwrap();
    let err =
        install_shortcut(Sandbox::Host, &shortcut(&[]), dir.path(), Overwrite::Yes).unwrap_err();
    assert!(matches!(err, Error::ShortcutWithoutCommand { .. }));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn removes_desktop_file_and_icon_on_host() {
    let dir = tempdir().unwrap();
    let apps = dir.path().join("applications");
    install_shortcut(Sandbox::Host, &shortcut(&["x"]), &apps, Overwrite::No).unwrap();

    remove_shortcut(Sandbox::Host, "5.7.1", &apps).unwrap();

    let id = desktop_file_id("5.7.1");
    assert!(!apps.join(format!("{id}.desktop")).exists());
    assert!(
        !dir.path()
            .join(format!("icons/hicolor/256x256/apps/{id}.png"))
            .exists()
    );
}

#[test]
fn removing_a_missing_shortcut_is_not_an_error() {
    let dir = tempdir().unwrap();
    remove_shortcut(Sandbox::Host, "nope", &dir.path().join("applications")).unwrap();
}

#[test]
fn remove_leaves_other_files_alone() {
    let dir = tempdir().unwrap();
    let apps = dir.path().join("applications");
    fs::create_dir_all(&apps).unwrap();
    let other = apps.join("org.example.Other.desktop");
    fs::write(&other, "[Desktop Entry]\n").unwrap();

    remove_shortcut(Sandbox::Host, "5.7.1", &apps).unwrap();

    assert!(Path::new(&other).exists());
}
