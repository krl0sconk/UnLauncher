// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli github login | status | logout`: GitHub without tokens.

use clap::Subcommand;
use common::{Secret, UserFacing};
use epic_auth::{CredentialKey, KeyringStore, SecretStore};
use github_auth::{Access, DeviceFlow, GITHUB_API_BASE};

#[derive(Debug, Subcommand)]
pub enum GithubCommand {
    /// Connect GitHub: uses this computer's GitHub account (gh, git) when it
    /// can read the Unreal Engine source, otherwise the Device Flow (type a
    /// short code on github.com; no token to create or paste).
    Login {
        /// Always use the Device Flow, ignoring existing credentials.
        #[arg(long)]
        device: bool,
    },
    /// Show the connected account and whether it can read EpicGames/UnrealEngine.
    Status,
    /// Remove the saved GitHub access from the keyring.
    Logout,
}

pub fn run(command: &GithubCommand) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let http = epic_auth::http_client()?;
    match command {
        GithubCommand::Login { device } => runtime.block_on(login(&http, *device)),
        GithubCommand::Status => {
            let Some(token) = KeyringStore.get(CredentialKey::GitHubToken)? else {
                println!("GitHub: not connected. Run `unlauncher-cli github login`.");
                return Ok(());
            };
            let access =
                runtime.block_on(github_auth::check_access(&http, GITHUB_API_BASE, &token))?;
            print_access(&access);
            Ok(())
        }
        GithubCommand::Logout => {
            KeyringStore.delete(CredentialKey::GitHubToken)?;
            println!(
                "GitHub disconnected. To revoke UnLauncher on GitHub too, open \
                 https://github.com/settings/applications"
            );
            Ok(())
        }
    }
}

async fn login(http: &reqwest::Client, force_device: bool) -> anyhow::Result<()> {
    if !force_device {
        let sandbox = system::Sandbox::detect();
        let existing =
            tokio::task::spawn_blocking(move || github_auth::detect_existing(sandbox)).await?;
        for credential in existing {
            if let Ok(access) =
                github_auth::check_access(http, GITHUB_API_BASE, &credential.token).await
                && access.is_ready()
            {
                println!(
                    "Using this computer's GitHub account (@{}, from {}).",
                    access.login().unwrap_or_default(),
                    credential.source.label()
                );
                return save(http, &credential.token).await;
            }
        }
    }
    let flow = DeviceFlow::new(http.clone());
    let code = flow.start().await?;
    println!(
        "Open {} and enter this code:\n\n    {}\n",
        code.verification_uri, code.user_code
    );
    if let Err(error) = system::open_url(&code.verification_uri) {
        tracing::debug!(%error, "cannot open the browser");
    }
    println!("Waiting for you to authorize UnLauncher on GitHub… (Ctrl+C to cancel)");
    let token = flow.wait_for_token(&code).await?;
    save(http, &token).await
}

/// Saves `token` in the keyring and reports what it can do.
async fn save(http: &reqwest::Client, token: &Secret) -> anyhow::Result<()> {
    let stored = token.clone();
    tokio::task::spawn_blocking(move || KeyringStore.set(CredentialKey::GitHubToken, &stored))
        .await??;
    let access = github_auth::check_access(http, GITHUB_API_BASE, token).await?;
    print_access(&access);
    Ok(())
}

fn print_access(access: &Access) {
    let mark = if access.is_ready() { "ok" } else { "missing" };
    println!("GitHub access: {mark} — {}", access.summary());
    if let Some(hint) = access.hint() {
        println!("  {hint}");
    }
}
