# Architecture

UnLauncher is a Cargo workspace: a graphical interface (egui), a command-line tool, a small helper that shows web pages, and a set of library crates underneath.

## Repository layout

```
apps/launcher/     the interface (package `unlauncher`)
apps/cli/          the command-line tool (`unlauncher-cli`)
apps/web/          the web window helper (`unlauncher-web`)
crates/            the library crates below
packaging/         Flatpak manifests, desktop file, AppStream metainfo, screenshots
```

## Crates and layers

Each crate depends only on crates in lower layers.

| Layer | Crates | What they do |
| --- | --- | --- |
| apps | `apps/launcher` (`unlauncher`), `apps/cli` (`unlauncher-cli`), `apps/web` (`unlauncher-web`) | The interface, the CLI and the web window helper. The only places that use `egui`, `clap`, `wry` and `tao`. |
| 2 | `ue-build`, `ue-prebuilt`, `fab` | Building Unreal Engine from source; importing and downloading prebuilt engines; the Fab library behind an `AssetSource` trait. |
| 1 | `jobs`, `epic-auth`, `web-bridge`, `github-auth` | Long-running tasks with progress, pause and cancel; Epic login and tokens; the protocol and client of the web window; GitHub access. |
| 0 | `ue-core`, `system` | Engines, projects, `Install.ini`, `Build.version`, `.uproject` (local files only, no network, no processes); Flatpak detection, host commands and system paths. |
| — | `common` | Types shared by all: `UserFacing` (error summary plus hint), `Secret`, `Overwrite`. |

Library crates never depend on a UI toolkit or on an app. Errors are typed enums that implement `UserFacing`, so every screen and the CLI can show a message and a next step.

## Processes

- **The interface** (`unlauncher`) never loads WebKit.
- **The web window** (`unlauncher-web`) is a separate process built on `wry` and `tao`. The interface starts it and talks to it through `web-bridge`: one JSON command per line on its stdin, one JSON event per line on its stdout. It runs inside the Flatpak sandbox, next to the interface, and may only navigate to Epic's own sites over HTTPS (`web_bridge::is_allowed_navigation`).
- **The Unreal Editor, `git` and the engine build scripts** run on the host. All of them go through `system::HostCommand`, which prefixes `flatpak-spawn --host` when UnLauncher runs in a Flatpak (detected with `/.flatpak-info`). The editor needs the host's GPU drivers and compilers, so it cannot run in the sandbox. `HostCommand::bundled` starts UnLauncher's own binaries (`unlauncher-web`) in the sandbox instead.

## An example: installing a prebuilt engine

1. The interface asks `unlauncher-web` (through `web-bridge`) to open Epic's Linux download page with the saved web session, and reads the signed download links from the page.
2. `ue-prebuilt` turns the links into a list of versions and, once the user picks one, checks that the link points to an Epic host over HTTPS and downloads the zip next to the destination, resuming a partial file if there is one.
3. `ue-prebuilt` extracts the zip, validates that it contains an engine, and registers it in `Install.ini` through `ue-core`.
4. Each step runs as a `jobs` task, so progress, pause and cancel work the same way for downloads, imports and builds.

## Secrets

Epic and GitHub tokens live in the system keyring (Secret Service) and nowhere else. In memory they are wrapped in `common::Secret`, whose `Debug` and `Display` print a placeholder, so they cannot reach a log or an error message by accident. Signed download URLs get the same care.

## External services

- **Epic's private API, through [`egs-api`](https://crates.io/crates/egs-api).** Epic has no public API for the Fab library or for asset manifests, and this is the one the Epic Games Launcher uses. It is isolated in `epic-auth` and `fab` so that it can be replaced if it breaks.
- **A real web engine for `unrealengine.com` and `fab.com`.** Cloudflare answers only browsers there, so plain HTTP clients get a challenge page. The web window loads those pages, and reads the download links and the Fab engine versions from inside them. Third-party logins (Google, Apple, …) stay in the system browser, because those providers do not like embedded web views.
- **GitHub, to clone `EpicGames/UnrealEngine`.** The repository is private and visible only to accounts linked to Epic. `github-auth` reuses an existing `gh` or `git` credential when there is one, can guide `gh auth login`, and supports a Device Flow sign-in (no client secret, no token to create by hand). EpicGames restricts which OAuth Apps may read the repository, so the sign-in through `gh` is the one that works in practice.

## Files UnLauncher shares with the Unreal Editor

UnLauncher reads and writes `~/.config/Epic/UnrealEngine/Install.ini`, the engine registry the editor itself uses. It never overwrites a file it does not own without an explicit `Overwrite::Yes` from the caller, and it keeps a `.bak` of what it changed.

## The interface

`apps/launcher/src/theme.rs` is the only place with colors, spacing and text sizes (written by hand from `apps/launcher/assets/tokens.json`; a test fails if they differ). Screens in `ui/` are built from the components in `ui/widgets/` and use no color or size of their own. Work that blocks (network, disk, processes) runs off the drawing thread and reports back through `jobs`.

## Rules the code follows

- Library code never calls `unwrap`, `expect` or `panic!`; every failure is a typed error.
- Tests use temporary folders and injected paths, never the real home, and never touch the network (they use local fake servers) or start Unreal Engine.
- Parsers (`Install.ini`, `.uproject`, `Build.version`, build logs, manifests) are covered by fixtures with realistic data.
- Anything that deletes or overwrites a user's file takes an explicit confirmation parameter.
