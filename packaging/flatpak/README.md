# Flatpak packaging

`flatpak-builder` manifests for UnLauncher (`io.github.krl0sconk.UnLauncher`). They use the GNOME runtime (`org.gnome.Platform//51`) because it ships WebKitGTK 4.1, which the web window (`unlauncher-web`) needs. Both binaries, `unlauncher` and `unlauncher-web`, are installed in `/app/bin/` and run inside the sandbox; the Unreal Editor and engine builds run on the host through `flatpak-spawn --host`.

| File | Purpose |
| --- | --- |
| `io.github.krl0sconk.UnLauncher.devel.yml` | Quick local build: cargo downloads the crates during the build (`--share=network`). Not valid for Flathub. |
| `io.github.krl0sconk.UnLauncher.yml` | Offline build of the local checkout, like Flathub's. Needs `../flathub/cargo-sources.json`. |
| `../flathub/io.github.krl0sconk.UnLauncher.yml` | The manifest submitted to Flathub: the same build from a git tag. |

The desktop file, MIME type, icon and AppStream metadata are in `packaging/linux/` and installed by every manifest. Run all commands from the repository root. The permissions are explained in [`docs/permissions.md`](../../docs/permissions.md).

## Requirements

```bash
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Platform//51 org.gnome.Sdk//51 \
    org.freedesktop.Sdk.Extension.rust-stable//26.08
```

Plus `flatpak` and `flatpak-builder` from your distribution. The Rust extension must provide a Rust at least as new as `rust-version` in the root `Cargo.toml`.

## Build and run

```bash
flatpak-builder --user --install --force-clean --install-deps-from=flathub \
    build-dir packaging/flatpak/io.github.krl0sconk.UnLauncher.devel.yml
flatpak run io.github.krl0sconk.UnLauncher
```

For the offline build, use `packaging/flatpak/io.github.krl0sconk.UnLauncher.yml` instead. It needs `packaging/flathub/cargo-sources.json`, which must be regenerated whenever `Cargo.lock` changes:

```bash
git clone https://github.com/flatpak/flatpak-builder-tools.git /tmp/flatpak-builder-tools
python3 -m pip install --user aiohttp tomlkit
python3 /tmp/flatpak-builder-tools/cargo/flatpak-cargo-generator.py \
    Cargo.lock -o packaging/flathub/cargo-sources.json
```

To test on another machine, build a bundle:

```bash
flatpak-builder --repo=repo --force-clean build-dir packaging/flatpak/io.github.krl0sconk.UnLauncher.yml
flatpak build-bundle repo unlauncher.flatpak io.github.krl0sconk.UnLauncher
flatpak install --user unlauncher.flatpak
```

## Check after installing

- UnLauncher appears in the application menu with its icon.
- Double-clicking a `.uproject` offers to open it with UnLauncher.
- Opening an engine starts the editor on the host.
- **Library → Sign in** opens the "UnLauncher — Epic Games" window and closes it after the login.
- **Install engine → List available versions** reads the versions from unrealengine.com.

## Uninstall

```bash
flatpak uninstall --user io.github.krl0sconk.UnLauncher
```
