# Security policy

## Supported versions

Only the latest release receives security fixes. Please update before reporting.

## Reporting a vulnerability

**Do not open a public issue** for a security problem.

Use GitHub's private vulnerability reporting: go to the [Security tab](https://github.com/krl0sconk/UnLauncher/security) of this repository and choose **Report a vulnerability**. Include the version (or commit), how you installed it (Flatpak or native) and the steps to reproduce. Never include real tokens, cookies or passwords in a report.

You can expect an acknowledgement within a few days. Fixes are released as soon as they are ready and the report is credited unless you prefer otherwise.

## Flatpak repository key

The Flatpak repository at `https://krl0sconk.github.io/UnLauncher/repo/` and the `.flatpak` bundles are signed with this GPG key (RSA 4096, UnLauncher Flatpak Repo):

```
189E 0790 DFEF 07A8 FFF9  BD50 070B 86C3 8000 478D
```

The public key is [`packaging/flatpak/unlauncher-repo.gpg`](packaging/flatpak/unlauncher-repo.gpg). Check it with `gpg --show-keys packaging/flatpak/unlauncher-repo.gpg`. If the key ever changes, the new fingerprint will be published here first.

## Scope notes

- **Tokens** (Epic and GitHub) are kept in the system keyring through the Secret Service. They are not written to files, logs or error messages.
- **The Epic web window** (`unlauncher-web`) uses its own web profile in a folder with `0700` permissions, separate from your browser. It only navigates to Epic's sites over HTTPS (`epicgames.com`, `unrealengine.com`, `fab.com`, `epicgames.dev`, `cloudflare.com` and the captcha Epic embeds); anything else is refused and offered to your system browser.
- **Downloads** of prebuilt engines are only accepted from HTTPS links on Epic's servers.
- **Flatpak permissions** are deliberately broad (`--talk-name=org.freedesktop.Flatpak`, `--filesystem=home`) because the Unreal Editor and the engine build have to run on the host. The reasons are in [`docs/permissions.md`](docs/permissions.md). A report that depends only on these documented permissions is not a vulnerability.
- UnLauncher uses Epic's private API through the third-party `egs-api` crate. Problems in Epic's services themselves are out of scope; report them to Epic.
