# Flatpak packaging

`flatpak-builder` manifests for UnLauncher (`io.github.krl0sconk.UnLauncher`). They use the GNOME runtime (`org.gnome.Platform//51`) because it ships WebKitGTK 4.1, which the web window (`unlauncher-web`) needs. Both binaries, `unlauncher` and `unlauncher-web`, are installed in `/app/bin/` and run inside the sandbox; the Unreal Editor and engine builds run on the host through `flatpak-spawn --host`.

| File | Purpose |
| --- | --- |
| `io.github.krl0sconk.UnLauncher.yml` | The official manifest: offline build of the checkout (crates come from `cargo-sources.json`), branch `stable`. The release workflow builds this one. |
| `io.github.krl0sconk.UnLauncher.devel.yml` | Quick local build: cargo downloads the crates during the build (`--share=network`). |
| `cargo-sources.json` | Vendored crates for the offline build; regenerate whenever `Cargo.lock` changes. |
| `unlauncher-repo.gpg` | Public key of the repository (binary). Users trust it through the `.flatpakref`. |
| `site/index.html` | Template of the landing page published on GitHub Pages. |

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

For the offline build, use `packaging/flatpak/io.github.krl0sconk.UnLauncher.yml` instead. Regenerate `packaging/flatpak/cargo-sources.json` whenever `Cargo.lock` changes:

```bash
git clone https://github.com/flatpak/flatpak-builder-tools.git /tmp/flatpak-builder-tools
python3 -m pip install --user aiohttp tomlkit
python3 /tmp/flatpak-builder-tools/cargo/flatpak-cargo-generator.py \
    Cargo.lock -o packaging/flatpak/cargo-sources.json
```

## Publishing

UnLauncher is distributed from its own Flatpak repository, not from Flathub. The workflow `.github/workflows/flatpak.yml` does everything when a tag `vX.Y.Z` is pushed (or when it is run by hand with `gh workflow run flatpak.yml -f tag=vX.Y.Z`):

1. Builds the manifest in the GNOME 51 image of the official Flatpak actions, exporting to an OSTree repository on branch `stable` and signing commits with the repository key.
2. Runs `flatpak build-update-repo --generate-static-deltas --prune --gpg-sign=... --title=UnLauncher`, which also signs the summary and keeps AppStream data for GNOME Software and KDE Discover.
3. Builds `UnLauncher-<version>.flatpak` (with `--runtime-repo` pointing to Flathub) and its `.sha256`, and uploads both to the GitHub release of the tag.
4. Publishes the site on GitHub Pages: `repo/`, `io.github.krl0sconk.UnLauncher.flatpakref`, `unlauncher.flatpakrepo` and `index.html`.

Every build starts from a new repository and prunes it, so only the latest version is published. Pages allows 1 GB per site and 100 MB per file; the workflow prints the size of `repo/` and lists files over 90 MB.

The release needs `<release version="X.Y.Z">` in `packaging/linux/io.github.krl0sconk.UnLauncher.metainfo.xml`; the workflow fails otherwise.

### Signing key

The repository is signed with a dedicated RSA 4096 GPG key without passphrase. Its fingerprint is in [`SECURITY.md`](../../SECURITY.md). The public part is `unlauncher-repo.gpg` and is embedded (base64) in the `.flatpakref` and `.flatpakrepo`. The private part is only stored as a GitHub Actions secret and in the maintainer's password manager; never commit it.

| Secret | Content |
| --- | --- |
| `FLATPAK_GPG_PRIVATE_KEY` | Armored private key (`gpg --armor --export-secret-keys`). |
| `FLATPAK_GPG_KEY_ID` | Fingerprint of that key. The workflow checks it matches `unlauncher-repo.gpg`. |

To rotate the key, generate a new one in a temporary `GNUPGHOME`, replace both secrets and `unlauncher-repo.gpg`, update the fingerprint in `SECURITY.md` and publish a release. Users must remove and re-add the remote, because Flatpak pins the key.

### Install

```bash
flatpak install --user https://krl0sconk.github.io/UnLauncher/io.github.krl0sconk.UnLauncher.flatpakref
```

To test a local build as a user would, export it to a repository, add it as a remote and install from there:

```bash
flatpak-builder --repo=repo --force-clean build-dir packaging/flatpak/io.github.krl0sconk.UnLauncher.yml
flatpak build-bundle repo unlauncher.flatpak io.github.krl0sconk.UnLauncher stable
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
