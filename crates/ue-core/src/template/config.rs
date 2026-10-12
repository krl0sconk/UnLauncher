// SPDX-License-Identifier: GPL-3.0-or-later

//! Updating a new project's `Config/*.ini` files for its name.
//!
//! Like the Unreal Editor (`ReplacementsInFiles` for `ini` files in
//! `TemplateDefs.ini`), the template name is replaced by the project name in
//! every `.ini` under `Config`. In the 5.7 Blueprint templates this fixes
//! `GameName=TP_ThirdPersonBP`, paths such as `/Game/TP_TopDownBP/...` and
//! `ActiveGameNameRedirects` targets (`/Script/TP_X`). `DefaultGame.ini` also
//! gets a new `ProjectID`, since several templates ship one and projects must
//! not share it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::registry::random_guid;

/// Section and key of the project id in `DefaultGame.ini`.
const GENERAL_PROJECT_SETTINGS: &str = "/Script/EngineSettings.GeneralProjectSettings";
const PROJECT_ID_KEY: &str = "ProjectID";

const BOM: char = '\u{feff}';

/// Replaces the template name with the project name in every `.ini` file under
/// `config_dir`, and writes a new `ProjectID` to `DefaultGame.ini`.
pub(super) fn rewrite_config(
    config_dir: &Path,
    template_name: &str,
    project_name: &str,
) -> Result<()> {
    fs::create_dir_all(config_dir).map_err(write_err(config_dir))?;
    let mut files = Vec::new();
    collect_ini_files(config_dir, &mut files)?;
    for path in files {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == io::ErrorKind::InvalidData => {
                tracing::warn!(path = %path.display(), "skipping config file that is not UTF-8");
                continue;
            }
            Err(source) => return Err(Error::ReadFile { path, source }),
        };
        let replaced = replace_template_name(&text, template_name, project_name);
        if replaced != text {
            fs::write(&path, replaced).map_err(write_err(&path))?;
        }
    }

    let game_ini = config_dir.join("DefaultGame.ini");
    let text = match fs::read_to_string(&game_ini) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(Error::ReadFile {
                path: game_ini,
                source,
            });
        }
    };
    let project_id: String = random_guid()
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect();
    let updated = set_ini_value(&text, GENERAL_PROJECT_SETTINGS, PROJECT_ID_KEY, &project_id);
    fs::write(&game_ini, updated).map_err(write_err(&game_ini))
}

fn collect_ini_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(read_err(dir))? {
        let path = entry.map_err(read_err(dir))?.path();
        if path.is_dir() {
            collect_ini_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
        {
            files.push(path);
        }
    }
    Ok(())
}

/// Replaces every occurrence of `template` (ignoring ASCII case) with
/// `project`: all-uppercase matches become the uppercase project name,
/// all-lowercase ones the lowercase name, others the name as given. This is
/// Unreal's `%TEMPLATENAME%` replacement, done in one pass so the new name is
/// never matched again.
fn replace_template_name(text: &str, template: &str, project: &str) -> String {
    if template.is_empty() {
        return text.to_owned();
    }
    // ASCII lowercasing keeps byte offsets, so indices into `lower` are valid
    // char boundaries in `text`.
    let lower = text.to_ascii_lowercase();
    let needle = template.to_ascii_lowercase();
    let upper_template = template.to_ascii_uppercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (index, _) in lower.match_indices(&needle) {
        let found = &text[index..index + needle.len()];
        out.push_str(&text[last..index]);
        if found == upper_template && found != template {
            out.push_str(&project.to_ascii_uppercase());
        } else if found == needle && found != template {
            out.push_str(&project.to_ascii_lowercase());
        } else {
            out.push_str(project);
        }
        last = index + needle.len();
    }
    out.push_str(&text[last..]);
    out
}

/// Sets `key=value` in `[section]` of ini `text`, replacing the first existing
/// entry for `key` or adding it after the section header; appends the section
/// if missing. Keeps the BOM, line endings and every other line.
fn set_ini_value(text: &str, section: &str, key: &str, value: &str) -> String {
    let (bom, body) = match text.strip_prefix(BOM) {
        Some(body) => (BOM.to_string(), body),
        None => (String::new(), text),
    };
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = if body.is_empty() {
        Vec::new()
    } else {
        body.strip_suffix(newline)
            .unwrap_or(body)
            .split(newline)
            .map(str::to_owned)
            .collect()
    };
    let entry = format!("{key}={value}");
    let header = lines.iter().position(|line| {
        line.trim()
            .strip_prefix('[')
            .and_then(|l| l.strip_suffix(']'))
            .is_some_and(|name| name.trim() == section)
    });
    if let Some(header) = header {
        let end = lines[header + 1..]
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .map_or(lines.len(), |offset| header + 1 + offset);
        let existing = lines[header + 1..end].iter().position(|line| {
            line.split_once('=')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        });
        match existing {
            Some(offset) => lines[header + 1 + offset] = entry,
            None => lines.insert(header + 1, entry),
        }
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(entry);
    }
    let mut out = bom;
    out.push_str(&lines.join(newline));
    out.push_str(newline);
    out
}

fn read_err(path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::ReadFile { path, source }
}

fn write_err(path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::WriteFile { path, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_template_name_in_any_case() {
        assert_eq!(
            replace_template_name(
                "GameName=TP_ThirdPersonBP\nA=tp_thirdpersonbp B=TP_THIRDPERSONBP C=Tp_ThirdPersonbp",
                "TP_ThirdPersonBP",
                "MyGame"
            ),
            "GameName=MyGame\nA=mygame B=MYGAME C=MyGame"
        );
    }

    #[test]
    fn new_name_containing_the_template_name_is_not_replaced_twice() {
        assert_eq!(
            replace_template_name("X=TP_Blank", "TP_Blank", "TP_Blank2"),
            "X=TP_Blank2"
        );
    }

    #[test]
    fn sets_value_in_existing_section() {
        let text =
            "[/Script/EngineSettings.GeneralProjectSettings]\r\nProjectID=OLD\r\nOther=1\r\n";
        assert_eq!(
            set_ini_value(text, GENERAL_PROJECT_SETTINGS, PROJECT_ID_KEY, "NEW"),
            "[/Script/EngineSettings.GeneralProjectSettings]\r\nProjectID=NEW\r\nOther=1\r\n"
        );
    }

    #[test]
    fn adds_key_after_header_and_keeps_bom() {
        let text = "\u{feff}[A]\nx=1\n[/Script/EngineSettings.GeneralProjectSettings]\nName=Demo\n";
        assert_eq!(
            set_ini_value(text, GENERAL_PROJECT_SETTINGS, PROJECT_ID_KEY, "ID"),
            "\u{feff}[A]\nx=1\n[/Script/EngineSettings.GeneralProjectSettings]\nProjectID=ID\nName=Demo\n"
        );
    }

    #[test]
    fn appends_missing_section() {
        assert_eq!(set_ini_value("", "S", "K", "V"), "[S]\nK=V\n");
        assert_eq!(set_ini_value("\n", "S", "K", "V"), "\n[S]\nK=V\n");
        assert_eq!(
            set_ini_value("[A]\nx=1", "S", "K", "V"),
            "[A]\nx=1\n\n[S]\nK=V\n"
        );
    }
}
