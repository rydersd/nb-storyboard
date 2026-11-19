use std::fs;
use std::path::Path;

use serde_json::{Map, json};
use storyboard_core::{AgentAuthor, Author, Format, Op};
use storyboard_store::{
    CommitError, commit, create, history, index, open, open_workspace, redo, undo,
};

fn format() -> Format {
    Format {
        width: 1920,
        height: 1080,
        extra: Map::new(),
    }
}

fn agent(name: &str) -> Author {
    Author::Agent {
        agent: AgentAuthor {
            name: name.into(),
            extra: Map::new(),
        },
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

fn update_shot(shot: &str, title: &str) -> Op {
    Op::UpdateShot {
        shot: shot.into(),
        patch: Map::from_iter([("title".into(), json!(title))]),
    }
}

fn commit_one(dir: &Path, base: &str, op: Op, author: Author) -> storyboard_store::CommitRecord {
    commit(dir, base, vec![op], author, None).expect("commit")
}

fn seed_shot(dir: &Path) -> storyboard_store::CommitRecord {
    let initial = open(dir).expect("open").version;
    let scene = commit_one(dir, &initial, add_scene("scene-1"), Author::Human);
    commit_one(
        dir,
        &scene.version,
        add_shot("scene-1", "shot-1"),
        Author::Human,
    )
}

fn write_project(dir: &Path, project: &storyboard_core::Project) -> Vec<u8> {
    let bytes = format!("{}\n", project.to_pretty_json().expect("serialize")).into_bytes();
    fs::write(dir.join("project.json"), &bytes).expect("write project");
    bytes
}

#[test]
fn whole_project_footprint_overlaps_every_operation_namespace() {
    assert!(!storyboard_core::footprint::disjoint(
        &["project".into()],
        &["shot/shot-1/field/title".into()],
    ));
    assert!(!storyboard_core::footprint::disjoint(
        &["scene/scene-1".into()],
        &["project".into()],
    ));
}

#[test]
fn external_edit_after_conflict_keeps_a_current_baseline_and_conflicts() {
    let directory = project();
    let seeded = seed_shot(directory.path());
    let checkpoint = commit_one(
        directory.path(),
        &seeded.version,
        add_scene("checkpoint"),
        Author::Human,
    );

    let mut first_hand_edit = open(directory.path()).expect("open").project;
    first_hand_edit.scenes[0].shots[0].title = "first hand edit".into();
    let first_bytes = write_project(directory.path(), &first_hand_edit);
    let first_hand_version = storyboard_store::version_of(&first_bytes);

    let first_conflict = commit(
        directory.path(),
        &seeded.version,
        vec![update_shot("shot-1", "first stale write")],
        agent("stale-writer"),
        None,
    );
    assert!(
        matches!(first_conflict, Err(CommitError::Conflict { .. })),
        "{first_conflict:?}"
    );

    let mut second_hand_edit = open(directory.path()).expect("open").project;
    second_hand_edit.scenes[0].shots[0].title = "second hand edit".into();
    write_project(directory.path(), &second_hand_edit);

    let second_conflict = commit(
        directory.path(),
        &first_hand_version,
        vec![update_shot("shot-1", "second stale write")],
        agent("stale-writer"),
        None,
    );
    assert!(
        matches!(second_conflict, Err(CommitError::Conflict { .. })),
        "{second_conflict:?}; checkpoint={}",
        checkpoint.version
    );
    assert_eq!(
        open(directory.path()).expect("open").project.scenes[0].shots[0].title,
        "second hand edit"
    );
}

#[test]
fn multi_step_undo_conflicts_with_an_intervening_overlapping_commit() {
    let directory = project();
    let seeded = seed_shot(directory.path());
    let alice_title = commit_one(
        directory.path(),
        &seeded.version,
        update_shot("shot-1", "alice"),
        agent("alice"),
    );
    let bob_title = commit_one(
        directory.path(),
        &alice_title.version,
        update_shot("shot-1", "bob"),
        agent("bob"),
    );
    let alice_scene = commit_one(
        directory.path(),
        &bob_title.version,
        add_scene("alice-scene"),
        agent("alice"),
    );

    let result = undo(directory.path(), &alice_scene.version, agent("alice"), 2);
    assert!(
        matches!(result, Err(CommitError::Conflict { .. })),
        "{result:?}"
    );
    let project = open(directory.path()).expect("open").project;
    assert_eq!(project.scenes[0].shots[0].title, "bob");
    assert!(project.scenes.iter().any(|scene| scene.id == "alice-scene"));
}

#[test]
fn multi_step_undo_records_every_target_and_does_not_select_it_again() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let first = commit_one(
        directory.path(),
        &initial,
        add_scene("scene-1"),
        agent("alice"),
    );
    let second = commit_one(
        directory.path(),
        &first.version,
        add_scene("scene-2"),
        agent("alice"),
    );

    undo(directory.path(), &second.version, agent("alice"), 2).expect("undo both commits");
    let records = history(directory.path(), usize::MAX).expect("history");
    let undone: Vec<_> = records
        .iter()
        .filter_map(|record| record.undoes.as_deref())
        .collect();
    assert_eq!(undone.len(), 2, "one undo target per record: {records:#?}");
    assert!(undone.contains(&first.version.as_str()));
    assert!(undone.contains(&second.version.as_str()));

    let next = undo(
        directory.path(),
        &open(directory.path()).expect("open").version,
        agent("alice"),
        1,
    );
    assert!(
        matches!(next, Err(CommitError::Conflict { .. })),
        "already-undone commits must be skipped, got {next:?}"
    );
}

#[test]
fn stale_commit_conflicts_with_an_intervening_undo_record() {
    let directory = project();
    let seeded = seed_shot(directory.path());
    let bob = commit_one(
        directory.path(),
        &seeded.version,
        update_shot("shot-1", "bob"),
        agent("bob"),
    );
    let bob_undo = undo(directory.path(), &bob.version, agent("bob"), 1).expect("bob undo");

    let stale = commit(
        directory.path(),
        &bob.version,
        vec![update_shot("shot-1", "stale")],
        agent("alice"),
        None,
    );
    assert!(
        matches!(stale, Err(CommitError::Conflict { .. })),
        "{stale:?}"
    );
    assert_eq!(
        open(directory.path()).expect("open").project.scenes[0].shots[0].title,
        "shot-1",
        "the stale write must not overwrite undo {}",
        bob_undo.version
    );
}

#[test]
fn redo_reapplies_the_latest_undone_occurrence_and_records_its_target() {
    let directory = project();
    let seeded = seed_shot(directory.path());
    let changed = commit_one(
        directory.path(),
        &seeded.version,
        update_shot("shot-1", "Alice title"),
        agent("alice"),
    );
    let undone = undo(directory.path(), &changed.version, agent("alice"), 1).expect("undo");
    assert_eq!(undone.version, seeded.version);

    let redone = redo(directory.path(), &undone.version, agent("alice"), 1).expect("redo");
    assert_eq!(redone.version, changed.version);
    assert_eq!(redone.redoes.as_deref(), Some(changed.version.as_str()));
    assert_eq!(
        open(directory.path()).expect("open").project.scenes[0].shots[0].title,
        "Alice title"
    );
    let records = history(directory.path(), usize::MAX).expect("history");
    assert_eq!(records[0].redoes.as_deref(), Some(changed.version.as_str()));
}

#[test]
fn stale_redo_does_not_switch_to_a_different_undone_commit() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let scene_a = commit_one(
        directory.path(),
        &initial,
        add_scene("scene-a"),
        Author::Human,
    );
    let scene_b = commit_one(
        directory.path(),
        &scene_a.version,
        add_scene("scene-b"),
        Author::Human,
    );

    let undo_b = undo(directory.path(), &scene_b.version, Author::Human, 1).expect("undo B");
    let undo_a = undo(directory.path(), &undo_b.version, Author::Human, 1).expect("undo A");

    let stale_redo = redo(directory.path(), &undo_b.version, Author::Human, 1);
    match stale_redo {
        Err(CommitError::Conflict {
            current_version,
            intervening,
            ..
        }) => {
            assert_eq!(current_version, undo_a.version);
            assert_eq!(
                intervening
                    .iter()
                    .map(|record| &record.version)
                    .collect::<Vec<_>>(),
                vec![&undo_a.version],
                "the stale tab must be shown the history it missed"
            );
        }
        other => panic!("stale redo must conflict instead of redoing scene A: {other:?}"),
    }
    let document = open(directory.path()).expect("open").project;
    assert!(
        document.scenes.is_empty(),
        "stale redo must not redo scene A"
    );
}

