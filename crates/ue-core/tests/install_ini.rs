// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for parsing and editing `Install.ini` text.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use support::read_fixture;
use ue_core::InstallIni;

const FIXTURES: [&str; 3] = [
    "install_ini/real.ini",
    "install_ini/with_extras.ini",
    "install_ini/crlf.ini",
];

#[test]
fn reads_installations_from_real_file() {
    let ini = InstallIni::parse(&read_fixture("install_ini/real.ini"));
    assert_eq!(
        ini.installations(),
        vec![(
            "UE_5.7".to_owned(),
            "/mnt/games/UE5/Linux_Unreal_Engine_5.7.4".to_owned()
        )]
    );
}

#[test]
fn ignores_entries_outside_installations() {
    let ini = InstallIni::parse(&read_fixture("install_ini/with_extras.ini"));
    let keys: Vec<_> = ini.installations().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["UE_5.7", "{8A9F2E21-4B6C-4D1E-9F3A-7C2B1D0E5F64}"]);
}

#[test]
fn render_without_changes_is_byte_identical() {
    for name in FIXTURES {
        let text = read_fixture(name);
        assert_eq!(InstallIni::parse(&text).render(), text, "{name}");
    }
}

#[test]
fn set_appends_new_entry_inside_installations_section() {
    let mut ini = InstallIni::parse(&read_fixture("install_ini/with_extras.ini"));
    ini.set("UE_5.8", "/opt/UnrealEngine/5.8");
    let expected = "\
; Managed by hand, do not remove this comment
[Installations]
UE_5.7=/opt/UnrealEngine/5.7
{8A9F2E21-4B6C-4D1E-9F3A-7C2B1D0E5F64}=/home/dev/src/UnrealEngine
UE_5.8=/opt/UnrealEngine/5.8
# a hash comment
this line has no equals sign

[SomeOtherTool]
Key=Value
Path = /somewhere with spaces
";
    assert_eq!(ini.render(), expected);
}

#[test]
fn set_replaces_existing_value_in_place() {
    let mut ini = InstallIni::parse(&read_fixture("install_ini/with_extras.ini"));
    ini.set("UE_5.7", "/new/path");
    let rendered = ini.render();
    assert!(rendered.contains("[Installations]\nUE_5.7=/new/path\n{8A9F"));
    assert!(!rendered.contains("/opt/UnrealEngine/5.7"));
}

#[test]
fn remove_keeps_every_other_line() {
    let text = read_fixture("install_ini/with_extras.ini");
    let mut ini = InstallIni::parse(&text);
    assert!(ini.remove("{8A9F2E21-4B6C-4D1E-9F3A-7C2B1D0E5F64}"));
    assert!(!ini.remove("UE_9.9"));
    let expected = text.replace(
        "{8A9F2E21-4B6C-4D1E-9F3A-7C2B1D0E5F64}=/home/dev/src/UnrealEngine\n",
        "",
    );
    assert_eq!(ini.render(), expected);
}

#[test]
fn set_then_remove_restores_original() {
    for name in FIXTURES {
        let text = read_fixture(name);
        let mut ini = InstallIni::parse(&text);
        ini.set("UE_TEMP", "/tmp/engine");
        assert!(ini.remove("UE_TEMP"));
        assert_eq!(ini.render(), text, "{name}");
    }
}

#[test]
fn set_on_empty_file_creates_section() {
    let mut ini = InstallIni::parse("");
    ini.set("UE_5.7", "/opt/ue");
    assert_eq!(ini.render(), "[Installations]\nUE_5.7=/opt/ue\n");
}

#[test]
fn set_without_section_appends_one() {
    let mut ini = InstallIni::parse("[Other]\nA=B");
    ini.set("UE_5.7", "/opt/ue");
    assert_eq!(
        ini.render(),
        "[Other]\nA=B\n\n[Installations]\nUE_5.7=/opt/ue\n"
    );
}

#[test]
fn keeps_crlf_line_endings_when_editing() {
    let mut ini = InstallIni::parse(&read_fixture("install_ini/crlf.ini"));
    ini.set("UE_5.8", "/opt/ue58");
    assert_eq!(
        ini.render(),
        "[Installations]\r\nUE_5.7=/opt/UnrealEngine/5.7\r\nUE_5.8=/opt/ue58\r\n"
    );
}
