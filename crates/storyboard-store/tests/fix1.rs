use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::{Map, Value, json};
use storyboard_core::{AgentAuthor, Author, Format, Op};
use storyboard_store::{
    CommitError, check_refs, commit, create, history, import_source, index, open, open_workspace,
    scan_sources, undo, watch,
};

fn format() -> Format {
    Format {
        width: 1920,
        height: 1080,
        extra: Map::new(),
    }
}

fn human() -> Author {
    Author::Human
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

fn commit_one(dir: &Path, base: &str, op: Op) -> storyboard_store::CommitRecord {
    commit(dir, base, vec![op], human(), None).expect("commit")
}

fn seed_shot(dir: &Path, shot: &str) -> storyboard_store::CommitRecord {
    let initial = open(dir).expect("open").version;
    let scene = commit_one(dir, &initial, add_scene("scene-1"));
    commit_one(dir, &scene.version, add_shot("scene-1", shot))
}

fn write_project(dir: &Path, project: &storyboard_core::Project) -> Vec<u8> {
    let bytes = format!("{}\n", project.to_pretty_json().expect("serialize")).into_bytes();
    fs::write(dir.join("project.json"), &bytes).expect("write project");
    bytes
}

fn assert_history_chain(dir: &Path) {
    let records = history(dir, usize::MAX).expect("history");
    let records: Vec<_> = records.into_iter().rev().collect();
    for pair in records.windows(2) {
        assert_eq!(pair[1].parent, pair[0].version, "broken history chain");
    }
    if let Some(last) = records.last() {
        assert_eq!(last.version, open(dir).expect("open").version);
    }
    let log = fs::read(dir.join("history/log.jsonl")).expect("log");
    assert!(log.ends_with(b"\n"));
    for line in log
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        serde_json::from_slice::<storyboard_store::CommitRecord>(line).expect("record parses");
    }
}

#[test]
fn history_readers_skip_an_unterminated_tail_without_modifying_it() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    commit_one(directory.path(), &initial, add_scene("scene-1"));
    let log = directory.path().join("history/log.jsonl");
    OpenOptions::new()
        .append(true)
        .open(&log)
        .expect("open log")
        .write_all(b"{\"in_flight\":")
        .expect("partial append");
    let before = fs::read(&log).expect("before");

    assert_eq!(
        open(directory.path())
            .expect("open reader")
            .project
            .scenes
            .len(),
        1
    );
    assert_eq!(
        history(directory.path(), 10).expect("history reader").len(),
        1
    );
    assert_eq!(fs::read(&log).expect("after readers"), before);
}

#[test]
#[ignore = "subprocess helper"]
fn process_commit_worker() {
    let Ok(dir) = std::env::var("NB_FIX1_PROJECT") else {
        return;
    };
    let worker: usize = std::env::var("NB_FIX1_WORKER")
        .expect("worker")
        .parse()
        .expect("worker number");
    for iteration in 0..6 {
        loop {
            let loaded = open(Path::new(&dir)).expect("worker open");
            let _ = history(Path::new(&dir), 16).expect("worker history");
            let result = commit(
                Path::new(&dir),
                &loaded.version,
                vec![update_shot(
                    &format!("shot-{worker}"),
                    &format!("worker {worker} iteration {iteration}"),
                )],
                agent(&format!("worker-{worker}")),
                Some("x".repeat(64 * 1024)),
            );
            match result {
                Ok(_) => break,
                Err(CommitError::Conflict { .. }) => continue,
                Err(error) => panic!("worker commit failed: {error:?}"),
            }
        }
    }
}

#[test]
fn commit_is_process_safe_while_readers_open_and_read_history() {
    let directory = project();
    let mut base = open(directory.path()).expect("open").version;
    let scene = commit_one(directory.path(), &base, add_scene("scene-1"));
    base = scene.version;
    for worker in 0..16 {
        base = commit_one(
            directory.path(),
            &base,
            add_shot("scene-1", &format!("shot-{worker}")),
        )
        .version;
    }

    let executable = std::env::current_exe().expect("test executable");
    let mut children = Vec::new();
    for worker in 0..16 {
        children.push(
            Command::new(&executable)
                .arg("--exact")
                .arg("process_commit_worker")
                .arg("--ignored")
                .env("NB_FIX1_PROJECT", directory.path())
                .env("NB_FIX1_WORKER", worker.to_string())
                .spawn()
                .expect("spawn worker"),
        );
    }
    for mut child in children {
        assert!(child.wait().expect("wait worker").success());
    }

    assert_history_chain(directory.path());
    assert_eq!(
        history(directory.path(), usize::MAX)
            .expect("history")
            .len(),
        113
    );
}