#[test]
fn redo_is_per_author_and_a_new_commit_clears_only_that_authors_redo() {
    let directory = project();
    let seeded = seed_shot(directory.path());
    let alice = commit_one(
        directory.path(),
        &seeded.version,
        update_shot("shot-1", "Alice title"),
        agent("alice"),
    );
    let bob = commit_one(
        directory.path(),
        &alice.version,
        add_scene("bob-scene"),
        agent("bob"),
    );
    let bob_undo = undo(directory.path(), &bob.version, agent("bob"), 1).expect("bob undo");
    let alice_undo =
        undo(directory.path(), &bob_undo.version, agent("alice"), 1).expect("alice undo");
    let alice_new = commit_one(
        directory.path(),
        &alice_undo.version,
        update_shot("shot-1", "Alice replacement"),
        agent("alice"),
    );

    let no_alice_redo = redo(directory.path(), &alice_new.version, agent("alice"), 1);
    assert!(
        matches!(no_alice_redo, Err(CommitError::Conflict { .. })),
        "{no_alice_redo:?}"
    );
    redo(directory.path(), &alice_new.version, agent("bob"), 1)
        .expect("bob redo survives alice commit");
    assert!(
        open(directory.path())
            .expect("open")
            .project
            .scenes
            .iter()
            .any(|scene| scene.id == "bob-scene")
    );
}

