// SPDX-License-Identifier: GPL-3.0-or-later

//! Asking the user before changing files.

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::bail;
use common::Overwrite;

/// Asks `question` on stderr and returns [`Overwrite::Yes`] only if the user
/// answers yes. With `assume_yes` (the `--yes` flag) it does not ask.
///
/// Fails without asking when stdin is not a terminal and `--yes` was not
/// given, so scripts never change files by accident.
pub fn ask(question: &str, assume_yes: bool) -> anyhow::Result<Overwrite> {
    if assume_yes {
        return Ok(Overwrite::Yes);
    }
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        bail!("refusing to change files without confirmation; pass --yes to confirm");
    }
    eprint!("{question} [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    stdin.lock().read_line(&mut answer)?;
    Ok(
        if matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes" | "s" | "si" | "sí"
        ) {
            Overwrite::Yes
        } else {
            Overwrite::No
        },
    )
}
