// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for listing engine templates and creating projects from them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::{fake_engine, fake_template, files_under, read_fixture, write_file};
use tempfile::tempdir;
use ue_core::{
    Engine, EngineAssociation, EngineId, Error, Project, ProjectTemplate, create_project,
    list_templates,
};

const BLANK_DEFS: &str = "templates/blank_bp_TemplateDefs.ini";

fn defs_named(name: &str) -> String {
    format!(
        "[/Script/GameProjectGeneration.TemplateProjectDefs]\n\
         LocalizedDisplayNames=(Language=\"en\",Text=\"{name}\")\n"
    )
}

fn engine_at(root: &Path, id: &str) -> Engine {
    fake_engine(root, 5, 7, 4);
    Engine::load(EngineId::new(id).unwrap(), root).unwrap()
}

fn template(engine_root: &Path, id: &str) -> ProjectTemplate {
    list_templates(engine_root)
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

/// An engine with a `TP_BlankBP` template shaped like the real one, plus
/// folders and files that must not be copied.
fn engine_with_blank_bp(root: &Path) -> (Engine, ProjectTemplate) {
    let engine = engine_at(root, "UE_5.7");
    let dir = fake_template(
        root,
        "TP_BlankBP",
        "TP_BlankBP",
        Some(&read_fixture(BLANK_DEFS)),
        false,
    );
    write_file(
        &dir,
        "Config/DefaultEngine.ini",
        "[/Script/Engine.RendererSettings]\nr.AllowStaticLighting=False\n",
    );
    write_file(&dir, "Config/DefaultGame.ini", "\n");
    write_file(
        &dir,
        "Config/DefaultInput.ini",
        "[/Script/Engine.InputSettings]\nbEnableMouseSmoothing=True\n",
    );
    write_file(&dir, "Config/config.ini", "[Ignored]\n");
    write_file(&dir, "Content/Maps/Main.umap", "map");
    write_file(&dir, "Media/TP_BlankBP.png", "png");
    write_file(&dir, "TP_BlankBP.png", "png");
    for excluded in ["Binaries", "Intermediate", "Saved", "DerivedDataCache"] {
        write_file(&dir, &format!("{excluded}/junk.bin"), "junk");
    }
    // Only top-level build folders are excluded.
    write_file(&dir, "Content/Saved/Kept.uasset", "kept");
    (engine, template(root, "TP_BlankBP"))
}

/// Whether `parent` has anything other than the listed entries.
fn entries(parent: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn engine_without_templates_folder_has_no_templates() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    assert_eq!(list_templates(&root).unwrap(), []);
}

#[test]
fn lists_template_with_names_from_real_template_defs() {
    let dir = tempdir().unwrap();
    let (_, blank) = engine_with_blank_bp(dir.path());
    assert_eq!(blank.id, "TP_BlankBP");
    assert_eq!(blank.name, "Blank");
    assert_eq!(
        blank.description.as_deref(),
        Some("A clean empty project with no code.")
    );
    assert_eq!(blank.path, dir.path().join("Templates/TP_BlankBP"));
    assert_eq!(blank.uproject, blank.path.join("TP_BlankBP.uproject"));
    assert!(!blank.has_code);
}

#[test]
fn prefers_english_names_and_falls_back_to_folder_id() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fake_template(
        root,
        "TP_Localized",
        "TP_Localized",
        Some(
            "LocalizedDisplayNames=(Language=\"ja\", Text=\"ブランク\")\n\
             LocalizedDisplayNames=(Language=\"en\", Text=\"Localized\")\n\
             LocalizedDescriptions=(Language=\"ko\", Text=\"...\")\n",
        ),
        false,
    );
    fake_template(root, "TP_NoDefs", "TP_NoDefs", None, false);

    let localized = template(root, "TP_Localized");
    assert_eq!(localized.name, "Localized");
    assert_eq!(localized.description, None);
    let bare = template(root, "TP_NoDefs");
    assert_eq!(bare.name, "TP_NoDefs");
    assert_eq!(bare.description, None);
}

