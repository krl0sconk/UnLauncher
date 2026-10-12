// SPDX-License-Identifier: GPL-3.0-or-later

//! UnLauncher graphical interface.
//!
//! `unlauncher` opens the window. `unlauncher Game.uproject` (what a double
//! click in the file manager runs) opens the project with its engine without
//! showing the window, and falls back to the window if that is not possible.
//! `unlauncher --launch-engine <id>` starts an engine's editor (used by menu
//! shortcuts). Both stay in the background until the editor closes; if it
//! crashes, they open the window to say why. The window starts editors the
//! same way, so a crash is caught even after the window is closed.

mod app;
mod backend;
mod epic_tasks;
mod images;
mod settings;
mod tasks;
mod text;
mod theme;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::backend::Env;

/// Launcher for Unreal Engine on Linux.
#[derive(Debug, Parser)]
#[command(name = "unlauncher", version)]
struct Args {
    /// A .uproject file to open with its engine.
    project: Option<PathBuf>,

    /// Start the editor of this registered engine.
    #[arg(long, value_name = "ID", conflicts_with = "project")]
    launch_engine: Option<String>,

    /// Exit as soon as the first frame is drawn (to measure startup time).
    #[arg(long, hide = true)]
    measure_startup: bool,

    /// Send a test desktop notification and exit (diagnostics).
    #[arg(long, hide = true)]
    test_notification: bool,

    /// Fill the app with made-up data: a signed-in name, 100 Fab assets and a
    /// few tasks (used to take screenshots and to measure).
    #[arg(long, hide = true)]
    demo: bool,

    /// With `--demo`: a dialog to open (install, install-prebuilt,
    /// new-project, asset, uninstall), for screenshots.
    #[arg(long, hide = true, requires = "demo")]
    demo_dialog: Option<String>,

    /// Screen to show first (used to take screenshots).
    #[arg(long, hide = true, value_enum)]
    screen: Option<app::Screen>,
}

fn main() -> ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    // egs-api logs Epic's error messages, which can quote tokens: never show them.
    let filter = match "egs_api=off".parse() {
        Ok(directive) => filter.add_directive(directive),
        Err(_) => filter,
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    let (env, settings_path) = match environment() {
        Ok(found) => found,
        Err(error) => {
            tracing::error!(%error, "cannot determine system paths");
            return ExitCode::FAILURE;
        }
    };

    if args.test_notification {
        return match system::notify(env.sandbox, "UnLauncher", "Test notification") {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("notification failed: {error:?}");
                ExitCode::FAILURE
            }
        };
    }

    // Direct actions: no window unless the editor cannot start or crashes.
    let direct = if let Some(project) = &args.project {
        Some(backend::watch_project(&env, project))
    } else {
        args.launch_engine
            .as_deref()
            .map(|id| backend::watch_engine_by_id(&env, id))
    };
    let startup = match direct {
        Some(Ok(None)) => return ExitCode::SUCCESS,
        Some(Ok(Some(crash))) => Some(app::Dialog::EditorCrashed(crash)),
        Some(Err(error)) => Some(app::Dialog::Error(error)),
        None => None,
    };

    match run_window(env, settings_path, startup, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "cannot open the window");
            ExitCode::FAILURE
        }
    }
}

fn environment() -> Result<(Env, PathBuf), system::Error> {
    let home = system::paths::home_dir()?;
    let data_dir = system::paths::app_data_dir()?;
    let env = Env {
        install_ini: system::paths::install_ini_path_in(&home),
        metadata: data_dir.join("engines.toml"),
        tasks_state: data_dir.join("tasks.json"),
        logs_dir: data_dir.join("logs"),
        cache_dir: data_dir.join("cache"),
        webview_dir: data_dir.join("webview"),
        thumbnails_dir: system::paths::app_cache_dir()?.join("thumbnails"),
        downloads_dir: system::paths::downloads_dir()?,
        applications_dir: system::paths::applications_dir()?,
        home,
        sandbox: system::Sandbox::detect(),
    };
    let settings_path = system::paths::app_config_dir()?.join(settings::FILE_NAME);
    Ok((env, settings_path))
}

fn run_window(
    env: Env,
    settings_path: PathBuf,
    startup: Option<app::Dialog>,
    args: &Args,
) -> eframe::Result {
    let (measure_startup, screen, demo) = (args.measure_startup, args.screen, args.demo);
    let demo_dialog = args.demo_dialog.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(text::APP_NAME)
            .with_app_id(system::APP_ID)
            .with_inner_size(theme::size::WINDOW)
            .with_min_inner_size(theme::size::WINDOW_MIN)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        text::APP_NAME,
        options,
        Box::new(move |cc| {
            let mut app = app::LauncherApp::new(cc, env, settings_path, startup)?;
            if demo {
                app.enable_demo();
                if let Some(name) = &demo_dialog {
                    app.open_demo_dialog(name);
                }
            }
            if let Some(screen) = screen {
                app.screen = screen;
            }
            if measure_startup {
                Ok(Box::new(MeasureStartup(app)))
            } else {
                Ok(Box::new(app))
            }
        }),
    )
}

/// The app logo as the window icon (an empty icon if it cannot be decoded).
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).unwrap_or_default()
}

/// Wraps the app to close the window after the first frame.
struct MeasureStartup(app::LauncherApp);

impl eframe::App for MeasureStartup {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.0.ui(ui, frame);
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
    }
}