#[test]
fn history_external_edit_uses_operation_namespace_for_conflicts() {
    let directory = project();
    let seeded = seed_shot(directory.path(), "shot-1");
    let stale = seeded.version.clone();
    let mut edited = open(directory.path()).expect("open").project;
    edited.scenes[0].shots[0].title = "hand edited".into();
    let bytes = write_project(directory.path(), &edited);
    let hand_version = storyboard_store::version_of(&bytes);

    let unrelated = commit_one(directory.path(), &hand_version, add_scene("scene-2"));
    let conflict = commit(
        directory.path(),
        &stale,
        vec![update_shot("shot-1", "stale title")],
        agent("stale-writer"),
        None,
    );
    assert!(
        matches!(conflict, Err(CommitError::Conflict { .. })),
        "{conflict:?}"
    );

    let rebased = commit(
        directory.path(),
        &stale,
        vec![add_scene("scene-3")],
        agent("unrelated-writer"),
        None,
    )
    .expect("unrelated stale commit rebases");
    assert!(rebased.rebased);
    assert_eq!(rebased.parent, unrelated.version);
}

#[cfg(unix)]
#[test]
fn sources_import_refuses_symlinked_folder_and_dangling_link_and_collision() {
    use std::os::unix::fs::symlink;

    let directory = project();
    let outside = tempfile::tempdir().expect("outside");
    let input = tempfile::NamedTempFile::new().expect("input");
    fs::write(input.path(), b"source bytes").expect("source");

    symlink(outside.path(), directory.path().join("sources/escape")).expect("folder symlink");
    assert!(
        import_source(
            directory.path(),
            input.path(),
            Path::new("sources/escape/outside.txt"),
        )
        .is_err()
    );
    assert!(!outside.path().join("outside.txt").exists());

    symlink(
        outside.path().join("missing"),
        directory.path().join("sources/dangling"),
    )
    .expect("dangling symlink");
    assert!(
        import_source(
            directory.path(),
            input.path(),
            Path::new("sources/dangling/file.txt"),
        )
        .is_err()
    );

    fs::write(directory.path().join("sources/collision.txt"), b"original").expect("collision");
    assert!(
        import_source(
            directory.path(),
            input.path(),
            Path::new("sources/collision.txt"),
        )
        .is_err()
    );
    assert_eq!(
        fs::read(directory.path().join("sources/collision.txt")).expect("collision bytes"),
        b"original"
    );
}

#[cfg(unix)]
#[test]
fn sources_scan_never_follows_symlinks_outside_the_project() {
    use std::os::unix::fs::symlink;

    let directory = project();
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("secret.txt"), b"secret").expect("outside file");
    symlink(outside.path(), directory.path().join("sources/outside")).expect("symlink");
    fs::write(directory.path().join("sources/local.txt"), b"local").expect("local file");

    let entries = scan_sources(directory.path()).expect("scan");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["sources/local.txt"]
    );
}

#[test]
fn history_undo_conflicts_with_a_later_overlapping_edit() {
    let directory = project();
    let seeded = seed_shot(directory.path(), "shot-1");
    let mine = commit(
        directory.path(),
        &seeded.version,
        vec![update_shot("shot-1", "mine")],
        agent("alice"),
        None,
    )
    .expect("mine");
    let theirs = commit(
        directory.path(),
        &mine.version,
        vec![update_shot("shot-1", "theirs")],
        agent("bob"),
        None,
    )
    .expect("theirs");

    let result = undo(directory.path(), &theirs.version, agent("alice"), 1);
    assert!(
        matches!(result, Err(CommitError::Conflict { .. })),
        "{result:?}"
    );
    assert_eq!(
        open(directory.path()).expect("open").project.scenes[0].shots[0].title,
        "theirs"
    );
}

