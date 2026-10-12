// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-cli`: UnLauncher commands without the graphical interface.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a CLI prints its results"
)]

mod build;
mod confirm;
mod engines;
mod fab_cmd;
mod github;
mod import;
mod progress;
mod projects;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use common::UserFacing;

/// Manage Unreal Engine installations and projects on Linux.
#[derive(Debug, Parser)]
#[command(name = "unlauncher-cli", version)]
struct Cli {
    /// Use this Install.ini instead of ~/.config/Epic/UnrealEngine/Install.ini.
    #[arg(long, global = true, value_name = "PATH")]
    install_ini: Option<PathBuf>,

    /// Show informational logs (-v) or debug logs (-vv) on stderr.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Registered and installed engines.
    #[command(subcommand)]
    Engines(engines::Command),
    /// Unreal projects (.uproject).
    #[command(subcommand)]
    Projects(projects::Command),
    /// Install a prebuilt engine from the zip Epic distributes for Linux.
    Import(import::ImportArgs),
    /// Clone Unreal Engine from GitHub, build it and register it.
    Build(build::BuildArgs),
    /// Fetch and rebuild an engine built from source.
    Update(build::UpdateArgs),
    /// Your Fab library (Epic sign-in required).
    #[command(subcommand)]
    Fab(fab_cmd::FabCommand),
    /// Explains how to get the official Linux prebuilt engine (the interface or `import`).
    DownloadEngine(fab_cmd::DownloadEngineArgs),
    /// Connect GitHub to clone Unreal Engine, without creating a token.
    #[command(subcommand)]
    Github(github::GithubCommand),
    /// Advanced: save or forget a personal GitHub token instead of `github login`.
    #[command(subcommand)]
    GithubToken(build::TokenCommand),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            print_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let install_ini = match cli.install_ini {
        Some(path) => path,
        None => system::paths::install_ini_path()?,
    };
    match cli.command {
        Command::Engines(command) => engines::run(command, &install_ini),
        Command::Projects(command) => projects::run(command, &install_ini),
        Command::Import(args) => import::run(args, &install_ini),
        Command::Build(args) => build::build(&args, &install_ini),
        Command::Update(args) => build::update(&args, &install_ini),
        Command::Github(command) => github::run(&command),
        Command::GithubToken(command) => build::token(&command),
        Command::Fab(command) => fab_cmd::run(&command, &install_ini),
        Command::DownloadEngine(args) => fab_cmd::download_engine(&args),
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    // egs-api logs Epic's error messages, which can quote tokens: never show them.
    let filter = match "egs_api=off".parse() {
        Ok(directive) => filter.add_directive(directive),
        Err(_) => filter,
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

/// Prints the error, its causes and, for library errors, the hint.
fn print_error(error: &anyhow::Error) {
    eprintln!("error: {error}");
    for cause in error.chain().skip(1) {
        eprintln!("  caused by: {cause}");
    }
    let hint = if let Some(e) = error.downcast_ref::<ue_core::Error>() {
        e.hint()
    } else if let Some(e) = error.downcast_ref::<ue_prebuilt::Error>() {
        e.hint()
    } else if let Some(e) = error.downcast_ref::<ue_build::Error>() {
        e.hint()
    } else if let Some(e) = error.downcast_ref::<epic_auth::Error>() {
        e.hint()
    } else if let Some(e) = error.downcast_ref::<fab::Error>() {
        e.hint()
    } else if let Some(e) = error.downcast_ref::<system::Error>() {
        e.hint()
    } else {
        None
    };
    if let Some(hint) = hint {
        eprintln!("hint: {hint}");
    }
}