#[test]
fn redo_conflicts_with_a_later_overlapping_commit() {
    let directory = project();
    let seeded = seed_shot(directory.path());
    let alice = commit_one(
        directory.path(),
        &seeded.version,
        update_shot("shot-1", "Alice title"),
        agent("alice"),
    );
    let undone = undo(directory.path(), &alice.version, agent("alice"), 1).expect("undo");
    let bob = commit_one(
        directory.path(),
        &undone.version,
        update_shot("shot-1", "Bob title"),
        agent("bob"),
    );
    let result = redo(directory.path(), &bob.version, agent("alice"), 1);
    assert!(
        matches!(result, Err(CommitError::Conflict { .. })),
        "{result:?}"
    );
    assert_eq!(
        open(directory.path()).expect("open").project.scenes[0].shots[0].title,
        "Bob title"
    );
}

#[cfg(feature = "render")]
#[test]
fn warm_workspace_index_regenerates_a_deleted_thumbnail() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let shot = seed_shot(&project_dir);
    let loaded = open(&project_dir).expect("open");
    let take = loaded.project.scenes[0].shots[0].takes[0].id.clone();
    let panel = loaded.project.scenes[0].shots[0].takes[0].panels[0]
        .id
        .clone();
    commit(
        &project_dir,
        &shot.version,
        vec![Op::AddLayer {
            shot: "shot-1".into(),
            take,
            panel,
            id: Some("layer-1".into()),
            layer_type: "solid".into(),
            fields: Map::from_iter([("fill".into(), json!("#336699"))]),
            index: None,
        }],
        Author::Human,
        None,
    )
    .expect("add layer");

    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let first = index(&workspace).expect("cold index")[0]
        .thumbnail
        .clone()
        .expect("thumbnail");
    fs::remove_file(project_dir.join(&first)).expect("delete thumbnail");

    let warm = index(&workspace).expect("warm index")[0]
        .thumbnail
        .clone()
        .expect("regenerated thumbnail");
    assert!(project_dir.join(&warm).is_file(), "missing {warm:?}");
}
