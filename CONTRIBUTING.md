# Contributing to UnLauncher

Thanks for your interest. UnLauncher is a native Unreal Engine launcher for Linux, written in Rust with egui and distributed as a Flatpak.

Before a larger change, open an issue to talk it through. [`ARCHITECTURE.md`](ARCHITECTURE.md) explains how the project is organized.

## Code rules

- **Layers.** A crate depends only on crates in lower layers (see `ARCHITECTURE.md`). Crates in `crates/` never depend on `egui`, `clap`, `wry`, `tao` or an app.
- **No panics in libraries.** No `unwrap`, `expect` or `panic!` in library code; return typed errors that implement `common::UserFacing`.
- **Processes.** Launch external programs only through `system::HostCommand` (`bundled` for UnLauncher's own binaries), so Flatpak works.
- **WebKit** is linked only by `apps/web`; the interface talks to it through `web-bridge`.
- **User files.** Never delete or overwrite a file you do not own without an explicit confirmation parameter.
- **Secrets.** Tokens are wrapped in `common::Secret` and stored only in the system keyring; never print or log them.
- **Interface.** Colors, sizes and spacing come from `apps/launcher/src/theme.rs`, not from the screens.
- **Headers and dependencies.** Every new `.rs` file starts with `// SPDX-License-Identifier: GPL-3.0-or-later`. Do not add a dependency without saying which one, what for and its license; it must be compatible with GPL-3.0 (`cargo deny check licenses`).
- **Parsers** (`Install.ini`, `.uproject`, `Build.version`, build logs, manifests) are written test-first, with a fixture.
- Code, comments, error messages and commit messages are in English; commits follow Conventional Commits.

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -p unlauncher-cli -- engines list      # CLI
cargo run -p unlauncher                          # interface
cargo build -p unlauncher-web                    # web window (the interface looks for it next to its executable)
```

Building the workspace needs GTK 3 and WebKitGTK 4.1 development files (see the [README](README.md)). Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --workspace` before opening a pull request; CI runs the same checks plus `cargo deny` and the Flatpak/AppStream linters.

## Tests

- Never read or write your real home in tests (`~/.config/Epic`, `~/.var/app`). Use `tempfile` and inject paths.
- Use fixtures with made-up data (paths like `/mnt/games/...`, fake signatures). Never commit real tokens, cookies, account ids or signed URLs.
- Tests do no real network access, and never build Unreal Engine or start the editor.

## Licensing

UnLauncher is GPL-3.0-or-later. By submitting a contribution you agree that it is licensed under the same terms (inbound = outbound); there is no CLA.

## Recording the demo GIF (optional)

The README has a placeholder for an animated demo (`docs/assets/demo.gif`). To record one, use [Kooha](https://github.com/SeaDve/Kooha) or [Peek](https://github.com/phw/peek) on the app started with `cargo run -p unlauncher -- --demo` (made-up data, no account): 800 px wide, under 10 MB, a few seconds of Library → Fab → Tasks. Then uncomment the image in the README.

## Reporting problems

Use the issue templates. For a vulnerability, follow [`SECURITY.md`](SECURITY.md) instead, and never paste tokens or cookies anywhere.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
