use std::{fs, path::Path, time::Duration};

use serde_json::json;
use storyboard_core::{Author, Format, Op};
use storyboard_store::{
    CommitError, commit, create, history, index, open, open_workspace, undo, watch,
};
use tempfile::TempDir;

fn format() -> Format {
    Format {
        width: 640,
        height: 360,
        extra: Default::default(),
    }
}
fn human() -> Author {
    Author::Human
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
    commit(dir, base, vec![op], human(), None).expect("commit")
}

#[test]
fn undo_honors_stale_base_version() {
    let project = TempDir::new().unwrap();
    let initial = create(project.path(), "Story", 24, format()).unwrap();
    let first = commit_one(project.path(), &initial.version, add_scene("scene-1"));
    let second = commit_one(project.path(), &first.version, add_scene("scene-2"));
    let error = undo(project.path(), &first.version, human(), 1).expect_err("stale undo conflicts");
    assert!(matches!(error, CommitError::Conflict { .. }));
    assert_eq!(open(project.path()).unwrap().version, second.version);
}

#[test]
fn external_hand_edit_history_author_is_human() {
    let project = TempDir::new().unwrap();
    let initial = create(project.path(), "Story", 24, format()).unwrap();
    let first = commit_one(project.path(), &initial.version, add_scene("scene-1"));
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(project.path().join("project.json")).unwrap()).unwrap();
    document["title"] = json!("Hand edit");
    fs::write(
        project.path().join("project.json"),
        format!("{}\n", serde_json::to_string_pretty(&document).unwrap()),
    )
    .unwrap();
    commit_one(project.path(), &first.version, add_scene("scene-2"));
    let external = history(project.path(), 10)
        .unwrap()
        .into_iter()
        .find(|record| record.external)
        .unwrap();
    assert_eq!(external.author, Author::Human);
}

#[test]
fn stale_rebase_onto_removed_target_is_conflict() {
    let project = TempDir::new().unwrap();
    let initial = create(project.path(), "Story", 24, format()).unwrap();
    let scene = commit_one(project.path(), &initial.version, add_scene("scene-1"));
    let shot = commit_one(
        project.path(),
        &scene.version,
        add_shot("scene-1", "shot-1"),
    );
    commit_one(
        project.path(),
        &shot.version,
        Op::RemoveShot {
            shot: "shot-1".into(),
        },
    );
    let error = commit(
        project.path(),
        &shot.version,
        vec![Op::UpdateShot {
            shot: "shot-1".into(),
            patch: [("title".into(), json!("Changed"))].into_iter().collect(),
        }],
        human(),
        None,
    )
    .expect_err("removed target conflicts");
    assert!(matches!(error, CommitError::Conflict { .. }), "{error:?}");
}

#[test]
fn stale_rebased_validation_failure_is_conflict() {
    let project = TempDir::new().unwrap();
    let initial = create(project.path(), "Story", 24, format()).unwrap();
    let scene = commit_one(project.path(), &initial.version, add_scene("scene-1"));
    commit_one(project.path(), &scene.version, add_scene("scene-2"));
    let error = commit(
        project.path(),
        &scene.version,
        vec![add_scene("scene-2")],
        human(),
        None,
    )
    .expect_err("rebase validation conflict");
    assert!(matches!(error, CommitError::Conflict { .. }), "{error:?}");
}

#[test]
fn history_limit_is_newest_first() {
    let project = TempDir::new().unwrap();
    let initial = create(project.path(), "Story", 24, format()).unwrap();
    let first = commit_one(project.path(), &initial.version, add_scene("scene-1"));
    let second = commit_one(project.path(), &first.version, add_scene("scene-2"));
    let latest = history(project.path(), 1).unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].version, second.version);
}

#[test]
fn corrupt_or_schema_mismatched_index_cache_is_a_miss() {
    let workspace = TempDir::new().unwrap();
    let project = workspace.path().join("story");
    create(&project, "Story", 24, format()).unwrap();
    let open_workspace_once = || index(&open_workspace(workspace.path()).unwrap()).unwrap();
    assert_eq!(open_workspace_once()[0].title, "Story");
    fs::write(project.join(".cache/index.json"), "not json").unwrap();
    assert_eq!(open_workspace_once()[0].title, "Story");
    fs::write(project.join(".cache/index.json"), "{}").unwrap();
    assert_eq!(open_workspace_once()[0].title, "Story");
}

#[test]
fn watch_ignores_lock_cache_tmp_and_failed_commits() {
    let workspace = TempDir::new().unwrap();
    let project = workspace.path().join("story");
    create(&project, "Story", 24, format()).unwrap();
    let workspace_state = open_workspace(workspace.path()).unwrap();
    let (_watcher, receiver) = watch(&workspace_state).unwrap();
    fs::write(project.join(".lock"), "noise").unwrap();
    fs::write(project.join(".cache/noise"), "noise").unwrap();
    fs::write(project.join("project.json.tmp"), "noise").unwrap();
    let base = open(&project).unwrap().version;
    assert!(
        commit(
            &project,
            &base,
            vec![Op::RemoveScene {
                scene: "missing".into()
            }],
            human(),
            None
        )
        .is_err()
    );
    assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
}
