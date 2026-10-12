// SPDX-License-Identifier: GPL-3.0-or-later

//! Turning intents and worker events into state changes.

#[allow(clippy::wildcard_imports, reason = "a part of the app module")]
use super::*;

impl LauncherApp {
    pub(super) fn finish_first_run(&mut self, paths: Vec<PathBuf>) {
        self.dialog = None;
        self.settings.first_run_done = true;
        self.save_settings();
        if !paths.is_empty() {
            self.run(true, move |env| backend::register_engines(env, &paths));
        }
    }

    /// Removes every finished task from the list.
    pub(super) fn clear_finished(&mut self) {
        for task in self.tasks.list() {
            if task.state.is_finished()
                && let Err(error) = self.tasks.remove(task.id)
            {
                tracing::warn!(%error, "cannot remove a finished task");
            }
        }
    }

    pub(super) fn task_op(&mut self, id: jobs::TaskId, op: TaskOp) {
        let result = match op {
            TaskOp::Pause => self.tasks.pause(id),
            TaskOp::Resume => self.tasks.resume(id),
            TaskOp::Cancel => self.tasks.cancel(id),
            TaskOp::Retry => self.tasks.retry(id).map(|_| ()),
            TaskOp::Remove => self.tasks.remove(id),
        };
        if let Err(error) = result {
            self.dialog = Some(Dialog::Error(UiError::from(error)));
        }
    }

    fn fab_versions(&mut self, result: Result<Vec<String>, UiError>) {
        match result {
            Ok(versions) => {
                tracing::info!(count = versions.len(), "read engine versions from Fab");
                self.settings.fab_versions = versions;
                self.settings.last_update_check = Some(unix_now());
                self.save_settings();
            }
            Err(error) => {
                tracing::warn!(error = %error.summary, "cannot check Fab for a new engine version");
            }
        }
    }

