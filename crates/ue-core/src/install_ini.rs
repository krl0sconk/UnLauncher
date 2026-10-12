// SPDX-License-Identifier: GPL-3.0-or-later

//! Lossless editing of `Install.ini`.

/// Section that lists engine installations.
const INSTALLATIONS: &str = "Installations";

/// The text of an `Install.ini` file, kept line by line.
///
/// Only `<id>=<path>` lines inside `[Installations]` are interpreted. Every
/// other line (comments, other sections, lines Unreal would ignore) is kept as
/// is, together with line endings and the final newline, so
/// `InstallIni::parse(text).render() == text` and unrelated content survives
/// edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallIni {
    lines: Vec<Line>,
    newline: &'static str,
    trailing_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Section {
        name: String,
        raw: String,
    },
    Entry {
        key: String,
        value: String,
        raw: String,
    },
    Other(String),
}

impl Line {
    fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Self::Section {
                name: name.trim().to_owned(),
                raw: raw.to_owned(),
            }
        } else if trimmed.is_empty() || trimmed.starts_with([';', '#']) {
            Self::Other(raw.to_owned())
        } else if let Some((key, value)) = trimmed.split_once('=') {
            Self::Entry {
                key: key.trim().to_owned(),
                value: value.trim().to_owned(),
                raw: raw.to_owned(),
            }
        } else {
            Self::Other(raw.to_owned())
        }
    }

    fn entry(key: &str, value: &str) -> Self {
        Self::Entry {
            key: key.to_owned(),
            value: value.to_owned(),
            raw: format!("{key}={value}"),
        }
    }

    fn raw(&self) -> &str {
        match self {
            Self::Section { raw, .. } | Self::Entry { raw, .. } | Self::Other(raw) => raw,
        }
    }
}

impl InstallIni {
    /// Parses `Install.ini` text. Never fails: lines it does not understand
    /// are kept verbatim.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_newline = text.ends_with('\n');
        let body = text.strip_suffix(newline).unwrap_or(text);
        let lines = if text.is_empty() {
            Vec::new()
        } else {
            body.split(newline).map(Line::parse).collect()
        };
        Self {
            lines,
            newline,
            trailing_newline,
        }
    }

    /// Returns the text, byte-identical to the parsed input except for edits.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = self
            .lines
            .iter()
            .map(Line::raw)
            .collect::<Vec<_>>()
            .join(self.newline);
        if self.trailing_newline {
            text.push_str(self.newline);
        }
        text
    }

    /// The `(id, path)` pairs in `[Installations]`, in file order.
    #[must_use]
    pub fn installations(&self) -> Vec<(String, String)> {
        self.installation_entries()
            .map(|(_, key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }

    /// Sets `key=value` in `[Installations]`.
    ///
    /// Replaces the first entry with that key in place; otherwise adds the
    /// entry after the last one in the section, creating the section at the end
    /// of the file if needed.
    pub fn set(&mut self, key: &str, value: &str) {
        let existing = self
            .installation_entries()
            .find(|(_, k, _)| *k == key)
            .map(|(index, _, _)| index);
        if let Some(index) = existing {
            self.lines[index] = Line::entry(key, value);
            return;
        }

        let Some(section) = self.lines.iter().position(is_installations_section) else {
            if self
                .lines
                .last()
                .is_some_and(|line| !line.raw().trim().is_empty())
            {
                self.lines.push(Line::Other(String::new()));
            }
            self.lines.push(Line::Section {
                name: INSTALLATIONS.to_owned(),
                raw: format!("[{INSTALLATIONS}]"),
            });
            self.lines.push(Line::entry(key, value));
            self.trailing_newline = true;
            return;
        };

        let section_end = self.lines[section + 1..]
            .iter()
            .position(|line| matches!(line, Line::Section { .. }))
            .map_or(self.lines.len(), |offset| section + 1 + offset);
        let insert_at = self.lines[section + 1..section_end]
            .iter()
            .rposition(|line| matches!(line, Line::Entry { .. }))
            .map_or(section + 1, |offset| section + 2 + offset);
        if insert_at == self.lines.len() {
            self.trailing_newline = true;
        }
        self.lines.insert(insert_at, Line::entry(key, value));
    }

    /// Removes every entry with `key` from `[Installations]`. Returns whether
    /// anything was removed.
    pub fn remove(&mut self, key: &str) -> bool {
        let indices: Vec<usize> = self
            .installation_entries()
            .filter(|(_, k, _)| *k == key)
            .map(|(index, _, _)| index)
            .collect();
        for index in indices.iter().rev() {
            self.lines.remove(*index);
        }
        !indices.is_empty()
    }

    /// `(line index, key, value)` of each entry inside `[Installations]`.
    fn installation_entries(&self) -> impl Iterator<Item = (usize, &str, &str)> {
        let mut in_section = false;
        self.lines
            .iter()
            .enumerate()
            .filter_map(move |(index, line)| match line {
                Line::Section { .. } => {
                    in_section = is_installations_section(line);
                    None
                }
                Line::Entry { key, value, .. } if in_section => {
                    Some((index, key.as_str(), value.as_str()))
                }
                _ => None,
            })
    }
}

fn is_installations_section(line: &Line) -> bool {
    matches!(line, Line::Section { name, .. } if name.eq_ignore_ascii_case(INSTALLATIONS))
}
