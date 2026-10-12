// SPDX-License-Identifier: GPL-3.0-or-later

//! `SectionHeader`: the label above a group of cards, with that group's
//! actions on the right. `space-4` below it.

use egui::{Align, Layout, Ui};

use super::colors;
use crate::theme::{Text, space};

/// Draws `label` and, right-aligned, the buttons `actions` adds (the first
/// one added is the rightmost).
pub fn section_header(ui: &mut Ui, label: &str, actions: impl FnOnce(&mut Ui)) {
    let p = colors(ui);
    ui.horizontal(|ui| {
        ui.label(Text::SectionLabel.colored(label, p.ink_muted));
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    super::gap(ui, space::S4);
}