#[test]
fn lists_blueprint_templates_first_then_by_name() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fake_template(
        root,
        "TP_ThirdPerson",
        "TP_ThirdPerson",
        Some(&defs_named("Third Person")),
        true,
    );
    fake_template(
        root,
        "TP_ThirdPersonBP",
        "TP_ThirdPersonBP",
        Some(&defs_named("Third Person")),
        false,
    );
    fake_template(
        root,
        "TP_Blank",
        "TP_Blank",
        Some(&defs_named("Blank")),
        true,
    );
    fake_template(
        root,
        "TP_BlankBP",
        "TP_BlankBP",
        Some(&defs_named("Blank")),
        false,
    );
    fake_template(
        root,
        "TP_UEIntro_BP",
        "UEIntro",
        Some(&defs_named("Intro To Unreal")),
        false,
    );
    // Folders without a .uproject are not templates.
    fs::create_dir_all(root.join("Templates/Media")).unwrap();
    fs::create_dir_all(root.join("Templates/TemplateResources/High")).unwrap();
    fs::write(root.join("Templates/TemplateCategories.ini"), "").unwrap();

    let templates = list_templates(root).unwrap();
    let ids: Vec<_> = templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "TP_BlankBP",
            "TP_UEIntro_BP",
            "TP_ThirdPersonBP",
            "TP_Blank",
            "TP_ThirdPerson"
        ]
    );
    let codes: Vec<_> = templates.iter().map(|t| t.has_code).collect();
    assert_eq!(codes, [false, false, false, true, true]);
    assert_eq!(
        templates[1].uproject,
        root.join("Templates/TP_UEIntro_BP/UEIntro.uproject")
    );
}

#[test]
fn creates_project_from_blueprint_template() {
    let dir = tempdir().unwrap();
    let (engine, blank) = engine_with_blank_bp(&dir.path().join("engine"));
    let parent = dir.path().join("Projects");

    let project = create_project(&blank, &engine, &parent, "MyGame").unwrap();
    let root = parent.join("MyGame");
    assert_eq!(project.path, root.join("MyGame.uproject"));
    assert_eq!(project.name, "MyGame");
    assert_eq!(
        project.association,
        EngineAssociation::Version { major: 5, minor: 7 }
    );

    assert_eq!(
        files_under(&root),
        [
            "Config/DefaultEngine.ini",
            "Config/DefaultGame.ini",
            "Config/DefaultInput.ini",
            "Content/Maps/Main.umap",
            "Content/Saved/Kept.uasset",
            "MyGame.uproject",
        ]
    );
    // Only the project was added to the parent folder: no temporary leftovers.
    assert_eq!(entries(&parent), ["MyGame"]);

    // Unknown .uproject fields survive, in Unreal's format.
    let text = fs::read_to_string(&project.path).unwrap();
    assert_eq!(
        text,
        support::TEMPLATE_UPROJECT.replace(
            "\"EngineAssociation\": \"\"",
            "\"EngineAssociation\": \"5.7\""
        )
    );
    // The template itself is untouched.
    assert!(blank.uproject.is_file());
    assert_eq!(
        fs::read_to_string(&blank.uproject).unwrap(),
        support::TEMPLATE_UPROJECT
    );
}

#[test]
fn writes_engine_id_for_non_release_engines() {
    let dir = tempdir().unwrap();
    let engine_root = dir.path().join("engine");
    engine_with_blank_bp(&engine_root);
    let engine = Engine::load(EngineId::new("{GUID-1}").unwrap(), &engine_root).unwrap();
    let blank = template(&engine_root, "TP_BlankBP");

    let project = create_project(&blank, &engine, dir.path(), "Game").unwrap();
    assert_eq!(
        project.association,
        EngineAssociation::Id(EngineId::new("{GUID-1}").unwrap())
    );
    assert_eq!(
        Project::load(&project.path).unwrap().association,
        project.association
    );
}

#[test]
fn leaves_no_backup_files() {
    let dir = tempdir().unwrap();
    let (engine, blank) = engine_with_blank_bp(&dir.path().join("engine"));
    let project = create_project(&blank, &engine, dir.path(), "Game").unwrap();
    let root = project.path.parent().unwrap();
    assert!(
        files_under(root)
            .iter()
            .all(|f| Path::new(f).extension().is_none_or(|ext| ext != "bak")),
        "{:?}",
        files_under(root)
    );
}

#[test]
fn copied_files_are_writable_even_if_the_template_is_read_only() {
    let dir = tempdir().unwrap();
    let (engine, blank) = engine_with_blank_bp(&dir.path().join("engine"));
    for file in [
        blank.uproject.clone(),
        blank.path.join("Config/DefaultInput.ini"),
        blank.path.join("Content/Maps/Main.umap"),
    ] {
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    }

    let project = create_project(&blank, &engine, dir.path(), "Game").unwrap();
    let root = project.path.parent().unwrap();
    for file in [
        "Game.uproject",
        "Config/DefaultInput.ini",
        "Content/Maps/Main.umap",
    ] {
        let mode = fs::metadata(root.join(file)).unwrap().permissions().mode();
        assert_ne!(mode & 0o200, 0, "{file} is not writable");
    }
    // The project folder is not left private like a temporary folder.
    let mode = fs::metadata(root).unwrap().permissions().mode();
    assert_ne!(mode & 0o077, 0);
}

