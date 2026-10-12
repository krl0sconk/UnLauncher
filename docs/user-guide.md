# User guide

UnLauncher installs Unreal Engine, opens your projects with the right engine and brings your Fab assets, without a terminal.

## First run

UnLauncher looks for engines in your home folder and `/opt` and offers to register what it finds. It moves and modifies nothing.

It uses the same registry as Unreal (`~/.config/Epic/UnrealEngine/Install.ini`), so what you register also shows up in the editor and the other way around. Every time it changes that file it keeps an `Install.ini.bak`.

## Engines

Engines are the cards under **Engine versions** in **Library**. The badge says whether an engine is *Prebuilt*, *Source* or *External*. The ⋮ menu of a card has **Update** (source builds only), **Open folder**, **Add to app menu**, **Clean caches**, **Remove from list** and **Uninstall**.

- **Add existing** registers an engine already on your disk: choose the folder that contains `Engine/`.
- **Install engine → Prebuilt**: click **List available versions**. UnLauncher opens unrealengine.com/linux in its own window (sign in to Epic there if asked), reads the versions and closes it. Pick one and a new or empty destination folder; it is downloaded, extracted and registered. The zip (about 40 GB) is downloaded next to the destination, on the same disk, and deleted afterwards, so that disk needs room for both. You can also download the Linux zip yourself from <https://www.unrealengine.com/linux> and use **Choose zip…**.
- **Install engine → From source** clones Unreal Engine from GitHub and builds it. The repository is private, so the **Connect GitHub** assistant checks each step on its own: sign in to Epic; connect a GitHub account (UnLauncher can reuse the one from the `gh` CLI or git on this computer, or guide `gh auth login` for you); link GitHub to your Epic account on Epic's connections page; accept the invitation to the EpicGames organization. It needs about 300 GB of free disk and 32 GB of RAM is recommended. The build takes hours and is followed in **Tasks**.
- **Update** fetches the latest commits of the branch and rebuilds what changed; it stops without touching anything if you modified the engine's code.
- **Uninstall** deletes the engine folder, never your projects, and asks you to type the engine id.
- **Found on this computer** lists engines on disk that are not registered yet.

When a new Unreal Engine version is released, **Library** shows a notice with **Download** and **Later** (checked at most once a day).

## Projects

Projects are the cards under **My projects**. UnLauncher searches `~/Documents/Unreal Projects`, `~/Unreal Projects` and the folders you add with **Add folder**.

- Double-click a card, or a `.uproject` in your file manager, to open the project with its engine. If its engine is missing, the card says so and offers **Change engine** or **Install engine**.
- The ⋮ menu has **Change engine** (a `.uproject.bak` is kept), **Show in folder** and **Delete project**.
- **New project** creates a project from the engine's Blueprint templates.
- If the editor closes with an error, UnLauncher opens and explains the cause from the log.

## Fab

**Sign in with Epic** (or **Settings → Accounts → Connect**) opens Epic's login in a window of UnLauncher. UnLauncher never reads your password; it keeps only a session token in the system keyring. If you sign in with Google, Apple, Facebook, Xbox, PlayStation, Steam or Nintendo, the login continues in your browser: when Epic shows a text with `authorizationCode`, copy it and return to UnLauncher, which signs in by itself.

**Fab** shows your library as cards. Search by title and filter by asset packs or plugins. Click a card, then **Add to project** (or **Install to engine** for plugins). **Get more on Fab** opens fab.com with your session and reloads the library when you close it.

## Tasks

Long installs, builds and downloads appear in **Tasks** with their progress. You can pause, resume and cancel them; a task interrupted by closing UnLauncher can be retried. You get a system notification when one finishes.

## Settings

**Appearance** (system, dark or light), the folders searched for projects, **Accounts** (Epic Games and GitHub, with Connect and Disconnect) and **About**.

## Command line

`unlauncher-cli` does the same without the interface; `unlauncher-cli --help` lists the commands. Anything that modifies files asks for confirmation, or takes `--yes`.

## Troubleshooting

- **"no installed engine"** on a project: the engine it was created with is not registered. Install it or use **Change engine**.
- **A build fails**: the task shows the likely cause. The full log is in `~/.local/share/unlauncher/logs/` (Flatpak: `~/.var/app/io.github.krl0sconk.UnLauncher/data/unlauncher/logs/`).
- **Engines on another disk (Flatpak)**: give the app access to that disk, for example with Flatseal.
- **The Epic window does not open** (outside Flatpak): install WebKitGTK 4.1 (`libwebkit2gtk-4.1-0`, `webkit2gtk4.1` or `webkit2gtk-4.1`). Meanwhile UnLauncher uses your browser.
