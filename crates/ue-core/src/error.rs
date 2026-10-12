// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::path::PathBuf;

use common::UserFacing;

use crate::engine::EngineId;

/// Errors returned by `ue-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file could not be read.
    #[error("failed to read {}", path.display())]
    ReadFile {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A file or directory could not be written.
    #[error("failed to write {}", path.display())]
    WriteFile {
        /// The file or directory that could not be written.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A file exists and the caller did not pass `Overwrite::Yes`.
    #[error("{} already exists and overwriting it was not confirmed", path.display())]
    OverwriteNotConfirmed {
        /// The file that would have been overwritten.
        path: PathBuf,
    },

    /// A deletion was requested without `Delete::Yes`.
    #[error("deleting {} was not confirmed", path.display())]
    DeleteNotConfirmed {
        /// What would have been deleted.
        path: PathBuf,
    },

    /// A project folder also holds other things, so it is not deleted.
    #[error("{} holds more than one project; it was not deleted", path.display())]
    NotOnlyThisProject {
        /// The folder.
        path: PathBuf,
    },

    /// A file or folder could not be deleted.
    #[error("failed to delete {}", path.display())]
    DeleteFile {
        /// The file or folder.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A directory is not an Unreal Engine installation.
    #[error("{} is not an Unreal Engine directory: {} not found", path.display(), missing.display())]
    NotAnEngine {
        /// The directory that was checked.
        path: PathBuf,
        /// The file that was expected and not found.
        missing: PathBuf,
    },

    /// `Build.version` exists but is not valid.
    #[error("{} is not a valid Build.version file", path.display())]
    BuildVersionMalformed {
        /// The `Build.version` file.
        path: PathBuf,
        /// The JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// A `.uproject` file exists but is not valid.
    #[error("{} is not a valid .uproject file: {reason}", path.display())]
    UProjectMalformed {
        /// The `.uproject` file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },

    /// A path cannot be written to `Install.ini` because it is not valid UTF-8.
    #[error("{} is not valid UTF-8 and cannot be stored in Install.ini", path.display())]
    NonUtf8Path {
        /// The offending path.
        path: PathBuf,
    },

    /// An engine id cannot be used in `Install.ini`.
    #[error("invalid engine id {id:?}: {reason}")]
    InvalidEngineId {
        /// The rejected id.
        id: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// Another engine is already registered with this id.
    #[error("engine id {id} is already registered for {}", path.display())]
    EngineIdTaken {
        /// The id in use.
        id: EngineId,
        /// The engine that uses it.
        path: PathBuf,
    },

    /// The engine directory is already registered under another id.
    #[error("{} is already registered as {id}", path.display())]
    EngineAlreadyRegistered {
        /// The id it is registered under.
        id: EngineId,
        /// The engine directory.
        path: PathBuf,
    },

    /// No engine is registered with this id.
    #[error("no engine is registered with id {id}")]
    EngineNotRegistered {
        /// The id that was looked up.
        id: EngineId,
    },

    /// A name cannot be used for a new Unreal project.
    #[error("invalid project name {name:?}: {reason}")]
    InvalidProjectName {
        /// The rejected name.
        name: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// The template contains C++ code, which UnLauncher cannot set up yet.
    #[error("template {template} contains C++ code and cannot be used to create a project yet")]
    CodeTemplateUnsupported {
        /// Id (folder name) of the template.
        template: String,
    },

    /// The folder for a new project already exists.
    #[error("{} already exists", path.display())]
    ProjectExists {
        /// The folder the project would have been created in.
        path: PathBuf,
    },

    /// A template or one of the content packs it uses is not valid.
    #[error("{} is not a valid template file: {reason}", path.display())]
    TemplateMalformed {
        /// The offending file or folder.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },

    /// UnLauncher's engine metadata file is not valid TOML.
    #[error("{} is not a valid engine metadata file", path.display())]
    MetadataMalformed {
        /// The metadata file.
        path: PathBuf,
        /// The TOML error.
        #[source]
        source: toml::de::Error,
    },

    /// The engine metadata cannot be written as TOML.
    #[error("cannot write engine metadata to {}", path.display())]
    MetadataUnserializable {
        /// The metadata file.
        path: PathBuf,
        /// The TOML error.
        #[source]
        source: toml::ser::Error,
    },
}

/// Result type of `ue-core`.
pub type Result<T> = std::result::Result<T, Error>;

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::ReadFile { .. } => {
                "Check that the file exists and that you have permission to read it."
            }
            Self::WriteFile { .. } => {
                "Check that you have permission to write there and that the disk is not full."
            }
            Self::OverwriteNotConfirmed { .. } => {
                "Confirm the change to overwrite the file; a copy is kept next to it as *.bak."
            }
            Self::DeleteNotConfirmed { .. } => "Confirm the deletion to continue.",
            Self::NotOnlyThisProject { .. } => {
                "Move the project into its own folder, or delete it with your file manager."
            }
            Self::DeleteFile { .. } => {
                "Close every Unreal Editor and check that you own the files, then try again."
            }
            Self::NotAnEngine { .. } => {
                "Choose the folder that contains the `Engine` directory of an Unreal Engine installation."
            }
            Self::BuildVersionMalformed { .. } => {
                "The engine installation may be incomplete or damaged; reinstall or rebuild it."
            }
            Self::UProjectMalformed { .. } => {
                "Fix the file in a text editor or restore it from a backup (*.bak)."
            }
            Self::NonUtf8Path { .. } => {
                "Rename or move the folder to a path without invalid characters."
            }
            Self::InvalidEngineId { .. } => {
                "Ids cannot be empty, contain '=' or line breaks, or start with '[', ';' or '#'."
            }
            Self::EngineIdTaken { .. } => "Choose a different id.",
            Self::EngineAlreadyRegistered { .. } => return None,
            Self::EngineNotRegistered { .. } => "List the registered engines to see the valid ids.",
            Self::InvalidProjectName { .. } => {
                "Use up to 20 ASCII letters, digits or underscores, starting with a letter."
            }
            Self::CodeTemplateUnsupported { .. } => {
                "C++ templates need their code modules renamed, which UnLauncher does not do yet. \
                 Choose a Blueprint template, or create the project from the Unreal Editor."
            }
            Self::ProjectExists { .. } => {
                "Choose another name or folder; existing files are never overwritten."
            }
            Self::TemplateMalformed { .. } => {
                "The engine installation may be incomplete or damaged; verify or reinstall it."
            }
            Self::MetadataMalformed { .. } => {
                "Fix the file in a text editor, or delete it: engines will show as \"unknown\" \
                 until UnLauncher records how they were installed again."
            }
            Self::MetadataUnserializable { .. } => {
                "An engine folder path is not valid UTF-8; rename or move it."
            }
        };
        Some(hint.to_owned())
    }
}
