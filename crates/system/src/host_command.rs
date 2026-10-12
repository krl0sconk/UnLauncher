// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::sandbox::Sandbox;

/// A process to run on the host system, or one of UnLauncher's own programs.
///
/// [`HostCommand::new`] runs a host program: directly outside Flatpak, and
/// through `flatpak-spawn --host` inside it, with the working directory and
/// environment passed as `flatpak-spawn` options so they apply to the host
/// process. [`HostCommand::bundled`] runs a program shipped with UnLauncher
/// (such as `unlauncher-web`) directly, inside the sandbox. Arguments are
/// always a list, never a shell string.
///
/// ```
/// use system::{HostCommand, Sandbox};
///
/// let command = HostCommand::new("UnrealEditor")
///     .arg("/home/me/Game/Game.uproject")
///     .current_dir("/opt/UE_5.7")
///     .to_command(Sandbox::Flatpak);
/// assert_eq!(command.get_program(), "flatpak-spawn");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct HostCommand {
    program: OsString,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    envs: Vec<(OsString, OsString)>,
    /// One of UnLauncher's own programs: never goes through `flatpak-spawn`.
    is_bundled: bool,
}

impl HostCommand {
    /// A command that runs `program` (a path or a name looked up in `PATH`).
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            current_dir: None,
            envs: Vec::new(),
            is_bundled: false,
        }
    }

    /// One of UnLauncher's own programs, found next to the running executable
    /// (`/app/bin/` inside Flatpak, `target/<profile>/` during development).
    ///
    /// It always runs directly, inside the sandbox, whatever the [`Sandbox`]:
    /// it is part of UnLauncher, not a host program.
    ///
    /// # Errors
    ///
    /// [`Error::BundledProgramMissing`] if the running executable cannot be
    /// found or there is no `name` next to it.
    pub fn bundled(name: &str) -> Result<Self> {
        let exe = std::env::current_exe().map_err(|_| Error::BundledProgramMissing {
            program: name.to_owned(),
            path: PathBuf::from(name),
        })?;
        let dir = exe.parent().unwrap_or_else(|| Path::new("/"));
        Self::bundled_in(dir, name)
    }

    /// [`HostCommand::bundled`] looking in an explicit folder.
    ///
    /// # Errors
    ///
    /// [`Error::BundledProgramMissing`] if `dir/name` is not a file.
    pub fn bundled_in(dir: &Path, name: &str) -> Result<Self> {
        let path = dir.join(name);
        if !path.is_file() {
            return Err(Error::BundledProgramMissing {
                program: name.to_owned(),
                path,
            });
        }
        let mut command = Self::new(path);
        command.is_bundled = true;
        Ok(command)
    }

    /// Whether this is one of UnLauncher's own programs (see
    /// [`HostCommand::bundled`]).
    #[must_use]
    pub fn is_bundled(&self) -> bool {
        self.is_bundled
    }

    /// Adds an argument.
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    /// Adds several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    /// Sets the working directory of the host process.
    pub fn current_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.current_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sets an environment variable for the host process.
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.envs
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    /// Builds the [`Command`] for `sandbox`, so callers can configure stdio
    /// before spawning (for example, to stream build output). A bundled
    /// command ignores `sandbox` and always runs directly.
    #[must_use]
    pub fn to_command(&self, sandbox: Sandbox) -> Command {
        let sandbox = if self.is_bundled {
            Sandbox::Host
        } else {
            sandbox
        };
        match sandbox {
            Sandbox::Host => {
                let mut command = Command::new(&self.program);
                command.args(&self.args);
                if let Some(dir) = &self.current_dir {
                    command.current_dir(dir);
                }
                command.envs(self.envs.iter().map(|(k, v)| (k, v)));
                command
            }
            Sandbox::Flatpak => {
                let mut command = Command::new("flatpak-spawn");
                command.arg("--host");
                if let Some(dir) = &self.current_dir {
                    let mut option = OsString::from("--directory=");
                    option.push(dir);
                    command.arg(option);
                }
                for (key, value) in &self.envs {
                    let mut option = OsString::from("--env=");
                    option.push(key);
                    option.push("=");
                    option.push(value);
                    command.arg(option);
                }
                command.arg(&self.program).args(&self.args);
                command
            }
        }
    }

    /// Starts the process in the detected sandbox, inheriting stdio.
    ///
    /// # Errors
    ///
    /// [`Error::SpawnFailed`] if the process cannot be started.
    pub fn spawn(&self) -> Result<Child> {
        self.spawn_in(Sandbox::detect())
    }

    /// Starts the process for an explicit `sandbox`, inheriting stdio.
    ///
    /// # Errors
    ///
    /// [`Error::SpawnFailed`] if the process cannot be started.
    pub fn spawn_in(&self, sandbox: Sandbox) -> Result<Child> {
        tracing::debug!(program = ?self.program, args = ?self.args, ?sandbox, "spawning host command");
        self.to_command(sandbox)
            .spawn()
            .map_err(|source| Error::SpawnFailed {
                program: self.program.to_string_lossy().into_owned(),
                source,
            })
    }

    /// Runs the process for an explicit `sandbox` with stdin, stdout and
    /// stderr on `/dev/null`, and waits for it to end. Used to watch the
    /// editor and notice when it crashes.
    ///
    /// Inside Flatpak the status is `flatpak-spawn`'s, which mirrors the host
    /// process's exit code.
    ///
    /// # Errors
    ///
    /// [`Error::SpawnFailed`] if the process cannot be started or waited for.
    pub fn wait_quiet_in(&self, sandbox: Sandbox) -> Result<ExitStatus> {
        tracing::debug!(program = ?self.program, args = ?self.args, ?sandbox, "running host command and waiting");
        let spawn_failed = |source| Error::SpawnFailed {
            program: self.program.to_string_lossy().into_owned(),
            source,
        };
        self.to_command(sandbox)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(spawn_failed)?
            .wait()
            .map_err(spawn_failed)
    }

    /// Runs the process for an explicit `sandbox`, writes `input` to its stdin
    /// (then closes it) and collects stdout and stderr. Stops it after
    /// `timeout`.
    ///
    /// For short commands whose output matters, such as `gh auth token`. The
    /// output can hold secrets: callers must not log it.
    ///
    /// # Errors
    ///
    /// [`Error::SpawnFailed`] if the process cannot be started or waited for,
    /// [`Error::TimedOut`] if it runs longer than `timeout` (it is killed).
    pub fn output_in(&self, sandbox: Sandbox, input: &[u8], timeout: Duration) -> Result<Output> {
        let program = self.program.to_string_lossy().into_owned();
        let spawn_failed = |source| Error::SpawnFailed {
            program: program.clone(),
            source,
        };
        let mut child = self
            .to_command(sandbox)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_failed)?;
        // Write and read on threads so a full pipe never blocks the timeout.
        let stdin = child.stdin.take().map(|mut stdin| {
            let input = input.to_vec();
            thread::spawn(move || {
                let _ = stdin.write_all(&input);
            })
        });
        let read = |pipe: Option<Box<dyn Read + Send>>| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                bytes
            })
        };
        let stdout = read(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let stderr = read(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );

        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(spawn_failed)? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::TimedOut { program });
            }
            thread::sleep(Duration::from_millis(20));
        };
        if let Some(stdin) = stdin {
            let _ = stdin.join();
        }
        Ok(Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    /// Starts the process in the detected sandbox, detached from UnLauncher.
    ///
    /// See [`HostCommand::spawn_detached_in`].
    ///
    /// # Errors
    ///
    /// [`Error::SpawnFailed`] if the process cannot be started.
    pub fn spawn_detached(&self) -> Result<()> {
        self.spawn_detached_in(Sandbox::detect())
    }

    /// Starts the process for an explicit `sandbox` and forgets about it.
    ///
    /// stdin, stdout and stderr go to `/dev/null`. The child is reaped on a
    /// background thread so it never stays as a zombie in the long-running
    /// interface. Use it for fire-and-forget launches such as opening the
    /// editor; use [`HostCommand::spawn_in`] when the output matters.
    ///
    /// # Errors
    ///
    /// [`Error::SpawnFailed`] if the process cannot be started.
    pub fn spawn_detached_in(&self, sandbox: Sandbox) -> Result<()> {
        tracing::debug!(program = ?self.program, args = ?self.args, ?sandbox, "spawning detached host command");
        let mut child = self
            .to_command(sandbox)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| Error::SpawnFailed {
                program: self.program.to_string_lossy().into_owned(),
                source,
            })?;
        let program = self.program.to_string_lossy().into_owned();
        let reaper = thread::Builder::new()
            .name("unlauncher-reaper".to_owned())
            .spawn(move || match child.wait() {
                Ok(status) => tracing::debug!(%program, %status, "detached host command exited"),
                Err(error) => {
                    tracing::warn!(%program, %error, "failed to wait for detached host command");
                }
            });
        if let Err(error) = reaper {
            // The process is already running; only its exit status is lost.
            tracing::warn!(%error, "failed to start the reaper thread for a detached host command");
        }
        Ok(())
    }
}
