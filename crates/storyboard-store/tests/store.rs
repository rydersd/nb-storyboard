use std::fs;
use std::io::Write;
use std::path::Path;

use serde_json::Map;
use storyboard_core::{Author, Format, Op};
use storyboard_store::{
    CommitError, check_refs, commit, create, history, import_source, index, open, open_workspace,
    scan_sources, undo,
};

fn format() -> Format {
    Format {
        width: 1920,
        height: 1080,
        extra: Map::new(),
    }
}

fn project() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary project");
    create(directory.path(), "Test project", 24, format()).expect("create project");
    directory
}

fn add_scene(id: &str) -> Op {
    Op::AddScene {
        id: Some(id.into()),
        title: id.into(),
        index: None,
    }
}

fn add_shot(scene: &str, id: &str) -> Op {
    Op::AddShot {
        scene: scene.into(),
        id: Some(id.into()),
        title: id.into(),
        index: None,
    }
}

fn commit_one(dir: &Path, base: &str, op: Op) -> storyboard_store::CommitRecord {
    commit(dir, base, vec![op], Author::Human, None).expect("commit")
}

#[test]
fn commit_writes_gitignore_log_note_summary_and_archive() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    let record = commit(
        directory.path(),
        &base,
        vec![add_scene("scene-1")],
        Author::Human,
        Some("start the story".into()),
    )
    .expect("commit");

    assert_eq!(record.note.as_deref(), Some("start the story"));
    assert!(!record.summary.is_empty());
    assert_eq!(
        fs::read_to_string(directory.path().join(".gitignore")).expect("gitignore"),
        "history/\n.cache/\n.lock\n"
    );
    assert!(directory.path().join("history/log.jsonl").is_file());
    assert!(
        fs::read_dir(directory.path().join("history"))
            .expect("history")
            .any(|entry| entry
                .expect("entry")
                .path()
                .extension()
                .is_some_and(|ext| ext == "json"))
    );
}

#[test]
fn commit_validates_before_stale_conflict() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    commit_one(directory.path(), &base, add_scene("scene-1"));

    let result = commit(
        directory.path(),
        &base,
        vec![Op::AddScene {
            id: Some("not valid".into()),
            title: "bad".into(),
            index: None,
        }],
        Author::Human,
        None,
    );
    assert!(matches!(result, Err(CommitError::Op(_))), "{result:?}");
}

#[test]
fn commit_replaces_a_stale_regular_temporary_file() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    fs::write(directory.path().join("project.json.tmp"), b"stale").expect("stale tmp");
    commit_one(directory.path(), &base, add_scene("scene-1"));
    assert!(!directory.path().join("project.json.tmp").exists());
}

#[test]
fn commit_conflict_has_reason_and_compact_intervening_summaries() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    let first = commit_one(directory.path(), &base, add_scene("scene-1"));
    let stale = first.version.clone();
    commit_one(
        directory.path(),
        &stale,
        Op::UpdateScene {
            scene: "scene-1".into(),
            patch: serde_json::json!({"title": "one"})
                .as_object()
                .unwrap()
                .clone(),
        },
    );
    let result = commit(
        directory.path(),
        &stale,
        vec![Op::UpdateScene {
            scene: "scene-1".into(),
            patch: serde_json::json!({"title": "two"})
                .as_object()
                .unwrap()
                .clone(),
        }],
        Author::Human,
        None,
    );
    match result {
        Err(CommitError::Conflict {
            reason,
            intervening,
            ..
        }) => {
            assert!(!reason.is_empty());
            assert_eq!(intervening.len(), 1);
            assert!(!intervening[0].summary.is_empty());
        }
        other => panic!("expected conflict, got {other:?}"),
    }
}

#[test]
fn history_repairs_a_torn_log_tail_under_the_next_write_lock() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    commit_one(directory.path(), &base, add_scene("scene-1"));
    let log = directory.path().join("history/log.jsonl");
    fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .expect("open log")
        .write_all(b"{\"torn\"")
        .expect("torn tail");
    let current = open(directory.path()).expect("open skips tail").version;
    assert_eq!(history(directory.path(), 10).expect("history").len(), 1);
    assert!(!fs::read(&log).expect("torn log").ends_with(b"\n"));
    commit_one(directory.path(), &current, add_scene("scene-2"));
    assert_eq!(history(directory.path(), 10).expect("history").len(), 2);
    assert!(fs::read(&log).expect("log").ends_with(b"\n"));
}