    pub(super) fn save_settings(&mut self) {
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = Some(Err(UiError::new(format!("{error:#}"))));
        }
    }

    fn snapshot_received(&mut self, snapshot: Snapshot) {
        self.loading = false;
        // A project may have a new thumbnail or screenshot.
        self.images.forget_files();
        if !self.settings.first_run_done && self.dialog.is_none() {
            let has_entries = snapshot
                .registry
                .as_ref()
                .is_ok_and(|r| !r.engines().is_empty() || !r.invalid_entries().is_empty());
            if snapshot.unregistered.is_empty() && has_entries {
                // Nothing new to offer: the registered engines are ready.
                self.settings.first_run_done = true;
                self.save_settings();
            } else {
                let found = snapshot
                    .unregistered
                    .iter()
                    .map(|engine| (engine.clone(), true))
                    .collect();
                self.dialog = Some(Dialog::FirstRun(found));
            }
        }
        self.snapshot = Some(snapshot);
    }

    pub(super) fn receive(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Snapshot(snapshot) => self.snapshot_received(snapshot),
                Event::Done { result, reload } => {
                    self.busy = self.busy.saturating_sub(1);
                    if let Err(error) = &result {
                        self.dialog = Some(Dialog::Error(error.clone()));
                    }
                    self.status = Some(result);
                    if reload {
                        self.reload();
                    }
                }
                Event::Picked(pick, Some(folder)) => self.picked(pick, folder),
                Event::Picked(_, None) => {}
                Event::Templates(engine_id, templates) => {
                    if let Some(Dialog::NewProject(form)) = &mut self.dialog
                        && form.engine.as_ref().is_some_and(|e| e.id == engine_id)
                    {
                        form.template = templates
                            .as_ref()
                            .ok()
                            .and_then(|t| t.iter().position(|t| !t.has_code));
                        form.templates = Some(templates);
                    }
                }
                Event::Github(event) => self.github_event(event),
                Event::Usage(id, usage) => {
                    if let Some(Dialog::Maintenance(form)) = &mut self.dialog
                        && form.engine.id == id
                    {
                        form.usage = Some(usage);
                    }
                }
                Event::Session(result) => self.session_changed(result),
                Event::WebLogin(progress) => self.web_login(progress),
                Event::Library(result) => {
                    self.library.loading = false;
                    self.library.assets = Some(result);
                }
                Event::EngineDownloads(result) => {
                    self.library.listing_downloads = false;
                    self.library.engine_downloads = Some(result);
                }
                Event::FabVersions(result) => self.fab_versions(result),
                Event::FabWindow(event) => self.fab_window_event(&event),
                Event::EngineZipFound(found) => {
                    self.watching_downloads = false;
                    self.found_zip = found;
                }
                Event::DownloadsWindowUnavailable(error) => {
                    self.downloads_window_unavailable(error);
                }
                Event::Preflight(results) => {
                    if let Some(Dialog::InstallEngine(form)) = &mut self.dialog {
                        form.checking = false;
                        form.preflight = Some(results);
                    }
                }
                Event::Created(result) => match result {
                    Ok(path) => {
                        self.dialog = Some(Dialog::Created(path.clone()));
                        if let Some(parent) = path.parent().and_then(|p| p.parent()) {
                            self.add_project_folder(parent.to_path_buf());
                        }
                        self.reload();
                    }
                    Err(error) => {
                        if let Some(Dialog::NewProject(form)) = &mut self.dialog {
                            form.creating = false;
                            form.error = Some(error);
                        }
                    }
                },
            }
        }
    }

    pub(super) fn picked(&mut self, pick: Pick, folder: PathBuf) {
        match pick {
            Pick::Engine => {
                self.dialog = Some(Dialog::Confirm {
                    title: text::REGISTER_ENGINE.trim_end_matches('…').to_owned(),
                    body: format!(
                        "Register {} in {}?\n{}",
                        folder.display(),
                        self.env.install_ini.display(),
                        text::BACKUP_NOTE
                    ),
                    action: Action::Register(vec![folder]),
                });
            }
            Pick::ProjectFolder => {
                self.add_project_folder(folder);
                self.reload();
            }
            Pick::NewProjectLocation => {
                if let Some(Dialog::NewProject(form)) = &mut self.dialog {
                    form.location = folder;
                }
            }
            Pick::Zip => {
                if let Some(Dialog::InstallEngine(form)) = &mut self.dialog {
                    if let Some(stem) = folder.file_stem() {
                        form.destination = self.env.home.join("UnrealEngine").join(stem);
                    }
                    form.zip = Some(folder);
                }
            }
            Pick::InstallDestination => {
                if let Some(Dialog::InstallEngine(form)) = &mut self.dialog {
                    form.destination = folder;
                }
            }
            Pick::SourceDestination => {
                if let Some(Dialog::InstallEngine(form)) = &mut self.dialog {
                    form.source_destination = folder;
                    form.preflight = None;
                }
            }
        }
    }

    pub(super) fn add_project_folder(&mut self, folder: PathBuf) {
        let defaults = system::paths::default_project_search_paths(&self.env.home);
        if !self.settings.project_folders.contains(&folder) && !defaults.contains(&folder) {
            self.settings.project_folders.push(folder);
            self.save_settings();
        }
    }

    pub(super) fn execute(&mut self, action: Action) {
        self.dialog = None;
        match action {
            Action::Register(paths) => {
                self.run(true, move |env| backend::register_engines(env, &paths));
            }
            Action::Update {
                engine_root,
                git_ref,
            } => {
                let task = crate::tasks::UpdateTask {
                    engine_root,
                    git_ref,
                    jobs: None,
                    paths: backend::task_paths(&self.env),
                    logs_dir: self.env.logs_dir.clone(),
                };
                self.tasks.submit(Box::new(task));
                self.screen = Screen::Tasks;
            }
            Action::CleanCache(path) => {
                self.run(false, move |_| backend::clean_cache(&path));
            }
            Action::DeleteProject(path) => {
                self.run(true, move |_| backend::delete_project(&path));
            }
            Action::Uninstall(id) => {
                self.run(true, move |env| backend::uninstall(env, &id));
            }
            Action::Unregister(id) => {
                self.run(true, move |env| backend::unregister_engine(env, &id));
            }
            Action::Associate { project, engine } => {
                self.run(true, move |env| backend::associate(env, &project, &engine));
            }
        }
    }

    pub(super) fn new_project(&mut self) {
        let engine = self
            .snapshot
            .as_ref()
            .and_then(|s| s.engines().iter().max_by_key(|e| e.version).cloned());
        self.dialog = Some(Dialog::NewProject(NewProjectForm {
            engine: engine.clone(),
            templates: None,
            template: None,
            name: "MyProject".to_owned(),
            location: self.env.home.join("Documents/Unreal Projects"),
            creating: false,
            error: None,
        }));
        if let Some(engine) = engine {
            self.handle(Intent::LoadTemplates(engine));
        }
    }

    /// Intents that only open a dialog.
    fn open_dialog(&mut self, intent: Intent) {
        match intent {
            Intent::Ask {
                title,
                body,
                action,
            } => {
                self.dialog = Some(Dialog::Confirm {
                    title,
                    body,
                    action,
                });
            }
            Intent::AskTyped {
                title,
                body,
                expected,
                action,
            } => {
                self.dialog = Some(Dialog::TypedConfirm {
                    title,
                    body,
                    expected,
                    typed: String::new(),
                    action,
                });
            }
            Intent::ChangeEngine {
                project,
                name,
                current,
            } => {
                self.dialog = Some(Dialog::ChangeEngine {
                    project,
                    name,
                    selected: current,
                });
            }
            _ => {}
        }
    }

    /// Intents that jump to another action: open a page or file, start the
    /// editor again, or open the Install dialog prefilled.
    fn handle_shortcut(&mut self, intent: Intent) {
        match intent {
            Intent::OpenUrl(url) => {
                if let Err(error) = system::open_url(&url) {
                    self.status = Some(Err(error.into()));
                }
            }
            Intent::Relaunch(target) => {
                self.dialog = None;
                self.run(false, move |env| backend::relaunch(env, &target));
            }
            Intent::OpenFile(path) => self.run(false, move |env| backend::open_file(env, &path)),
            Intent::ChooseZip(zip) => {
                self.open_install();
                self.picked(Pick::Zip, zip);
            }
            Intent::DownloadNewEngine => {
                self.open_install();
                self.handle_epic(Intent::ListEngineDownloads);
            }
            _ => {}
        }
    }

    /// Intents of the New project dialog.
    fn handle_new_project(&mut self, intent: Intent) {
        match intent {
            Intent::LoadTemplates(engine) => {
                if let Some(Dialog::NewProject(form)) = &mut self.dialog {
                    form.engine = Some(engine.clone());
                    form.templates = None;
                    form.template = None;
                }
                self.spawn(move || {
                    Event::Templates(engine.id.clone(), backend::templates(&engine))
                });
            }
            Intent::CreateProject {
                engine,
                template,
                parent,
                name,
            } => {
                if let Some(Dialog::NewProject(form)) = &mut self.dialog {
                    form.creating = true;
                    form.error = None;
                }
                self.spawn(move || {
                    Event::Created(backend::create_project(&engine, &template, &parent, &name))
                });
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines, reason = "one arm per intent")]
    pub(super) fn handle(&mut self, intent: Intent) {
        match intent {
            Intent::Navigate(screen) => {
                if screen == Screen::Settings && self.github.saved.is_none() {
                    self.refresh_github();
                }
                self.screen = screen;
            }
            Intent::PickEngineFolder => self.pick_folder(Pick::Engine, "Choose the engine folder"),
            Intent::PickProjectFolder => {
                self.pick_folder(Pick::ProjectFolder, "Choose a folder with projects");
            }
            Intent::PickNewProjectLocation => {
                self.pick_folder(
                    Pick::NewProjectLocation,
                    "Choose where to create the project",
                );
            }
            Intent::RemoveProjectFolder(folder) => {
                self.settings.project_folders.retain(|f| f != &folder);
                self.save_settings();
                self.reload();
            }
            Intent::Execute(action) => self.execute(action),
            intent @ (Intent::Ask { .. }
            | Intent::AskTyped { .. }
            | Intent::ChangeEngine { .. }) => self.open_dialog(intent),
            Intent::LaunchEngine(engine) => {
                self.remember_engine(&engine.id);
                self.run(false, move |env| backend::launch_editor(env, &engine, None));
            }
            Intent::AddShortcut(engine) => {
                self.run(false, move |env| backend::add_engine_shortcut(env, &engine));
            }
            Intent::OpenProject(path) => {
                let engine = self.snapshot.as_ref().and_then(|snapshot| {
                    snapshot
                        .projects
                        .iter()
                        .flatten()
                        .find(|p| p.path == path)
                        .and_then(|p| snapshot.engine_for(p))
                });
                if let Some(id) = engine.map(|engine| engine.id.clone()) {
                    self.remember_engine(&id);
                }
                self.run(false, move |env| backend::open_project(env, &path));
            }
            Intent::NewProject => self.new_project(),
            intent @ (Intent::LoadTemplates(_) | Intent::CreateProject { .. }) => {
                self.handle_new_project(intent);
            }
            Intent::FinishFirstRun(paths) => self.finish_first_run(paths),
            intent @ (Intent::InstallEngine
            | Intent::PickZip
            | Intent::PickInstallDestination
            | Intent::StartImport { .. }
            | Intent::SetInstallMode(_)
            | Intent::PickSourceDestination
            | Intent::RunPreflight
            | Intent::StartBuild) => self.handle_install(intent),
            Intent::Maintenance(engine) => self.open_maintenance(engine),
            intent @ (Intent::OpenLoginPage
            | Intent::OpenEngineDownloadPage
            | Intent::SignInWithWindow
            | Intent::CancelWindowLogin
            | Intent::UseBrowserLogin
            | Intent::UseWindowLogin
            | Intent::SignIn
            | Intent::SignOut
            | Intent::RefreshLibrary
            | Intent::InstallAsset(_)
            | Intent::StartFabInstall
            | Intent::ListEngineDownloads
            | Intent::StartEngineDownload(_)) => self.handle_epic(intent),
            intent @ (Intent::SaveGitHubToken(_)
            | Intent::GithubRefresh
            | Intent::GithubConnect
            | Intent::GithubCancelDevice
            | Intent::GithubGhLogin
            | Intent::GithubUseExisting(_)
            | Intent::GithubDisconnect
            | Intent::OpenEpicConnections
            | Intent::OpenGithubInvitation) => self.handle_github(intent),
            intent @ (Intent::OpenUrl(_)
            | Intent::Relaunch(_)
            | Intent::OpenFile(_)
            | Intent::ChooseZip(_)
            | Intent::DownloadNewEngine) => self.handle_shortcut(intent),
            Intent::Task(id, op) => self.task_op(id, op),
            Intent::ClearFinished => self.clear_finished(),
            Intent::TaskDetails { title, error } => {
                self.dialog = Some(Dialog::TaskDetails {
                    title,
                    error,
                    has_logs: self.env.logs_dir.is_dir(),
                });
            }
            Intent::OpenLogsFolder => {
                let logs = self.env.logs_dir.clone();
                self.run(false, move |env| backend::open_file(env, &logs));
            }
            Intent::SetTheme(choice) => {
                self.settings.theme = choice;
                crate::theme::set_choice(&self.ctx, choice);
                self.save_settings();
            }
            Intent::DismissNewVersion => self.new_version_dismissed = true,
            Intent::GetMoreOnFab => self.get_more_on_fab(),
            Intent::CloseDialog => self.dialog = None,
        }
    }
}
