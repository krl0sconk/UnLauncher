// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::path::PathBuf;

use common::UserFacing;

/// Errors returned by `ue-prebuilt`.
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

    /// The file is not a zip archive, or its index is damaged.
    #[error("{} is not a valid zip archive", path.display())]
    InvalidArchive {
        /// The archive.
        path: PathBuf,
        /// The zip error.
        #[source]
        source: zip::result::ZipError,
    },

    /// An entry of the archive could not be read or decompressed.
    #[error("failed to extract {entry} from {}", archive.display())]
    ReadEntry {
        /// The archive.
        archive: PathBuf,
        /// Name of the entry inside the archive.
        entry: String,
        /// The zip error.
        #[source]
        source: zip::result::ZipError,
    },

    /// An entry would be written outside the destination (an absolute path or
    /// one that climbs out with `..`). Nothing is extracted.
    #[error("{} contains an unsafe entry: {entry}", archive.display())]
    UnsafeEntry {
        /// The archive.
        archive: PathBuf,
        /// Name of the entry inside the archive.
        entry: String,
    },

    /// The destination exists and is not an empty directory.
    #[error("{} already exists and is not an empty folder", path.display())]
    DestinationNotEmpty {
        /// The destination.
        path: PathBuf,
    },

    /// The destination cannot hold an engine (for example `/`).
    #[error("{} cannot be used as an engine folder", path.display())]
    InvalidDestination {
        /// The destination.
        path: PathBuf,
    },

    /// There is not enough free space to extract the archive.
    #[error(
        "not enough free space in {}: {} needed, {} available",
        path.display(),
        format_size(*needed),
        format_size(*available)
    )]
    NotEnoughSpace {
        /// The folder whose file system was checked.
        path: PathBuf,
        /// Bytes needed (total uncompressed size).
        needed: u64,
        /// Bytes available to the user.
        available: u64,
    },

    /// The archive was extracted but does not contain an engine. What was
    /// extracted has been removed.
    #[error("{} does not contain an Unreal Engine installation", path.display())]
    NotAnEngineArchive {
        /// The archive.
        path: PathBuf,
        /// Why the extracted folder is not an engine.
        #[source]
        source: Box<ue_core::Error>,
    },

    /// The task was cancelled. An import removes what it extracted; a
    /// download keeps its `.part` file so it can be resumed.
    #[error("cancelled")]
    Cancelled,

    /// The list of downloads from unrealengine.com does not have the
    /// expected shape. The body is not quoted: it holds signed URLs.
    #[error("Epic's download page answered with something unexpected")]
    UnexpectedBlobs,

    /// Fab's list of engine versions does not have the expected shape.
    #[error("Fab's list of engine versions has an unexpected format")]
    UnexpectedVersions,

    /// The download could not be started or was interrupted.
    #[error("failed to download {file_name}")]
    Download {
        /// Name of the file being downloaded.
        file_name: String,
        /// The HTTP error, with the (signed) URL stripped.
        #[source]
        source: reqwest::Error,
    },

    /// The download server answered with an unexpected status.
    #[error("the download server answered {status} for {file_name}")]
    HttpStatus {
        /// Name of the file being downloaded.
        file_name: String,
        /// HTTP status code.
        status: u16,
    },

    /// The download link is not `https` or its host is not one Epic serves
    /// engines from. Nothing was requested.
    #[error("the download link for {file_name} points to an untrusted host ({host})")]
    UntrustedDownloadUrl {
        /// Name of the file.
        file_name: String,
        /// The link's host (never the whole URL, which is signed).
        host: String,
    },

    /// A complete file already exists at the download destination.
    #[error("{} already exists", path.display())]
    DestinationExists {
        /// The destination.
        path: PathBuf,
    },

    /// The downloaded file does not have the expected size.
    #[error(
        "{} has {actual} bytes but {expected} were expected",
        path.display()
    )]
    SizeMismatch {
        /// The partial file (`.part`).
        path: PathBuf,
        /// Bytes expected.
        expected: u64,
        /// Bytes on disk.
        actual: u64,
    },

    /// Registering the engine failed.
    #[error(transparent)]
    Engine(Box<ue_core::Error>),
}

impl From<ue_core::Error> for Error {
    fn from(error: ue_core::Error) -> Self {
        Self::Engine(Box::new(error))
    }
}

/// Result type of `ue-prebuilt`.
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
            Self::InvalidArchive { .. } | Self::ReadEntry { .. } => {
                "The download may be incomplete or damaged; download the zip again."
            }
            Self::UnsafeEntry { .. } => {
                "This zip is not an Unreal Engine release from Epic; download it again from the \
                 official page."
            }
            Self::DestinationNotEmpty { .. } => {
                "Choose a new or empty folder; existing files are never overwritten."
            }
            Self::InvalidDestination { .. } => "Choose a folder inside your home or another disk.",
            Self::NotEnoughSpace { .. } => "Free some space or choose a folder on another disk.",
            Self::NotAnEngineArchive { .. } => {
                "Check that you chose the Linux Unreal Engine zip from Epic, not a project, \
                 plugin or another platform's release."
            }
            Self::UnexpectedVersions => "Update UnLauncher; Fab may have changed this list.",
            Self::UnexpectedBlobs => {
                "Download the Linux engine zip from https://www.unrealengine.com/linux in \
                 your browser and install it with Import."
            }
            Self::Download { .. } | Self::HttpStatus { .. } => {
                "Check your connection and try again; the download resumes where it stopped. \
                 You can also download the zip from https://www.unrealengine.com/linux and use \
                 Import."
            }
            Self::UntrustedDownloadUrl { .. } => {
                "UnLauncher only downloads engines from Epic's servers. Download the zip from \
                 https://www.unrealengine.com/linux in your browser and use Import."
            }
            Self::DestinationExists { .. } => {
                "Choose another folder or remove the existing file; files are never overwritten."
            }
            Self::SizeMismatch { .. } => {
                "The download is incomplete or damaged; try again to resume or restart it."
            }
            Self::Cancelled => return None,
            Self::Engine(error) => return error.hint(),
        };
        Some(hint.to_owned())
    }
}

/// A byte count for messages, in GB with one decimal (or bytes below 1 GB's tenth).
#[allow(clippy::cast_precision_loss)] // Display only.
fn format_size(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    if bytes < 100_000_000 {
        format!("{bytes} bytes")
    } else {
        format!("{:.1} GB", bytes as f64 / GB)
    }
}