#[test]
fn history_undo_skips_undo_records_and_records_undone_version() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let first = commit_one(directory.path(), &initial, add_scene("scene-1"));
    let second = commit_one(directory.path(), &first.version, add_scene("scene-2"));
    let first_undo = undo(directory.path(), &second.version, human(), 1).expect("first undo");
    let line: Value = serde_json::from_slice(
        fs::read(directory.path().join("history/log.jsonl"))
            .expect("log")
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty())
            .expect("last line"),
    )
    .expect("record json");
    assert_eq!(line["undoes"], second.version);

    let second_undo = undo(directory.path(), &first_undo.version, human(), 1).expect("second undo");
    assert!(
        open(directory.path())
            .expect("open")
            .project
            .scenes
            .is_empty()
    );
    let latest = history(directory.path(), 1)
        .expect("history")
        .pop()
        .expect("latest");
    assert_eq!(latest.version, second_undo.version);
    let latest_json = serde_json::to_value(latest).expect("latest json");
    assert_eq!(latest_json["undoes"], first.version);
}

fn receive_matching(
    receiver: &std::sync::mpsc::Receiver<storyboard_store::Change>,
    needle: &str,
) -> storyboard_store::Change {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let change = receiver.recv_timeout(remaining).expect("watch event");
        if format!("{change:?}").contains(needle) {
            return change;
        }
    }
}

#[test]
fn watch_reports_project_and_source_changes() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let (_watcher, receiver) = watch(&workspace).expect("watch");

    fs::write(project_dir.join("sources/new.txt"), b"source").expect("source write");
    let source = receive_matching(&receiver, "Source");
    assert!(format!("{source:?}").contains("sources/new.txt"));

    let mut document = open(&project_dir).expect("open").project;
    document.title = "Changed".into();
    write_project(&project_dir, &document);
    let project = receive_matching(&receiver, "Project");
    assert!(format!("{project:?}").contains("version"));
}

#[cfg(unix)]
#[test]
fn watch_matches_events_through_a_symlinked_workspace_path() {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().expect("parent");
    let real = parent.path().join("real");
    fs::create_dir(&real).expect("real workspace");
    let project_dir = real.join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let alias = parent.path().join("alias");
    symlink(&real, &alias).expect("workspace symlink");
    let workspace = open_workspace(&alias).expect("workspace");
    let (_watcher, receiver) = watch(&workspace).expect("watch");

    let mut document = open(&project_dir).expect("open").project;
    document.title = "Changed through real path".into();
    write_project(&project_dir, &document);
    let change = receive_matching(&receiver, "Project");
    assert!(format!("{change:?}").contains("one"));
}

fn seed_non_empty_panel(dir: &Path) -> storyboard_store::CommitRecord {
    let shot = seed_shot(dir, "shot-1");
    let loaded = open(dir).expect("open");
    let shot_data = &loaded.project.scenes[0].shots[0];
    let take = shot_data.takes[0].id.clone();
    let panel = shot_data.takes[0].panels[0].id.clone();
    commit(
        dir,
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
        human(),
        None,
    )
    .expect("add solid layer")
}

#[cfg(feature = "render")]
#[test]
fn workspace_thumbnail_is_real_relative_cached_and_content_keyed() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let panel_commit = seed_non_empty_panel(&project_dir);
    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let first = index(&workspace).expect("index").remove(0);
    let first_path = first.thumbnail.expect("thumbnail");
    assert!(first_path.is_relative());
    let first_bytes = fs::read(project_dir.join(&first_path)).expect("thumbnail bytes");
    assert_eq!(&first_bytes[..8], b"\x89PNG\r\n\x1a\n");

    let unrelated = commit_one(&project_dir, &panel_commit.version, add_scene("unrelated"));
    let second = index(&open_workspace(workspace_dir.path()).expect("workspace"))
        .expect("index")
        .remove(0);
    assert_eq!(second.thumbnail.as_ref(), Some(&first_path));

    let loaded = open(&project_dir).expect("open");
    let take = loaded.project.scenes[0].shots[0].takes[0].id.clone();
    let panel = loaded.project.scenes[0].shots[0].takes[0].panels[0]
        .id
        .clone();
    commit(
        &project_dir,
        &unrelated.version,
        vec![Op::UpdateLayer {
            shot: "shot-1".into(),
            take,
            panel,
            layer: "layer-1".into(),
            patch: Map::from_iter([("fill".into(), json!("#993366"))]),
        }],
        human(),
        None,
    )
    .expect("change panel content");
    let third = index(&open_workspace(workspace_dir.path()).expect("workspace"))
        .expect("index")
        .remove(0);
    assert_ne!(third.thumbnail.as_ref(), Some(&first_path));
    assert!(!project_dir.join(&first_path).exists());
}