#[test]
fn replaces_template_name_in_config_and_sets_new_project_id() {
    let dir = tempdir().unwrap();
    let engine_root = dir.path().join("engine");
    let engine = engine_at(&engine_root, "UE_5.7");
    let tpl = fake_template(
        &engine_root,
        "TP_ThirdPersonBP",
        "TP_ThirdPersonBP",
        Some(&defs_named("Third Person")),
        false,
    );
    write_file(
        &tpl,
        "Config/DefaultEngine.ini",
        "[URL]\r\nGameName=TP_ThirdPersonBP\r\n\r\n[/Script/EngineSettings.GameMapsSettings]\r\nGameDefaultMap=/Game/ThirdPerson/Lvl_ThirdPerson.Lvl_ThirdPerson\r\n",
    );
    write_file(
        &tpl,
        "Config/DefaultGame.ini",
        "[/Script/EngineSettings.GeneralProjectSettings]\nProjectID=C684876B46D5306ADA6286AA20FA1808\nProjectName=Third Person BP Game Template\n",
    );
    write_file(&tpl, "Content/Data.uasset", "TP_ThirdPersonBP");
    let third_person = template(&engine_root, "TP_ThirdPersonBP");

    let project = create_project(&third_person, &engine, dir.path(), "Shooter").unwrap();
    let root = project.path.parent().unwrap();

    assert_eq!(
        fs::read_to_string(root.join("Config/DefaultEngine.ini")).unwrap(),
        "[URL]\r\nGameName=Shooter\r\n\r\n[/Script/EngineSettings.GameMapsSettings]\r\nGameDefaultMap=/Game/ThirdPerson/Lvl_ThirdPerson.Lvl_ThirdPerson\r\n"
    );
    let game = fs::read_to_string(root.join("Config/DefaultGame.ini")).unwrap();
    let id_line = game.lines().find(|l| l.starts_with("ProjectID=")).unwrap();
    let id = id_line.trim_start_matches("ProjectID=");
    assert_eq!(id.len(), 32, "{id}");
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(id, "C684876B46D5306ADA6286AA20FA1808");
    assert!(game.contains("ProjectName=Third Person BP Game Template\n"));
    // Only Config files are rewritten.
    assert_eq!(
        fs::read_to_string(root.join("Content/Data.uasset")).unwrap(),
        "TP_ThirdPersonBP"
    );
}

#[test]
fn adds_project_id_to_empty_default_game_ini() {
    let dir = tempdir().unwrap();
    let (engine, blank) = engine_with_blank_bp(&dir.path().join("engine"));
    let project = create_project(&blank, &engine, dir.path(), "Game").unwrap();
    let game = fs::read_to_string(project.path.with_file_name("Config/DefaultGame.ini")).unwrap();
    assert!(
        game.starts_with("\n[/Script/EngineSettings.GeneralProjectSettings]\nProjectID="),
        "{game:?}"
    );
}

#[test]
fn renames_uproject_whose_name_differs_from_the_folder() {
    let dir = tempdir().unwrap();
    let engine_root = dir.path().join("engine");
    let engine = engine_at(&engine_root, "UE_5.7");
    fake_template(
        &engine_root,
        "TP_UEIntro_BP",
        "UEIntro",
        Some(&defs_named("Intro")),
        false,
    );
    let intro = template(&engine_root, "TP_UEIntro_BP");

    let project = create_project(&intro, &engine, dir.path(), "Learn").unwrap();
    assert_eq!(
        files_under(project.path.parent().unwrap()),
        ["Config/DefaultGame.ini", "Learn.uproject"]
    );
}

fn input_pack_manifest() -> &'static str {
    "{\n\t\"Version\": 1,\n\t\"Ident\": \"InputHigh\",\n\t\"AdditionalFiles\":\n\t{\n\t\t\"DestinationFilesFolder\": \"Input\",\n\t\t\"AdditionalFilesList\":\n\t\t[\n\t\t\t\"Templates/TemplateResources/High/Input/Content/*.*\"\n\t\t]\n\t}\n}\n"
}