#[test]
fn history_logs_external_edit_and_later_disjoint_commit_rebases() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let scene = commit_one(directory.path(), &initial, add_scene("scene-1"));
    let mut edited = open(directory.path()).expect("open").project;
    edited.title = "Edited by hand".into();
    let bytes = format!("{}\n", edited.to_pretty_json().expect("serialize")).into_bytes();
    fs::write(directory.path().join("project.json"), &bytes).expect("hand edit");
    let hand_version = storyboard_store::version_of(&bytes);
    commit_one(
        directory.path(),
        &hand_version,
        add_shot("scene-1", "shot-1"),
    );
    let rebased = commit_one(directory.path(), &scene.version, add_scene("scene-2"));
    assert!(rebased.rebased);
    assert!(
        history(directory.path(), 10)
            .expect("history")
            .iter()
            .any(|record| record.external)
    );
}

#[test]
fn history_undo_commits_latest_author_inverse_through_commit() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    let landed = commit_one(directory.path(), &base, add_scene("scene-1"));
    let undone = undo(directory.path(), &landed.version, Author::Human, 1).expect("undo");
    assert!(undone.summary.contains("undo"));
    assert!(
        open(directory.path())
            .expect("open")
            .project
            .scenes
            .is_empty()
    );
}

#[test]
fn history_writes_an_interval_snapshot() {
    let directory = project();
    let mut base = open(directory.path()).expect("open").version;
    for number in 1..=50 {
        let record = commit_one(
            directory.path(),
            &base,
            add_scene(&format!("scene-{number}")),
        );
        base = record.version;
    }
    assert!(directory.path().join("history/snapshot-50.json").is_file());
}

#[test]
fn sources_scan_tracks_relative_image_dimensions_and_refs() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    let scene = commit_one(directory.path(), &base, add_scene("scene-1"));
    let shot = commit_one(
        directory.path(),
        &scene.version,
        add_shot("scene-1", "shot-1"),
    );
    let png = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 8, 215, 99, 248, 15, 4, 0, 9, 251, 3,
        253, 160, 146, 95, 202, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let path = directory.path().join("sources/stills/one.png");
    fs::create_dir_all(path.parent().expect("parent")).expect("sources dir");
    fs::write(&path, png).expect("image");
    let mut project = open(directory.path()).expect("open").project;
    project.scenes[0].shots[0]
        .refs
        .push("sources/stills/one.png".into());
    fs::write(
        directory.path().join("project.json"),
        format!("{}\n", project.to_pretty_json().expect("serialize")),
    )
    .expect("write refs");

    let sources = scan_sources(directory.path()).expect("scan");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].path, "sources/stills/one.png");
    assert_eq!(
        sources[0]
            .dimensions
            .as_ref()
            .map(|size| (size.width, size.height)),
        Some((1, 1))
    );
    assert_eq!(sources[0].uses.len(), 1);
    assert!(directory.path().join(".cache/sources.json").is_file());
    assert_eq!(shot.parent, scene.version);
}

#[test]
fn sources_import_stays_inside_sources_and_never_overwrites() {
    let directory = project();
    let input = tempfile::NamedTempFile::new().expect("input");
    fs::write(input.path(), b"reference").expect("input bytes");
    let imported = import_source(
        directory.path(),
        input.path(),
        Path::new("sources/references/a.txt"),
    )
    .expect("import");
    assert_eq!(imported, Path::new("sources/references/a.txt"));
    assert_eq!(
        fs::read(directory.path().join(&imported)).expect("imported bytes"),
        b"reference"
    );
    assert!(import_source(directory.path(), input.path(), &imported).is_err());
    assert!(import_source(directory.path(), input.path(), Path::new("../outside.txt")).is_err());
}

#[test]
fn sources_check_reports_gone_refs_by_name() {
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    let scene = commit_one(directory.path(), &base, add_scene("scene-1"));
    let shot = commit_one(
        directory.path(),
        &scene.version,
        add_shot("scene-1", "shot-1"),
    );
    let mut document = open(directory.path()).expect("open").project;
    document.scenes[0].shots[0]
        .refs
        .push("sources/gone.png".into());
    assert_eq!(shot.parent, scene.version);
    let missing = check_refs(directory.path(), &document).expect("check refs");
    assert_eq!(missing[0].path, "sources/gone.png");
}

#[test]
fn workspace_index_is_relative_and_warm_cache_survives_folder_rename() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let base = open(&project_dir).expect("open").version;
    let scene = commit_one(&project_dir, &base, add_scene("scene-1"));
    commit_one(&project_dir, &scene.version, add_shot("scene-1", "shot-1"));

    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let entries = index(&workspace).expect("index");
    assert_eq!(entries[0].path, std::path::PathBuf::from("one"));
    assert_eq!(entries[0].scene_count, 1);
    assert_eq!(entries[0].shot_count, 1);
    assert_eq!(entries[0].aspect, "16:9");
    assert!(project_dir.join(".cache/index.json").is_file());
    fs::rename(&project_dir, workspace_dir.path().join("renamed")).expect("rename");
    let renamed = open_workspace(workspace_dir.path()).expect("workspace");
    assert_eq!(
        index(&renamed).expect("warm index")[0].path,
        std::path::PathBuf::from("renamed")
    );
}