#[cfg(feature = "render")]
#[test]
fn render_cache_key_tracks_source_size_and_modified_time() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let shot = seed_shot(&project_dir, "shot-1");
    let png = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 8, 215, 99, 248, 15, 4, 0, 9, 251, 3,
        253, 160, 146, 95, 202, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let source = project_dir.join("sources/image.png");
    fs::write(&source, png).expect("source");
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
            id: Some("image-layer".into()),
            layer_type: "image".into(),
            fields: Map::from_iter([("src".into(), json!("sources/image.png"))]),
            index: None,
        }],
        human(),
        None,
    )
    .expect("image layer");
    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let first = index(&workspace).expect("index")[0]
        .thumbnail
        .clone()
        .expect("thumbnail");

    let mut changed = png.to_vec();
    changed.extend_from_slice(b"different-size");
    fs::write(&source, changed).expect("changed source");
    let second = index(&workspace).expect("reindex")[0]
        .thumbnail
        .clone()
        .expect("thumbnail");
    assert_ne!(second, first);
    assert!(!project_dir.join(first).exists());
    assert!(project_dir.join(second).is_file());
}

#[cfg(not(feature = "render"))]
#[test]
fn workspace_thumbnail_is_none_without_render_feature() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    seed_non_empty_panel(&project_dir);
    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    assert_eq!(index(&workspace).expect("index")[0].thumbnail, None);
}

#[test]
fn cache_prunes_render_entries_not_used_by_the_current_project() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    seed_non_empty_panel(&project_dir);
    let render_cache = project_dir.join(".cache/render");
    fs::create_dir_all(&render_cache).expect("render cache");
    fs::write(render_cache.join("orphan.png"), b"old").expect("orphan");

    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let _ = index(&workspace).expect("index");
    assert!(!render_cache.join("orphan.png").exists());
}

#[test]
fn store_times_are_rfc3339_utc_strings() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let record = commit_one(directory.path(), &initial, add_scene("scene-1"));
    assert!(
        record.at.contains('T') && record.at.ends_with('Z'),
        "{}",
        record.at
    );

    fs::write(directory.path().join("sources/a.txt"), b"a").expect("source");
    let source = scan_sources(directory.path()).expect("scan").remove(0);
    assert!(
        source.modified.contains('T') && source.modified.ends_with('Z'),
        "{}",
        source.modified
    );

    let workspace_root = directory.path().parent().expect("workspace root");
    let workspace = open_workspace(workspace_root).expect("workspace");
    let entry = index(&workspace)
        .expect("index")
        .into_iter()
        .find(|entry| directory.path().ends_with(&entry.path))
        .expect("entry");
    assert!(
        entry.modified.contains('T') && entry.modified.ends_with('Z'),
        "{}",
        entry.modified
    );
}

#[test]
fn history_snapshot_is_interval_only_not_every_commit() {
    let directory = project();
    let mut base = open(directory.path()).expect("open").version;
    for number in 1..=49 {
        base = commit_one(
            directory.path(),
            &base,
            add_scene(&format!("scene-{number}")),
        )
        .version;
    }
    assert!(!directory.path().join("history/snapshot-1.json").exists());
    assert!(!directory.path().join("history/snapshot-49.json").exists());
    base = commit_one(directory.path(), &base, add_scene("scene-50")).version;
    assert!(directory.path().join("history/snapshot-50.json").is_file());
    assert_eq!(base, open(directory.path()).expect("open").version);
}

