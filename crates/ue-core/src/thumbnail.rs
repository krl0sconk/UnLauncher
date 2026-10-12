// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a project's or template's picture lives, if it has one. Only
//! checks that the files exist; decoding is the interface's job.

use std::fs;
use std::path::{Path, PathBuf};

use crate::project::Project;

/// Folder inside a template with its pictures.
const TEMPLATE_MEDIA_DIR: &str = "Media";
/// Image extensions looked for in a template's `Media` folder.
const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// The editor's automatic capture, relative to the project folder.
const AUTO_SCREENSHOT: &str = "Saved/AutoScreenshot.png";

/// The picture of `project`, in this order:
///
/// 1. `<project folder>/<Name>.png`, the thumbnail set in Project Settings;
/// 2. `<project folder>/Saved/AutoScreenshot.png`, the editor's automatic capture.
///
/// Returns `None` if neither is a file. Does not read the files.
#[must_use]
pub fn project_thumbnail(project: &Project) -> Option<PathBuf> {
    let dir = project.path.parent()?;
    [
        dir.join(format!("{}.png", project.name)),
        dir.join(AUTO_SCREENSHOT),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// The picture of the template in `template_dir`: the image in its `Media`
/// folder named like the template (`id`), or else the first image there by
/// name. `None` if the folder has no image.
pub(crate) fn template_thumbnail(template_dir: &Path, id: &str) -> Option<PathBuf> {
    let media = template_dir.join(TEMPLATE_MEDIA_DIR);
    let mut images: Vec<PathBuf> = fs::read_dir(&media)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_image(path))
        .collect();
    images.sort();
    let named_like_template = images
        .iter()
        .position(|path| path.file_stem().is_some_and(|stem| stem == id));
    match named_like_template {
        Some(index) => Some(images.swap_remove(index)),
        None => images.into_iter().next(),
    }
}

fn is_image(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        IMAGE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}
