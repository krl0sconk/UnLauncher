// SPDX-License-Identifier: GPL-3.0-or-later

//! What is drawn while a picture is not there: a `thumb-*` color chosen by
//! hashing the name, so the same project always gets the same one.

use super::disk::stable_hash;
use crate::theme::Palette;

/// How many `thumb-*` colors the design system has.
const COLORS: u64 = 5;

/// Which `thumb-*` color (0 to 4) belongs to `name`.
pub fn color_index(name: &str) -> usize {
    // The remainder is below 5.
    usize::try_from(stable_hash(name) % COLORS).unwrap_or(0)
}

/// The placeholder color of `name` in `palette`.
pub fn color(palette: &Palette, name: &str) -> egui::Color32 {
    palette.thumb[color_index(name)]
}

/// Up to two capital initials: the first letters of the first two words, or
/// the first two letters of a single word (`KartRacer` gives `KR`, as it is
/// written in camel case; `my game` gives `MG`).
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let letters: Vec<char> = match words.as_slice() {
        [] => return "?".to_owned(),
        [one] => camel_initials(one),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };
    letters.iter().flat_map(|c| c.to_uppercase()).collect()
}

/// For one word: its first letter and the next capital (`KartRacer` →
/// `KR`), or else its first two letters.
fn camel_initials(word: &str) -> Vec<char> {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return Vec::new();
    };
    let rest: Vec<char> = chars.collect();
    let second = rest
        .iter()
        .copied()
        .find(|c| c.is_uppercase())
        .or_else(|| rest.first().copied());
    std::iter::once(first).chain(second).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_follow_words_and_camel_case() {
        assert_eq!(initials("KartRacer"), "KR");
        assert_eq!(initials("My Game"), "MG");
        assert_eq!(initials("my_game_2"), "MG");
        assert_eq!(initials("Lyra"), "LY");
        assert_eq!(initials("X"), "X");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("Épico"), "ÉP");
    }

    #[test]
    fn a_name_always_gets_the_same_color() {
        for name in ["KartRacer", "Lyra", "Stack O Bot", ""] {
            assert!(color_index(name) < 5);
            assert_eq!(color_index(name), color_index(name));
        }
        // The five colors are all reachable.
        let used: std::collections::HashSet<usize> = (0..200)
            .map(|n| color_index(&format!("Project{n}")))
            .collect();
        assert_eq!(used.len(), 5);
    }
}
