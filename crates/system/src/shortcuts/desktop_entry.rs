// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering of `.desktop` files per the freedesktop.org Desktop Entry
//! Specification 1.5.

use std::fmt::Write as _;

use super::DesktopShortcut;

/// Characters that force an `Exec` argument to be quoted.
const EXEC_RESERVED: &[char] = &[
    ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')',
    '`',
];

/// Characters that must be backslash-escaped inside a quoted `Exec` argument.
const EXEC_QUOTE_ESCAPED: &[char] = &['"', '`', '$', '\\'];

/// Renders `shortcut` as a desktop entry with `icon` as its `Icon` value (an
/// icon name or an absolute path).
///
/// The `Exec` line is built from `shortcut.exec` following the spec's quoting
/// rules: arguments with reserved characters are double-quoted, `"`, `` ` ``,
/// `$` and `\` inside quotes are backslash-escaped, a literal `%` becomes
/// `%%`, and the general string escapes (`\\`, `\n`, `\t`, `\r`) are applied
/// on top. No field codes (`%f`, `%u`…) are added.
#[must_use]
pub fn render_desktop_entry(shortcut: &DesktopShortcut, icon: &str) -> String {
    let mut entry = String::from("[Desktop Entry]\nType=Application\nVersion=1.5\n");
    push_key(&mut entry, "Name", &escape_string(&shortcut.name));
    if let Some(comment) = &shortcut.comment {
        push_key(&mut entry, "Comment", &escape_string(comment));
    }
    push_key(
        &mut entry,
        "Exec",
        &escape_string(&exec_line(&shortcut.exec)),
    );
    push_key(&mut entry, "Icon", &escape_string(icon));
    entry.push_str("Terminal=false\nCategories=Development;\nStartupNotify=true\n");
    entry
}

/// Appends `key=value\n`.
fn push_key(entry: &mut String, key: &str, value: &str) {
    // Writing to a `String` cannot fail.
    let _ = writeln!(entry, "{key}={value}");
}

/// Joins `argv` into an `Exec` value, before the general string escapes.
fn exec_line(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote_exec_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quotes one `Exec` argument if needed and escapes `%`.
fn quote_exec_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty() || arg.contains(EXEC_RESERVED);
    let mut out = String::with_capacity(arg.len() + 2);
    if needs_quotes {
        out.push('"');
    }
    for c in arg.chars() {
        if c == '%' {
            out.push_str("%%");
        } else {
            if needs_quotes && EXEC_QUOTE_ESCAPED.contains(&c) {
                out.push('\\');
            }
            out.push(c);
        }
    }
    if needs_quotes {
        out.push('"');
    }
    out
}

/// Applies the escapes of the `string`/`localestring` value types.
fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, c) in value.chars().enumerate() {
        match c {
            ' ' if i == 0 => out.push_str("\\s"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_arguments_are_not_quoted() {
        assert_eq!(quote_exec_arg("unlauncher"), "unlauncher");
        assert_eq!(quote_exec_arg("--launch-engine=x"), "--launch-engine=x");
    }

    #[test]
    fn empty_argument_is_quoted() {
        assert_eq!(quote_exec_arg(""), "\"\"");
    }

    #[test]
    fn leading_space_in_strings_is_escaped() {
        assert_eq!(escape_string(" a b"), "\\sa b");
    }
}
