// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for finding the picture of a project or a template.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::{fake_engine, fake_template, write_file};
use tempfile::tempdir;
use ue_core::{Project, list_templates, project_thumbnail};

const UPROJECT: &str = "{\n\t\"FileVersion\": 3,\n\t\"EngineAssociation\": \"5.7\"\n}\n";

fn project_in(root: &Path, name: &str) -> Project {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.uproject"));
    fs::write(&path, UPROJECT).unwrap();
    Project::load(&path).unwrap()
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"png").unwrap();
}

#[test]
fn a_project_without_pictures_has_no_thumbnail() {
    let root = tempdir().unwrap();
    let project = project_in(root.path(), "KartRacer");
    assert_eq!(project_thumbnail(&project), None);
}

#[test]
fn the_project_png_is_the_thumbnail() {
    let root = tempdir().unwrap();
    let project = project_in(root.path(), "KartRacer");
    let png = root.path().join("KartRacer/KartRacer.png");
    touch(&png);
    assert_eq!(project_thumbnail(&project), Some(png));
}

#[test]
fn the_auto_screenshot_is_the_second_choice() {
    let root = tempdir().unwrap();
    let project = project_in(root.path(), "KartRacer");
    let screenshot = root.path().join("KartRacer/Saved/AutoScreenshot.png");
    touch(&screenshot);
    assert_eq!(project_thumbnail(&project), Some(screenshot.clone()));

    // The picture set in Project Settings wins over the automatic capture.
    let png = root.path().join("KartRacer/KartRacer.png");
    touch(&png);
    assert_eq!(project_thumbnail(&project), Some(png));
}

#[test]
fn a_folder_named_like_the_picture_is_not_a_thumbnail() {
    let root = tempdir().unwrap();
    let project = project_in(root.path(), "KartRacer");
    fs::create_dir_all(root.path().join("KartRacer/KartRacer.png")).unwrap();
    assert_eq!(project_thumbnail(&project), None);
}

#[test]
fn another_projects_picture_is_not_used() {
    let root = tempdir().unwrap();
    let project = project_in(root.path(), "KartRacer");
    touch(&root.path().join("KartRacer/Other.png"));
    assert_eq!(project_thumbnail(&project), None);
}

fn engine_with_template(root: &Path, id: &str) -> PathBuf {
    fake_engine(root, 5, 7, 4);
    fake_template(root, id, "Blank", None, false)
}

#[test]
fn a_template_without_media_has_no_thumbnail() {
    let root = tempdir().unwrap();
    engine_with_template(root.path(), "TP_BlankBP");
    let templates = list_templates(root.path()).unwrap();
    assert_eq!(templates[0].thumbnail, None);
}

#[test]
fn a_template_uses_the_picture_named_like_it() {
    let root = tempdir().unwrap();
    let dir = engine_with_template(root.path(), "TP_BlankBP");
    write_file(&dir, "Media/Aaa.png", "x");
    let wanted = write_file(&dir, "Media/TP_BlankBP.png", "x");
    let templates = list_templates(root.path()).unwrap();
    assert_eq!(templates[0].thumbnail, Some(wanted));
}

#[test]
fn a_template_falls_back_to_the_first_image_in_media() {
    let root = tempdir().unwrap();
    let dir = engine_with_template(root.path(), "TP_BlankBP");
    write_file(&dir, "Media/readme.txt", "x");
    write_file(&dir, "Media/b.jpg", "x");
    let first = write_file(&dir, "Media/a.png", "x");
    let templates = list_templates(root.path()).unwrap();
    assert_eq!(templates[0].thumbnail, Some(first));
}
