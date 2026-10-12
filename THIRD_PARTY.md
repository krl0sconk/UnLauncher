# Third-party material

UnLauncher is licensed under [GPL-3.0-or-later](LICENSE). It bundles and depends on the following third-party work. Their licenses are compatible with the GPL-3.0.

## Fonts and icons bundled in the interface

| Material | Where | License |
| --- | --- | --- |
| [Archivo](https://github.com/Omnibus-Type/Archivo) | `apps/launcher/assets/fonts/` | SIL Open Font License 1.1 (`Archivo-OFL.txt`) |
| [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) | `apps/launcher/assets/fonts/` | SIL Open Font License 1.1 (`JetBrainsMono-OFL.txt`) |
| [Lucide](https://lucide.dev) icons | `apps/launcher/assets/icons/` | ISC (`LICENSE-Lucide-ISC.txt`) |
| egui's default fonts (`epaint_default_fonts`: Ubuntu-Light, Hack, Noto Emoji, emoji-icon-font) | compiled into the interface | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu Font Licence 1.0 |

The interface also shows these licenses in **Settings → About**.

## Notable libraries

| Library | Used for | License |
| --- | --- | --- |
| [egs-api](https://github.com/AchetaGames/egs-api-rs) | Talking to Epic's services (Fab library, manifests, chunks) | MIT |
| [egui](https://github.com/emilk/egui) / eframe | The graphical interface | MIT OR Apache-2.0 |
| [wry](https://github.com/tauri-apps/wry) and [tao](https://github.com/tauri-apps/tao) | The embedded Epic web window (`unlauncher-web`, WebKitGTK) | MIT OR Apache-2.0 |
| [git2](https://github.com/rust-lang/git2-rs) | Cloning Unreal Engine from GitHub | MIT OR Apache-2.0 |

## Everything else

The complete list of Rust dependencies and their licenses comes from `Cargo.lock`. Regenerate it with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny):

```bash
cargo install --locked cargo-deny
cargo deny list
```

`cargo deny check licenses` (run in CI with the rules in [`deny.toml`](deny.toml)) fails if a dependency uses a license that is not on the allow-list.

## Trademarks

Unreal Engine, Fab and Epic Games are trademarks of Epic Games, Inc. UnLauncher is not affiliated with or endorsed by Epic Games, and it does not use Epic's logos.