#[test]
fn inserts_shared_content_packs() {
    let dir = tempdir().unwrap();
    let engine_root = dir.path().join("engine");
    let engine = engine_at(&engine_root, "UE_5.7");
    let pack = engine_root.join("Templates/TemplateResources/High/Input");
    write_file(&pack, "FeaturePack/manifest.json", input_pack_manifest());
    write_file(&pack, "Content/IMC_Default.uasset", "imc");
    write_file(&pack, "Content/Actions/IA_Jump.uasset", "jump");
    write_file(&pack, "Media/Preview.png", "png");
    fake_template(
        &engine_root,
        "TP_FirstPersonBP",
        "TP_FirstPersonBP",
        Some(
            "SharedContentPacks=(MountName=\"Input\",DetailLevels=(\"High\"))\n\
             ;SharedContentPacks=(MountName=\"Variant_Horror\",DetailLevels=(\"Standard\"))\n",
        ),
        false,
    );
    let first_person = template(&engine_root, "TP_FirstPersonBP");

    let project = create_project(&first_person, &engine, dir.path(), "Fps").unwrap();
    assert_eq!(
        files_under(project.path.parent().unwrap()),
        [
            "Config/DefaultGame.ini",
            "Content/Input/Actions/IA_Jump.uasset",
            "Content/Input/IMC_Default.uasset",
            "Fps.uproject",
        ]
    );
}

#[test]
fn missing_content_pack_fails_without_leaving_anything() {
    let dir = tempdir().unwrap();
    let engine_root = dir.path().join("engine");
    let engine = engine_at(&engine_root, "UE_5.7");
    fake_template(
        &engine_root,
        "TP_FirstPersonBP",
        "TP_FirstPersonBP",
        Some("SharedContentPacks=(MountName=\"Input\",DetailLevels=(\"High\"))\n"),
        false,
    );
    let first_person = template(&engine_root, "TP_FirstPersonBP");
    let parent = dir.path().join("Projects");
    fs::create_dir_all(&parent).unwrap();

    let err = create_project(&first_person, &engine, &parent, "Fps").unwrap_err();
    assert!(matches!(err, Error::ReadFile { .. }), "{err:?}");
    assert!(entries(&parent).is_empty(), "{:?}", entries(&parent));
}

#[test]
fn rejects_invalid_project_names() {
    let dir = tempdir().unwrap();
    let (engine, blank) = engine_with_blank_bp(&dir.path().join("engine"));
    let parent = dir.path().join("Projects");
    for name in [
        "",
        "1Game",
        "_Game",
        "My Game",
        "Mi-Juego",
        "Juégo",
        "../Escape",
        "ThisNameIsWayTooLong1",
    ] {
        let err = create_project(&blank, &engine, &parent, name).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidProjectName { name: rejected, .. } if rejected == name),
            "{name}: {err:?}"
        );
    }
    assert!(!parent.exists());
}

#[test]
fn refuses_existing_destination() {
    let dir = tempdir().unwrap();
    let (engine, blank) = engine_with_blank_bp(&dir.path().join("engine"));
    let parent = dir.path().join("Projects");
    let existing = write_file(&parent, "Game/notes.txt", "mine");
    fs::create_dir_all(parent.join("Empty")).unwrap();
    fs::write(parent.join("File"), "a file").unwrap();

    for name in ["Game", "Empty", "File"] {
        let err = create_project(&blank, &engine, &parent, name).unwrap_err();
        assert!(
            matches!(&err, Error::ProjectExists { path } if *path == parent.join(name)),
            "{name}: {err:?}"
        );
    }
    assert_eq!(fs::read_to_string(&existing).unwrap(), "mine");
    assert_eq!(files_under(&parent.join("Game")), ["notes.txt"]);
    assert_eq!(files_under(&parent.join("Empty")), Vec::<String>::new());
    assert_eq!(fs::read_to_string(parent.join("File")).unwrap(), "a file");
    assert_eq!(entries(&parent), ["Empty", "File", "Game"]);
}

#[test]
fn refuses_code_templates() {
    let dir = tempdir().unwrap();
    let engine_root = dir.path().join("engine");
    let engine = engine_at(&engine_root, "UE_5.7");
    fake_template(
        &engine_root,
        "TP_Blank",
        "TP_Blank",
        Some(&defs_named("Blank")),
        true,
    );
    let blank_cpp = template(&engine_root, "TP_Blank");
    assert!(blank_cpp.has_code);

    let parent: PathBuf = dir.path().join("Projects");
    let err = create_project(&blank_cpp, &engine, &parent, "Game").unwrap_err();
    assert!(
        matches!(&err, Error::CodeTemplateUnsupported { template } if template == "TP_Blank"),
        "{err:?}"
    );
    assert!(!parent.exists());
}
