// SPDX-License-Identifier: GPL-3.0-or-later

//! Archivo and JetBrains Mono, one egui font family per weight (egui does not
//! pick weights inside a family). Each Archivo weight is the Latin file with
//! Latin Extended behind it; egui's own fonts are the last fallback for
//! symbols neither covers.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily};

/// A font file: its name in egui and its bytes.
type File = (&'static str, &'static [u8]);

/// `(family name, files)` in fallback order.
const FAMILIES: [(&str, &[File]); 7] = [
    (
        "archivo-400",
        &[
            (
                "archivo-latin-400",
                include_bytes!("../../assets/fonts/archivo-latin-400-normal.ttf"),
            ),
            (
                "archivo-latin-ext-400",
                include_bytes!("../../assets/fonts/archivo-latin-ext-400-normal.ttf"),
            ),
        ],
    ),
    (
        "archivo-500",
        &[
            (
                "archivo-latin-500",
                include_bytes!("../../assets/fonts/archivo-latin-500-normal.ttf"),
            ),
            (
                "archivo-latin-ext-500",
                include_bytes!("../../assets/fonts/archivo-latin-ext-500-normal.ttf"),
            ),
        ],
    ),
    (
        "archivo-600",
        &[
            (
                "archivo-latin-600",
                include_bytes!("../../assets/fonts/archivo-latin-600-normal.ttf"),
            ),
            (
                "archivo-latin-ext-600",
                include_bytes!("../../assets/fonts/archivo-latin-ext-600-normal.ttf"),
            ),
        ],
    ),
    (
        "archivo-700",
        &[
            (
                "archivo-latin-700",
                include_bytes!("../../assets/fonts/archivo-latin-700-normal.ttf"),
            ),
            (
                "archivo-latin-ext-700",
                include_bytes!("../../assets/fonts/archivo-latin-ext-700-normal.ttf"),
            ),
        ],
    ),
    (
        "archivo-800",
        &[
            (
                "archivo-latin-800",
                include_bytes!("../../assets/fonts/archivo-latin-800-normal.ttf"),
            ),
            (
                "archivo-latin-ext-800",
                include_bytes!("../../assets/fonts/archivo-latin-ext-800-normal.ttf"),
            ),
        ],
    ),
    (
        "mono-400",
        &[(
            "jetbrains-mono-latin-400",
            include_bytes!("../../assets/fonts/jetbrains-mono-latin-400-normal.ttf"),
        )],
    ),
    (
        "mono-500",
        &[(
            "jetbrains-mono-latin-500",
            include_bytes!("../../assets/fonts/jetbrains-mono-latin-500-normal.ttf"),
        )],
    ),
];

/// The font definitions: our families plus egui's defaults as fallback.
pub(super) fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (family, files) in FAMILIES {
        let mut names = Vec::new();
        for (name, bytes) in files {
            defs.font_data
                .insert((*name).to_owned(), Arc::new(FontData::from_static(bytes)));
            names.push((*name).to_owned());
        }
        names.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(family.into()), names);
    }
    // Body text is Archivo 400 and code is JetBrains Mono 400.
    for (generic, named) in [
        (FontFamily::Proportional, "archivo-400"),
        (FontFamily::Monospace, "mono-400"),
    ] {
        if let Some(names) = defs.families.get(&FontFamily::Name(named.into())).cloned() {
            defs.families.insert(generic, names);
        }
    }
    defs
}

/// Registers the families in `ctx`.
pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_weight_is_a_family_with_a_fallback() {
        let defs = definitions();
        for (family, files) in FAMILIES {
            let names = &defs.families[&FontFamily::Name(family.into())];
            assert!(names.len() >= files.len(), "{family}");
            for name in names {
                assert!(defs.font_data.contains_key(name), "{family}: {name}");
            }
        }
        assert_eq!(
            defs.families[&FontFamily::Proportional][0],
            "archivo-latin-400"
        );
        assert_eq!(
            defs.families[&FontFamily::Monospace][0],
            "jetbrains-mono-latin-400"
        );
    }
}
