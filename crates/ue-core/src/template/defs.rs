// SPDX-License-Identifier: GPL-3.0-or-later

//! Parsing of a template's `Config/TemplateDefs.ini`.

use std::fs;
use std::io;
use std::path::Path;

use crate::error::{Error, Result};

/// Location of the template definition relative to a template folder.
pub(crate) const TEMPLATE_DEFS_PATH: &str = "Config/TemplateDefs.ini";

/// Placeholder Unreal replaces with the template name in ignore lists.
const TEMPLATE_NAME_PLACEHOLDER: &str = "%TEMPLATENAME%";

/// A shared content pack a template pulls in from `Templates/TemplateResources`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentPackRef {
    /// Pack folder name, e.g. `Characters`.
    pub mount_name: String,
    /// Detail levels the pack exists in (`High`, `Standard`), in file order.
    pub detail_levels: Vec<String>,
}

/// The parts of `TemplateDefs.ini` UnLauncher uses.
///
/// Only top-level keys are read: options of template variants (`Variants=`)
/// are ignored, so a project gets the template's default content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TemplateDefs {
    /// English display name (`LocalizedDisplayNames`).
    pub display_name: Option<String>,
    /// English description (`LocalizedDescriptions`).
    pub description: Option<String>,
    /// Folders not copied into a new project, relative to the template.
    pub folders_to_ignore: Vec<String>,
    /// Files not copied into a new project, relative to the template.
    pub files_to_ignore: Vec<String>,
    /// Shared content packs inserted into a new project.
    pub shared_content_packs: Vec<ContentPackRef>,
    /// Category keys (`Categories=`), e.g. `Games` or `AEC`.
    pub categories: Vec<String>,
}

impl TemplateDefs {
    /// Reads `Config/TemplateDefs.ini` of the template at `template_dir`. A
    /// missing file gives empty definitions.
    pub(crate) fn load(template_dir: &Path) -> Result<Self> {
        let path = template_dir.join(TEMPLATE_DEFS_PATH);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(Error::ReadFile { path, source }),
        }
    }

    /// Parses `TemplateDefs.ini` text. Never fails: what it does not
    /// understand is ignored.
    pub(crate) fn parse(text: &str) -> Self {
        let mut defs = Self::default();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with([';', '#', '[']) {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().trim_start_matches('+');
            let value = value.trim();
            match key {
                "LocalizedDisplayNames" => {
                    set_if_english(&mut defs.display_name, value);
                }
                "LocalizedDescriptions" => {
                    set_if_english(&mut defs.description, value);
                }
                "Categories" => defs.categories.extend(
                    value
                        .split(',')
                        .map(unquote)
                        .filter(|category| !category.is_empty()),
                ),
                "FoldersToIgnore" => defs.folders_to_ignore.push(unquote(value)),
                "FilesToIgnore" => defs.files_to_ignore.push(unquote(value)),
                "SharedContentPacks" => {
                    if let Some(mount_name) = struct_field(value, "MountName") {
                        defs.shared_content_packs.push(ContentPackRef {
                            mount_name,
                            detail_levels: list_field(value, "DetailLevels"),
                        });
                    }
                }
                _ => {}
            }
        }
        defs
    }

    /// `folders_to_ignore` with `%TEMPLATENAME%` replaced by `template_name`.
    pub(crate) fn ignored_folders(&self, template_name: &str) -> Vec<String> {
        expand(&self.folders_to_ignore, template_name)
    }

    /// `files_to_ignore` with `%TEMPLATENAME%` replaced by `template_name`.
    pub(crate) fn ignored_files(&self, template_name: &str) -> Vec<String> {
        expand(&self.files_to_ignore, template_name)
    }
}

fn expand(entries: &[String], template_name: &str) -> Vec<String> {
    entries
        .iter()
        .map(|entry| entry.replace(TEMPLATE_NAME_PLACEHOLDER, template_name))
        .collect()
}

/// Stores the `Text` of a `(Language="en", Text="...")` entry if `slot` is empty.
fn set_if_english(slot: &mut Option<String>, value: &str) {
    if slot.is_some() {
        return;
    }
    let is_english =
        struct_field(value, "Language").is_some_and(|lang| lang.eq_ignore_ascii_case("en"));
    if is_english && let Some(text) = struct_field(value, "Text") {
        *slot = Some(text);
    }
}

/// Removes surrounding double quotes, if any.
fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

/// Byte offset just after `name=` inside an Unreal struct literal such as
/// `(Language="en", Text="Blank")`, matching only whole field names.
fn field_start(value: &str, name: &str) -> Option<usize> {
    value.match_indices(name).find_map(|(index, _)| {
        let before = value[..index].trim_end();
        if !(before.ends_with('(') || before.ends_with(',')) {
            return None;
        }
        let rest = value[index + name.len()..].trim_start();
        let rest = rest.strip_prefix('=')?;
        Some(value.len() - rest.len())
    })
}

/// The value of field `name` in an Unreal struct literal: a quoted string
/// (with `\n`, `\"` and `\\` escapes) or a bare token.
fn struct_field(value: &str, name: &str) -> Option<String> {
    let rest = value[field_start(value, name)?..].trim_start();
    let (token, _) = read_token(rest);
    token
}