#[cfg(unix)]
#[test]
fn workspace_warm_index_lists_when_document_and_log_are_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("one");
    create(&project_dir, "One", 24, format()).expect("create");
    let initial = open(&project_dir).expect("open").version;
    commit_one(&project_dir, &initial, add_scene("scene-1"));
    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let expected = index(&workspace).expect("cold index");

    let project_file = project_dir.join("project.json");
    let log_file = project_dir.join("history/log.jsonl");
    fs::set_permissions(&project_file, fs::Permissions::from_mode(0o000)).expect("project mode");
    fs::set_permissions(&log_file, fs::Permissions::from_mode(0o000)).expect("log mode");
    let actual =
        index(&open_workspace(workspace_dir.path()).expect("warm workspace")).expect("warm index");
    fs::set_permissions(&project_file, fs::Permissions::from_mode(0o600)).expect("restore project");
    fs::set_permissions(&log_file, fs::Permissions::from_mode(0o600)).expect("restore log");
    assert_eq!(actual, expected);
}

#[test]
fn sources_check_reports_refs_outside_sources_as_issues() {
    let directory = project();
    seed_shot(directory.path(), "shot-1");
    let mut document = open(directory.path()).expect("open").project;
    document.scenes[0].shots[0]
        .refs
        .push("elsewhere/file.png".into());
    let issues = check_refs(directory.path(), &document).expect("issues, not fatal error");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "elsewhere/file.png");
}

#[test]
fn commit_rewrites_a_partly_written_archive() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let archive = directory
        .path()
        .join("history")
        .join(format!("archive-{initial}.json"));
    fs::write(&archive, b"{\"partial\":").expect("partial archive");
    commit_one(directory.path(), &initial, add_scene("scene-1"));
    let archived = fs::read(&archive).expect("archive");
    let archived_project: storyboard_core::Project =
        serde_json::from_slice(&archived).expect("complete archived project");
    assert!(archived_project.scenes.is_empty());
    assert_eq!(storyboard_store::version_of(&archived), initial);
}

#[test]
#[ignore = "subprocess helper"]
fn crash_commit_worker() {
    let Ok(dir) = std::env::var("NB_FIX1_PROJECT") else {
        return;
    };
    let base = std::env::var("NB_FIX1_BASE").expect("base");
    let id = std::env::var("NB_FIX1_SCENE").expect("scene");
    let result = commit(
        Path::new(&dir),
        &base,
        vec![add_scene(&id)],
        agent("crash-worker"),
        None,
    );
    match std::env::var("NB_STORYBOARD_TEST_FAIL_LOG").as_deref() {
        Ok("1") if matches!(result, Err(CommitError::LandedWithoutLog { .. })) => return,
        Ok("1") => std::process::exit(2),
        _ => {}
    }
    result.expect("commit");
}

#[test]
fn commit_survives_process_death_at_each_persistence_step() {
    let executable = std::env::current_exe().expect("test executable");
    for point in ["after_archive", "after_tmp", "after_rename", "after_log"] {
        let directory = project();
        let base = open(directory.path()).expect("open").version;
        let status = Command::new(&executable)
            .arg("--exact")
            .arg("crash_commit_worker")
            .arg("--ignored")
            .env("NB_FIX1_PROJECT", directory.path())
            .env("NB_FIX1_BASE", &base)
            .env("NB_FIX1_SCENE", format!("scene-{point}"))
            .env("NB_STORYBOARD_TEST_CRASH_AT", point)
            .status()
            .expect("crash worker");
        assert!(!status.success(), "worker did not crash at {point}");
        open(directory.path()).unwrap_or_else(|error| panic!("reopen after {point}: {error}"));
        let current = open(directory.path()).expect("current").version;
        let _ = commit(
            directory.path(),
            &current,
            vec![add_scene(&format!("followup-{point}"))],
            human(),
            None,
        );
        assert_history_chain(directory.path());
    }
}

