// SPDX-License-Identifier: GPL-3.0-or-later

//! Resumable HTTP download to a `.part` file.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use jobs::Reporter;
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::{Response, StatusCode};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::error::{Error, Result};
use crate::space::check_free_space;

/// Stage name reported by [`download_file`].
pub const DOWNLOADING_STAGE: &str = "Downloading";

/// Suffix of the partial file kept next to the destination.
const PART_SUFFIX: &str = ".part";

/// How the server answered the request.
enum Answer {
    /// `206` starting where the `.part` file ends: append.
    Resume,
    /// `200`: the whole file; the `.part` file is restarted.
    Whole,
    /// The range was refused or does not match: restart from zero.
    Restart,
}

/// Downloads `url` to `dest`, resuming a previous attempt if one was left.
///
/// Bytes go to `dest` plus `.part`. If that file exists, the rest is requested
/// with an HTTP `Range`; when the server ignores it the file restarts. Once
/// every byte arrived and the size matches `expected_size` (or the size the
/// server reported), the `.part` file is renamed to `dest`. Pause and cancel
/// are checked between chunks; on cancel the `.part` file is kept. The URL may
/// be presigned: it is never logged nor put in errors.
///
/// # Errors
///
/// - [`Error::DestinationExists`]: `dest` exists (never overwritten).
/// - [`Error::Download`], [`Error::HttpStatus`]: the request failed; the
///   `.part` file is kept.
/// - [`Error::SizeMismatch`]: a short file is kept to resume, a longer one is
///   removed.
/// - [`Error::NotEnoughSpace`]: the disk cannot hold the rest (checked before
///   writing).
/// - [`Error::WriteFile`], [`Error::Cancelled`].
pub async fn download_file(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    reporter: &Reporter,
) -> Result<PathBuf> {
    let file_name = dest.file_name().map_or_else(
        || dest.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    ensure_absent(dest).await?;
    let part = part_path(dest);
    reporter.stage(DOWNLOADING_STAGE);
    reporter.message(file_name.clone());

    let mut restarted = false;
    let total = loop {
        let mut offset = file_len(&part).await?;
        if expected_size.is_some_and(|expected| offset > expected) {
            truncate(&part).await?;
            offset = 0;
        }
        if expected_size == Some(offset) && offset > 0 {
            break offset;
        }
        reporter.checkpoint().await.map_err(|_| Error::Cancelled)?;

        let response = request(http, url, offset, &file_name).await?;
        let answer = classify(&response, offset);
        let (append, start) = match answer {
            Answer::Resume => (true, offset),
            Answer::Whole => (false, 0),
            Answer::Restart if !restarted => {
                tracing::warn!(file = %file_name, "server refused to resume, restarting");
                truncate(&part).await?;
                restarted = true;
                continue;
            }
            Answer::Restart => {
                return Err(Error::HttpStatus {
                    file_name,
                    status: response.status().as_u16(),
                });
            }
        };
        let total = expected_size.or_else(|| reported_total(&response, start));
        // Engines are tens of GB: never fill the disk halfway.
        if let Some(total) = total {
            let folder = dest
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            check_free_space(folder, total.saturating_sub(start))?;
        }
        stream(response, &part, append, start, total, reporter, &file_name).await?;
        break total.unwrap_or(file_len(&part).await?);
    };

    let actual = file_len(&part).await?;
    if actual != total {
        if actual > total {
            remove(&part).await?;
        }
        return Err(Error::SizeMismatch {
            path: part,
            expected: total,
            actual,
        });
    }
    ensure_absent(dest).await?;
    fs::rename(&part, dest)
        .await
        .map_err(|source| Error::WriteFile {
            path: dest.to_owned(),
            source,
        })?;
    reporter.fraction(Some(1.0));
    tracing::info!(file = %file_name, bytes = actual, "download finished");
    Ok(dest.to_owned())
}

/// Sends the GET, with `Range` when resuming.
async fn request(
    http: &reqwest::Client,
    url: &str,
    offset: u64,
    file_name: &str,
) -> Result<Response> {
    let mut request = http.get(url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| download_error(file_name, e))?;
    let status = response.status();
    if status.is_success() || status == StatusCode::RANGE_NOT_SATISFIABLE {
        Ok(response)
    } else {
        Err(Error::HttpStatus {
            file_name: file_name.to_owned(),
            status: status.as_u16(),
        })
    }
}

fn classify(response: &Response, offset: u64) -> Answer {
    match response.status() {
        StatusCode::PARTIAL_CONTENT if offset > 0 => {
            match content_range(response).map(|(start, _)| start) {
                Some(start) if start == offset => Answer::Resume,
                _ => Answer::Restart,
            }
        }
        StatusCode::OK => Answer::Whole,
        _ => Answer::Restart,
    }
}

/// The full size the server announced, if it did.
fn reported_total(response: &Response, start: u64) -> Option<u64> {
    if response.status() == StatusCode::PARTIAL_CONTENT
        && let Some((_, Some(total))) = content_range(response)
    {
        return Some(total);
    }
    response.content_length().map(|len| start + len)
}

/// Parses `Content-Range: bytes <start>-<end>/<total|*>`.
fn content_range(response: &Response) -> Option<(u64, Option<u64>)> {
    let value = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// Writes the body to `part`, reporting progress and honouring pause/cancel.
async fn stream(
    mut response: Response,
    part: &Path,
    append: bool,
    mut written: u64,
    total: Option<u64>,
    reporter: &Reporter,
    file_name: &str,
) -> Result<()> {
    let write_error = |source| Error::WriteFile {
        path: part.to_owned(),
        source,
    };
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part)
        .await
        .map_err(write_error)?;
    let mut out = BufWriter::new(file);
    report(reporter, written, total);

    let outcome = loop {
        if reporter.checkpoint().await.is_err() {
            break Err(Error::Cancelled);
        }
        match response.chunk().await {
            Ok(Some(bytes)) => {
                if let Err(source) = out.write_all(&bytes).await {
                    break Err(write_error(source));
                }
                written += bytes.len() as u64;
                report(reporter, written, total);
            }
            Ok(None) => break Ok(()),
            Err(error) => break Err(download_error(file_name, error)),
        }
    };
    // Keep what arrived, whatever happened, so the next call can resume.
    let flushed = async {
        out.flush().await?;
        out.get_ref().sync_all().await
    }
    .await;
    outcome?;
    flushed.map_err(write_error)
}

#[allow(clippy::cast_precision_loss)] // Progress only.
fn report(reporter: &Reporter, written: u64, total: Option<u64>) {
    if let Some(total) = total.filter(|&t| t > 0) {
        reporter.fraction(Some(written as f32 / total as f32));
    }
}

/// A download error with the (possibly signed) URL removed.
fn download_error(file_name: &str, error: reqwest::Error) -> Error {
    Error::Download {
        file_name: file_name.to_owned(),
        source: error.without_url(),
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = OsString::from(dest.as_os_str());
    name.push(PART_SUFFIX);
    PathBuf::from(name)
}

async fn ensure_absent(dest: &Path) -> Result<()> {
    match fs::symlink_metadata(dest).await {
        Ok(_) => Err(Error::DestinationExists {
            path: dest.to_owned(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::ReadFile {
            path: dest.to_owned(),
            source,
        }),
    }
}

/// Size of `path`, or 0 if it does not exist.
async fn file_len(path: &Path) -> Result<u64> {
    match fs::metadata(path).await {
        Ok(meta) => Ok(meta.len()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(source) => Err(Error::ReadFile {
            path: path.to_owned(),
            source,
        }),
    }
}

async fn truncate(part: &Path) -> Result<()> {
    File::create(part)
        .await
        .map(drop)
        .map_err(|source| Error::WriteFile {
            path: part.to_owned(),
            source,
        })
}

async fn remove(part: &Path) -> Result<()> {
    fs::remove_file(part)
        .await
        .map_err(|source| Error::WriteFile {
            path: part.to_owned(),
            source,
        })
}