/// The items of a list field such as `DetailLevels=("High","Standard")`.
fn list_field(value: &str, name: &str) -> Vec<String> {
    let Some(start) = field_start(value, name) else {
        return Vec::new();
    };
    let Some(mut rest) = value[start..].trim_start().strip_prefix('(') else {
        return Vec::new();
    };
    let mut items = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() || rest.starts_with(')') {
            break;
        }
        let (token, remaining) = read_token(rest);
        if let Some(token) = token.filter(|t| !t.is_empty()) {
            items.push(token);
        }
        rest = remaining.trim_start();
        match rest.strip_prefix(',') {
            Some(after) => rest = after,
            None => break,
        }
    }
    items
}

/// Reads one quoted string or bare token from the start of `text`; returns it
/// and the remaining text.
fn read_token(text: &str) -> (Option<String>, &str) {
    let Some(quoted) = text.strip_prefix('"') else {
        let end = text.find([',', ')']).unwrap_or(text.len());
        return (Some(text[..end].trim().to_owned()), &text[end..]);
    };
    let mut out = String::new();
    let mut chars = quoted.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return (Some(out), &quoted[index + 1..]),
            '\\' => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, other)) => out.push(other),
                None => break,
            },
            other => out.push(other),
        }
    }
    // Unterminated string.
    (None, "")
}

/// A template category from `Templates/TemplateCategories.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Category {
    /// The key templates use, e.g. `AEC`.
    pub key: String,
    /// English display name, e.g. `Architecture`.
    pub name: String,
}

/// Parses `TemplateCategories.ini` text, keeping the file order. Never fails.
pub(crate) fn parse_categories(text: &str) -> Vec<Category> {
    text.lines()
        .map(str::trim)
        .filter_map(|line| line.trim_start_matches('+').strip_prefix("Categories="))
        .filter_map(|value| {
            let key = quoted_after(value, "Key=\"")?;
            let name = quoted_after(value, "(Language=\"en\",Text=\"")
                .map_or_else(|| key.clone(), |name| name.replace("\\n", " "));
            Some(Category { key, name })
        })
        .collect()
}

/// The text between `prefix` and the next `"` in `value`.
fn quoted_after(value: &str, prefix: &str) -> Option<String> {
    let start = value.find(prefix)? + prefix.len();
    let end = value[start..].find('"')?;
    Some(value[start..start + end].to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_struct_fields_with_and_without_spaces() {
        let value = r#"(Language="en", Text="Blank")"#;
        assert_eq!(struct_field(value, "Language").unwrap(), "en");
        assert_eq!(struct_field(value, "Text").unwrap(), "Blank");
        let value = r#"(Language="en",Text="Third Person")"#;
        assert_eq!(struct_field(value, "Text").unwrap(), "Third Person");
    }

    #[test]
    fn unescapes_quoted_text() {
        let value = r#"(Language="en", Text="One.\n\nSay \"hi\".")"#;
        assert_eq!(struct_field(value, "Text").unwrap(), "One.\n\nSay \"hi\".");
    }

    #[test]
    fn matches_whole_field_names_only() {
        let value = r#"(MyText="no", Text="yes")"#;
        assert_eq!(struct_field(value, "Text").unwrap(), "yes");
    }

    #[test]
    fn reads_list_fields() {
        let value = r#"(MountName="Characters",DetailLevels=("High", Standard))"#;
        assert_eq!(list_field(value, "DetailLevels"), vec!["High", "Standard"]);
        assert_eq!(struct_field(value, "MountName").unwrap(), "Characters");
    }

    #[test]
    fn ignores_commented_and_variant_entries() {
        let defs = TemplateDefs::parse(
            ";SharedContentPacks=(MountName=\"Variant_Combat\",DetailLevels=(\"Standard\"))\n\
             Variants=(Name=\"Combat\",SharedContentPacks=((DetailLevels=(Standard),MountName=\"Variant_Combat\")))\n\
             SharedContentPacks=(MountName=\"Input\",DetailLevels=(\"High\"))\n",
        );
        assert_eq!(
            defs.shared_content_packs,
            vec![ContentPackRef {
                mount_name: "Input".to_owned(),
                detail_levels: vec!["High".to_owned()],
            }]
        );
    }

    #[test]
    fn reads_categories() {
        let defs = TemplateDefs::parse("Categories=AEC\n+Categories=\"Games\"\n");
        assert_eq!(defs.categories, ["AEC", "Games"]);
    }

    #[test]
    fn parses_category_names() {
        let text = concat!(
            "; comment\n",
            "Categories=(Key=\"Games\", LocalizedDisplayNames=((Language=\"en\",Text=\"Games\"),",
            "(Language=\"ja\",Text=\"x\")), IsMajorCategory=true)\n",
            "Categories=(Key=\"ME\", LocalizedDisplayNames=((Language=\"en\",Text=\"Film / Video &\\nLive Events\")))\n",
            "Categories=(Key=\"X\")\n",
        );
        let categories = parse_categories(text);
        let names: Vec<_> = categories
            .iter()
            .map(|c| (c.key.as_str(), c.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("Games", "Games"),
                ("ME", "Film / Video & Live Events"),
                ("X", "X")
            ]
        );
    }
}