#[test]
fn commit_reports_landed_without_log_and_recovery_keeps_known_author() {
    let executable = std::env::current_exe().expect("test executable");
    let directory = project();
    let base = open(directory.path()).expect("open").version;
    let status = Command::new(&executable)
        .arg("--exact")
        .arg("crash_commit_worker")
        .arg("--ignored")
        .env("NB_FIX1_PROJECT", directory.path())
        .env("NB_FIX1_BASE", &base)
        .env("NB_FIX1_SCENE", "landed")
        .env("NB_STORYBOARD_TEST_FAIL_LOG", "1")
        .status()
        .expect("failure worker");
    assert!(status.success(), "worker did not observe LandedWithoutLog");

    let current = open(directory.path()).expect("reopen").version;
    let _ = commit(
        directory.path(),
        &current,
        vec![add_scene("followup")],
        human(),
        None,
    );
    let records = history(directory.path(), usize::MAX).expect("history");
    let recovered = records
        .iter()
        .find(|record| record.version == current)
        .expect("recovered landed commit");
    assert_eq!(recovered.author, agent("crash-worker"));
    assert_history_chain(directory.path());
}

#[test]
fn recovered_commit_without_authorship_is_not_attributed_to_a_human() {
    let directory = project();
    let initial = open(directory.path()).expect("open").version;
    let first = commit_one(directory.path(), &initial, add_scene("scene-1"));
    let mut document = open(directory.path()).expect("open").project;
    document.title = "orphaned landed bytes".into();
    let orphaned = write_project(directory.path(), &document);
    let orphaned_version = storyboard_store::version_of(&orphaned);

    let _ = commit(
        directory.path(),
        &orphaned_version,
        vec![add_scene("followup")],
        human(),
        None,
    )
    .expect("recover and commit");
    let recovered = history(directory.path(), usize::MAX)
        .expect("history")
        .into_iter()
        .find(|record| record.version == orphaned_version)
        .expect("recovered record");
    assert_ne!(recovered.author, Author::Human);
    assert!(format!("{:?}", recovered.author).contains("recovered"));
    assert_eq!(recovered.parent, first.version);
}

#[test]
fn commit_error_implements_display() {
    fn display(error: &impl std::fmt::Display) -> String {
        error.to_string()
    }
    let directory = project();
    let version = open(directory.path()).expect("open").version;
    let error = commit(directory.path(), &version, Vec::new(), human(), None).unwrap_err();
    assert!(!display(&error).is_empty());
}

#[test]
fn workspace_index_lists_a_broken_project_instead_of_failing() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    create(&workspace_dir.path().join("good"), "Good", 24, format()).expect("create");
    let broken = workspace_dir.path().join("broken");
    fs::create_dir_all(&broken).expect("broken dir");
    fs::write(broken.join("project.json"), b"").expect("empty document");

    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let entries = index(&workspace).expect("one broken project must not hide the others");
    assert_eq!(entries.len(), 2, "{entries:#?}");
    let good = entries
        .iter()
        .find(|entry| entry.path == Path::new("good"))
        .expect("good");
    assert_eq!(good.title, "Good");
    assert_eq!(good.problem, None);
    let broken = entries
        .iter()
        .find(|entry| entry.path == Path::new("broken"))
        .expect("broken project is listed");
    assert!(
        broken
            .problem
            .as_deref()
            .is_some_and(|problem| problem.contains("project.json")),
        "{:?}",
        broken.problem
    );
}

#[test]
fn workspace_index_lists_a_project_without_a_cache_folder() {
    let workspace_dir = tempfile::tempdir().expect("workspace");
    let project_dir = workspace_dir.path().join("fresh-clone");
    create(&project_dir, "Fresh", 24, format()).expect("create");
    fs::remove_dir_all(project_dir.join(".cache")).expect("remove git-ignored cache");

    let workspace = open_workspace(workspace_dir.path()).expect("workspace");
    let entries = index(&workspace).expect("index");
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].title, "Fresh");
    assert_eq!(entries[0].problem, None);
}

#[test]
fn sources_scan_works_without_a_cache_folder() {
    let directory = project();
    fs::remove_dir_all(directory.path().join(".cache")).expect("remove git-ignored cache");
    fs::write(directory.path().join("sources/a.txt"), b"a").expect("source");
    let scanned = scan_sources(directory.path()).expect("scan in a fresh clone");
    assert_eq!(scanned.len(), 1);
}
