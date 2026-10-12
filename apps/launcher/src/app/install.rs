// SPDX-License-Identifier: GPL-3.0-or-later

//! Installing, building and maintaining engines.

#[allow(clippy::wildcard_imports, reason = "a part of the app module")]
use super::*;

impl LauncherApp {
    pub(super) fn open_maintenance(&mut self, engine: Engine) {
        let env = self.env.clone();
        let measured = engine.clone();
        self.dialog = Some(Dialog::Maintenance(MaintenanceForm {
            engine,
            usage: None,
        }));
        self.spawn(move || {
            Event::Usage(measured.id.clone(), backend::engine_usage(&env, &measured))
        });
    }

    pub(super) fn open_install(&mut self) {
        self.dialog = Some(Dialog::InstallEngine(InstallForm {
            mode: InstallMode::Prebuilt,
            zip: None,
            destination: self.env.home.join("UnrealEngine"),
            git_ref: "release".to_owned(),
            source_destination: self.env.home.join("UnrealEngine/source"),
            jobs: String::new(),
            token_input: String::new(),
            preflight: None,
            checking: false,
        }));
        self.refresh_github();
    }

    pub(super) fn run_preflight(&mut self) {
        let Some(Dialog::InstallEngine(form)) = &mut self.dialog else {
            return;
        };
        form.checking = true;
        let input = ue_build::PreflightInput::new(&form.source_destination);
        let http = self.http.clone();
        let sandbox = self.env.sandbox;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.runtime.spawn(async move {
            use epic_auth::{CredentialKey, KeyringStore, SecretStore};
            let token =
                tokio::task::spawn_blocking(|| KeyringStore.get(CredentialKey::GitHubToken))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten();
            let results = ue_build::preflight(&input, sandbox, &http, token.as_ref()).await;
            let _ = tx.send(Event::Preflight(results));
            ctx.request_repaint();
        });
    }

    pub(super) fn start_build(&mut self) {
        let Some(Dialog::InstallEngine(form)) = &self.dialog else {
            return;
        };
        let task = crate::tasks::BuildTask {
            git_ref: form.git_ref.trim().to_owned(),
            destination: form.source_destination.clone(),
            jobs: form.jobs.trim().parse().ok().filter(|jobs| *jobs > 0),
            paths: backend::task_paths(&self.env),
            logs_dir: self.env.logs_dir.clone(),
        };
        self.dialog = None;
        self.tasks.submit(Box::new(task));
        self.screen = Screen::Tasks;
    }

    pub(super) fn start_import(&mut self, zip: PathBuf, destination: PathBuf) {
        self.dialog = None;
        let task = crate::tasks::ImportTask {
            zip,
            destination,
            id: None,
            paths: backend::task_paths(&self.env),
        };
        self.tasks.submit(Box::new(task));
        self.screen = Screen::Tasks;
    }

    pub(super) fn handle_install(&mut self, intent: Intent) {
        match intent {
            Intent::InstallEngine => self.open_install(),
            Intent::PickZip => self.pick_zip(),
            Intent::PickInstallDestination => {
                self.pick_folder(
                    Pick::InstallDestination,
                    "Choose where to install the engine",
                );
            }
            Intent::StartImport { zip, destination } => self.start_import(zip, destination),
            Intent::SetInstallMode(mode) => {
                if let Some(Dialog::InstallEngine(form)) = &mut self.dialog {
                    form.mode = mode;
                }
            }
            Intent::PickSourceDestination => {
                self.pick_folder(
                    Pick::SourceDestination,
                    "Choose where to put the engine source",
                );
            }
            Intent::RunPreflight => self.run_preflight(),
            Intent::StartBuild => self.start_build(),
            _ => {}
        }
    }
}
