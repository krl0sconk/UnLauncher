// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine project templates (`<engine>/Templates/TP_*`) and creating projects
//! from them.

mod config;
mod create;
mod defs;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::project::uproject_files;

pub use create::create_project;
use defs::TemplateDefs;

/// Folder of an engine that holds its project templates.
const TEMPLATES_DIR: &str = "Templates";
/// Category definitions, inside `Templates/`.
const CATEGORIES_FILE: &str = "TemplateCategories.ini";
/// Category of templates that declare none.
const OTHER_CATEGORY: &str = "Other";

/// A project template shipped with an engine, such as `TP_BlankBP`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTemplate {
    /// Folder name, e.g. `TP_BlankBP`.
    pub id: String,
    /// English display name from `TemplateDefs.ini`, or the id if it has none.
    pub name: String,
    /// English description from `TemplateDefs.ini`, if any.
    pub description: Option<String>,
    /// The template folder.
    pub path: PathBuf,
    /// The template's `.uproject` file. Its name can differ from the folder
    /// name (`TP_UEIntro_BP/UEIntro.uproject`).
    pub uproject: PathBuf,
    /// Whether the template has C++ code (a `Source/` folder).
    pub has_code: bool,
    /// English name of the template's category (`Games`, `Architecture`…),
    /// or `Other` if it has none. Several categories ship a template with the
    /// same name (`Blank`), so show both.
    pub category: String,
    /// A picture from the template's `Media` folder, if it has one.
    pub thumbnail: Option<PathBuf>,
}

/// Lists the project templates of the engine at `engine_root`: the folders of
/// `<engine_root>/Templates` that contain a `.uproject` file.
///
/// Names and descriptions come from the English entries of each template's
/// `Config/TemplateDefs.ini`; a template without one (or whose file cannot be
/// read) is listed under its folder name. Blueprint-only templates come first,
/// then templates with code; each group is sorted by name. Templates are
/// grouped by category in the order of `Templates/TemplateCategories.ini`
/// (Games first in Epic's engines). An engine without a `Templates` folder has
/// no templates.
///
/// # Errors
///
/// [`Error::ReadFile`] if the `Templates` folder exists but cannot be read.
pub fn list_templates(engine_root: &Path) -> Result<Vec<ProjectTemplate>> {
    let dir = engine_root.join(TEMPLATES_DIR);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::ReadFile { path: dir, source }),
    };

    let categories = fs::read_to_string(dir.join(CATEGORIES_FILE))
        .map(|text| defs::parse_categories(&text))
        .unwrap_or_default();
    let mut templates: Vec<ProjectTemplate> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| load_template(&path, &categories))
        .collect();
    let rank = |template: &ProjectTemplate| {
        categories
            .iter()
            .position(|c| c.name == template.category)
            .unwrap_or(categories.len())
    };
    templates.sort_by(|a, b| {
        (rank(a), a.has_code, a.name.to_lowercase(), &a.id).cmp(&(
            rank(b),
            b.has_code,
            b.name.to_lowercase(),
            &b.id,
        ))
    });
    Ok(templates)
}

/// Loads the template in `dir`, or `None` if it has no `.uproject`.
fn load_template(dir: &Path, categories: &[defs::Category]) -> Option<ProjectTemplate> {
    let mut uprojects = uproject_files(dir);
    uprojects.sort();
    let uproject = uprojects.into_iter().next()?;
    let id = dir.file_name()?.to_string_lossy().into_owned();
    let defs = TemplateDefs::load(dir).unwrap_or_else(|error| {
        tracing::warn!(template = %dir.display(), %error, "cannot read template definition");
        TemplateDefs::default()
    });
    let category = defs.categories.first().map_or_else(
        || OTHER_CATEGORY.to_owned(),
        |key| {
            categories
                .iter()
                .find(|c| &c.key == key)
                .map_or_else(|| key.clone(), |c| c.name.clone())
        },
    );
    Some(ProjectTemplate {
        category,
        name: defs.display_name.unwrap_or_else(|| id.clone()),
        description: defs.description,
        has_code: dir.join("Source").is_dir(),
        thumbnail: crate::thumbnail::template_thumbnail(dir, &id),
        id,
        path: dir.to_path_buf(),
        uproject,
    })
}
