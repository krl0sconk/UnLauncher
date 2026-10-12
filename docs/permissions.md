# Flatpak permissions

Why the Flatpak of UnLauncher asks for each permission. This is also the justification for the Flathub reviewers.

UnLauncher (`io.github.krl0sconk.UnLauncher`) installs, builds and launches Unreal
Engine on Linux. The interface runs inside the sandbox; the Unreal Editor and the
engine build cannot.

## `--talk-name=org.freedesktop.Flatpak`

Used only through `flatpak-spawn --host`, in one place (`system::HostCommand`), to:

1. **Launch the Unreal Editor** of an installed engine (`<engine>/Engine/Binaries/Linux/UnrealEditor <project>.uproject`).
   The editor is a ~100 GB native application installed by the user outside Flatpak. It needs the host's GPU drivers (Vulkan), compilers for C++ projects, and unrestricted access to the user's projects. It cannot run inside UnLauncher's sandbox.
2. **Build Unreal Engine from source.** This runs Epic's scripts on the host:
   - `Setup.sh` downloads Epic's binary dependencies and toolchain;
   - `GenerateProjectFiles.sh`;
   - `Engine/Build/BatchFiles/Linux/Build.sh`, which runs UnrealBuildTool and clang for hours.

   These scripts expect a normal host environment.
3. **Check for `git` and `bash`** on the host before a build (`git --version`, `bash --version`).

Arguments are always passed as a list, never through a shell string. No other host commands are run. Precedents for this permission: VSCodium (`com.vscodium.codium`) and the Godot .NET editor run their toolchains the same way.

## Runtime: `org.gnome.Platform`

UnLauncher uses the GNOME runtime (51) instead of `org.freedesktop.Platform` because it ships WebKitGTK 4.1. A small helper, `unlauncher-web` (in `/app/bin/`, run inside the sandbox, never through `flatpak-spawn`), shows Epic's sign-in page and Epic's Linux download page in a real web engine: `www.unrealengine.com` only answers browsers (Cloudflare), and reading Epic's authorization-code page there spares users from copying it by hand. The main window does not load WebKit. The change needs no new permissions: the helper uses the same network access, and its cookies live in the app's own data folder.

## Other permissions

| Permission | Why |
| --- | --- |
| `--share=network` | Epic login (in the built-in web window), Fab downloads, the official engine download, cloning `EpicGames/UnrealEngine` from GitHub. |
| `--share=ipc`, `--socket=wayland`, `--socket=fallback-x11`, `--device=dri` | The egui window (OpenGL) and the WebKitGTK window of `unlauncher-web`. |
| `--filesystem=home` | Engines (tens of GB each) and projects live anywhere in the home directory. UnLauncher also reads and writes `~/.config/Epic/UnrealEngine/Install.ini`, the engine registry the Unreal Editor itself uses. |
| `--talk-name=org.freedesktop.secrets` | Epic and GitHub tokens are stored in the system keyring, never in files. |

Desktop integration (engine shortcuts, notifications, opening the login page, file chooser) uses portals: DynamicLauncher, Notification, OpenURI and FileChooser.

## Why portals are not enough

Portals cover what UnLauncher does for itself, and it uses them for exactly that. They cannot replace the two broad permissions:

- **Running the Unreal Editor and the engine build** needs a host process with the host's GPU drivers, compilers and toolchain. No portal runs arbitrary programs on the host. The editor also cannot run inside UnLauncher's sandbox: it is a ~100 GB application installed by the user, it builds C++ projects with the host's clang, and it opens the user's projects wherever they are.
- **Filesystem access.** The `FileChooser` portal hands out one file or folder at a time. UnLauncher needs to *scan* for engines and projects, to read and write the Unreal Editor's shared `~/.config/Epic/UnrealEngine/Install.ini`, and to extract tens of GB of engine into a folder the editor then uses directly. A one-off portal grant does not allow that, and it would not persist for the Editor process started on the host.

The scope is kept as small as it can be: no `--filesystem=host`, no `--device=all`, and the helper that runs the web view only navigates to Epic's own sites (see [`ARCHITECTURE.md`](../ARCHITECTURE.md)).

## What is not requested

- No `--filesystem=host`. Engines on other disks need a manual override, for example with Flatseal.
- No `--device=all`.
- No session-bus access beyond the names above.

## Linter exceptions

`flatpak-builder-lint` reports two errors that need an exception from the Flathub reviewers, justified above:

- `finish-args-flatpak-spawn-access`: launching the Unreal Editor and building the engine on the host.
- `finish-args-home-filesystem-access`: engines and projects can be anywhere in the home folder, and `~/.config/Epic/UnrealEngine/Install.ini` is shared with the Unreal Editor.
