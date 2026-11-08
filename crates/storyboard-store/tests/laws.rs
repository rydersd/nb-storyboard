use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;

use proptest::prelude::*;
use serde_json::{Map, Value, json};
use storyboard_core::ops::{RestoreAction, RestoreTarget};
use storyboard_core::{AgentAuthor, Author, Format, Op, Pin};
use storyboard_store::{
    CommitError, cache_put, commit, create, history, import_source, open, prune_cache, undo,
    version_of,
};

#[path = "../../storyboard-core/tests/law_support.rs"]
mod law_support;

use law_support::{all_scene_ids, all_shot_ids, choose_existing, project_strategy};

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
    create(directory.path(), "Law project", 24, format()).expect("create project");
    directory
}

fn generated_project(document: &storyboard_core::Project) -> tempfile::TempDir {
    let directory = project();
    let bytes = format!(
        "{}\n",
        document
            .to_pretty_json()
            .expect("serialize generated project")
    );
    fs::write(directory.path().join("project.json"), bytes).expect("seed generated project");
    directory
}

fn add_scene(id: &str, title: &str) -> Op {
    Op::AddScene {
        id: Some(id.into()),
        title: title.into(),
        index: None,
    }
}

fn update_scene(id: &str, title: &str) -> Op {
    Op::UpdateScene {
        scene: id.into(),
        patch: Map::from_iter([("title".into(), json!(title))]),
    }
}

fn update_shot(id: &str, title: &str) -> Op {
    Op::UpdateShot {
        shot: id.into(),
        patch: Map::from_iter([("title".into(), json!(title))]),
    }
}

fn sticky(id: &str, author: Author) -> Op {
    Op::AddSticky {
        id: Some(id.into()),
        text: "Generated note".into(),
        kind: "note".into(),
        author,
        pin: Pin::None,
        created: "2026-11-07T00:00:00Z".into(),
    }
}

fn assert_undo_with_cancelled_intervening_commit_preserves_later_work(
    base_project: &storyboard_core::Project,
    steps: usize,
    alice_title: &str,
    bob_title: &str,
    carol_title: &str,
) {
    let directory = generated_project(base_project);
    let initial = open(directory.path()).expect("initial open").version;
    let scene = choose_existing(&all_scene_ids(base_project), 0);
    let shot = choose_existing(&all_shot_ids(base_project), 0);
    let alice = agent("alice");
    let bob = agent("bob");
    let carol = agent("carol");
    let first = commit(
        directory.path(),
        &initial,
        vec![update_scene(&scene, &format!("alice-{alice_title}"))],
        alice.clone(),
        None,
    )
    .expect("alice first edit");
    let before_bob = if steps == 2 {
        commit(
            directory.path(),
            &first.version,
            vec![update_scene(&scene, &format!("alice-second-{alice_title}"))],
            alice.clone(),
            None,
        )
        .expect("alice second edit")
    } else {
        first
    };
    let bob_value = format!("bob-{bob_title}");
    let bob_commit = commit(
        directory.path(),
        &before_bob.version,
        vec![update_shot(&shot, &bob_value)],
        bob.clone(),
        None,
    )
    .expect("bob shot edit");
    let carol_value = format!("carol-{carol_title}");
    let carol_commit = commit(
        directory.path(),
        &bob_commit.version,
        vec![update_scene(&scene, &carol_value)],
        carol,
        None,
    )
    .expect("carol scene edit");
    let bob_undo = undo(directory.path(), &carol_commit.version, bob, 1).expect("bob undo");

    let undo_result = undo(directory.path(), &bob_undo.version, alice, steps);
    let loaded = open(directory.path()).expect("open after undo attempt");
    let scene_title = loaded
        .project
        .scenes
        .iter()
        .find(|item| item.id == scene)
        .expect("carol's scene")
        .title
        .as_str();
    assert!(
        matches!(undo_result, Err(CommitError::Conflict { .. })) || scene_title == carol_value,
        "Alice's undo erased Carol's later scene edit after Bob undid his shot edit: {undo_result:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 18,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn law_no_lost_work_single_step_undo_preserves_intervening_overlap(
        generated in project_strategy(),
        alice_title in "[A-Za-z0-9]{1,12}",
        bob_title in "[A-Za-z0-9]{1,12}",
        carol_title in "[A-Za-z0-9]{1,12}",
    ) {
        assert_undo_with_cancelled_intervening_commit_preserves_later_work(
            &generated.project,
            1,
            &alice_title,
            &bob_title,
            &carol_title,
        );
    }

    #[test]
    fn law_no_lost_work_multi_step_undo_preserves_intervening_overlap(
        generated in project_strategy(),
        alice_title in "[A-Za-z0-9]{1,12}",
        bob_title in "[A-Za-z0-9]{1,12}",
        carol_title in "[A-Za-z0-9]{1,12}",
    ) {
        assert_undo_with_cancelled_intervening_commit_preserves_later_work(
            &generated.project,
            2,
            &alice_title,
            &bob_title,
            &carol_title,
        );
    }

    #[test]
    fn law_no_lost_work_checks_every_intervening_commit(
        generated in project_strategy(),
        stale_title in "[A-Za-z0-9]{1,12}",
        landed_title in "[A-Za-z0-9]{1,12}",
        intervening_count in 2usize..=4,
        overlap_selector in any::<usize>(),
    ) {
        let directory = generated_project(&generated.project);
        let initial = open(directory.path()).expect("initial open").version;
        let scene = choose_existing(&all_scene_ids(&generated.project), generated.token);
        let shot = choose_existing(&all_shot_ids(&generated.project), generated.token);
        let stale_base = initial.clone();
        let overlap_at = overlap_selector % intervening_count;
        let landed_value = format!("landed-{landed_title}");
        let mut current = initial;
        for index in 0..intervening_count {
            let op = if index == overlap_at {
                update_scene(&scene, &landed_value)
            } else {
                update_shot(&shot, &format!("unrelated-{index}-{landed_title}"))
            };
            current = commit(
                directory.path(),
                &current,
                vec![op],
                Author::Human,
                None,
            ).expect("intervening commit").version;
        }

        let stale = commit(
            directory.path(),
            &stale_base,
            vec![update_scene(&scene, &format!("stale-{stale_title}"))],
            Author::Human,
            None,
        );
        let loaded = open(directory.path()).expect("open after stale attempt");
        let scene_a = loaded.project.scenes.iter()
            .find(|item| item.id == scene)
            .expect("overlapped scene");
        prop_assert!(
            matches!(stale, Err(CommitError::Conflict { .. })),
            "stale write did not conflict: {stale:?}"
        );
        prop_assert_eq!(&scene_a.title, &landed_value);
    }

    #[test]
    fn law_no_lost_work(
        generated in project_strategy(),
        first in "[A-Za-z0-9]{1,12}",
        second in "[A-Za-z0-9]{1,12}",
        overlaps in any::<bool>(),
        bob_adds in any::<bool>(),
        alice_adds in any::<bool>(),
    ) {
        let directory = generated_project(&generated.project);
        let initial = open(directory.path()).expect("initial open").version;
        let scene = choose_existing(&all_scene_ids(&generated.project), generated.token);
        let shot = choose_existing(&all_shot_ids(&generated.project), generated.token);
        let original_shot_title = generated.project.scenes.iter()
            .flat_map(|item| &item.shots)
            .find(|item| item.id == shot)
            .expect("shot")
            .title.clone();
        let first_title = format!("first-{first}");
        let second_title = format!("second-{second}");
        let landed = commit(directory.path(), &initial, vec![update_scene(&scene, &first_title)], Author::Human, None).expect("first edit");
        let stale_op = if overlaps {
            update_scene(&scene, &second_title)
        } else {
            update_shot(&shot, &second_title)
        };
        let stale = commit(directory.path(), &initial, vec![stale_op], Author::Human, None);
        let loaded = open(directory.path()).expect("open after stale write");
        let scene_a = loaded.project.scenes.iter().find(|item| item.id == scene).expect("scene");
        let shot_b = loaded.project.scenes.iter().flat_map(|item| &item.shots).find(|item| item.id == shot).expect("shot");
        prop_assert_eq!(&scene_a.title, &first_title, "the first landed edit was overwritten");
        if overlaps {
            let conflicted = matches!(stale, Err(CommitError::Conflict { .. }));
            prop_assert!(conflicted);
            prop_assert_eq!(&shot_b.title, &original_shot_title);
            prop_assert_eq!(loaded.version, landed.version);
        } else {
            let rebased = stale.expect("disjoint stale edit rebases");
            prop_assert!(rebased.rebased);
            prop_assert_eq!(&shot_b.title, &second_title);
        }

        let directory = generated_project(&generated.project);
        let mut order_base = open(directory.path()).expect("order base").version;
        if generated.project.scenes.len() == 1 {
            order_base = commit(
                directory.path(),
                &order_base,
                vec![add_scene(
                    &format!("scene-{:016x}-order-seed", generated.token),
                    "Order seed",
                )],
                Author::Human,
                None,
            )
            .expect("seed a second scene")
            .version;
        }
        let ordered = open(directory.path()).expect("seeded order project");
        let first_scene = ordered.project.scenes[0].id.clone();
        let last_scene = ordered.project.scenes.last().expect("last scene").id.clone();
        let bob_id = format!("scene-{:016x}-bob-order", generated.token);
        let bob_order = if bob_adds {
            Op::AddScene {
                id: Some(bob_id),
                title: "Bob order".into(),
                index: Some(0),
            }
        } else {
            Op::MoveScene {
                scene: last_scene,
                index: 0,
            }
        };
        let alice_order = if alice_adds {
            Op::AddScene {
                id: Some(format!("scene-{:016x}-alice-order", generated.token)),
                title: "Alice order".into(),
                index: Some(0),
            }
        } else {
            Op::MoveScene {
                scene: first_scene,
                index: 0,
            }
        };
        commit(
            directory.path(),
            &order_base,
            vec![bob_order],
            agent("bob"),
            None,
        )
        .expect("Bob order edit");
        let stale_order = commit(
            directory.path(),
            &order_base,
            vec![alice_order],
            agent("alice"),
            None,
        );
        prop_assert!(
            matches!(stale_order, Err(CommitError::Conflict { .. })),
            "Alice's stale scene-order edit landed over Bob's order intent: {stale_order:?}"
        );

        let mut shot_base = open(directory.path()).expect("shot-order base").version;
        let shot_scene = open(directory.path())
            .expect("shot-order project")
            .project
            .scenes
            .into_iter()
            .find(|scene| !scene.shots.is_empty())
            .expect("generated scene with shots");
        if shot_scene.shots.len() == 1 {
            shot_base = commit(
                directory.path(),
                &shot_base,
                vec![Op::AddShot {
                    scene: shot_scene.id.clone(),
                    id: Some(format!("shot-{:016x}-order-seed", generated.token)),
                    title: "Order seed".into(),
                    index: None,
                }],
                Author::Human,
                None,
            )
            .expect("seed a second shot")
            .version;
        }
        let shot_scene = open(directory.path())
            .expect("seeded shot-order project")
            .project
            .scenes
            .into_iter()
            .find(|scene| scene.id == shot_scene.id)
            .expect("shot-order scene");
        let first_shot = shot_scene.shots[0].id.clone();
        let last_shot = shot_scene.shots.last().expect("last shot").id.clone();
        let bob_shot_order = if bob_adds {
            Op::AddShot {
                scene: shot_scene.id.clone(),
                id: Some(format!("shot-{:016x}-bob-order", generated.token)),
                title: "Bob order".into(),
                index: Some(0),
            }
        } else {
            Op::MoveShot {
                shot: last_shot,
                index: 0,
            }
        };
        let alice_shot_order = if alice_adds {
            Op::AddShot {
                scene: shot_scene.id,
                id: Some(format!("shot-{:016x}-alice-order", generated.token)),
                title: "Alice order".into(),
                index: Some(0),
            }
        } else {
            Op::MoveShot {
                shot: first_shot,
                index: 0,
            }
        };
        commit(
            directory.path(),
            &shot_base,
            vec![bob_shot_order],
            agent("bob"),
            None,
        )
        .expect("Bob shot-order edit");
        let stale_shot_order = commit(
            directory.path(),
            &shot_base,
            vec![alice_shot_order],
            agent("alice"),
            None,
        );
        prop_assert!(
            matches!(stale_shot_order, Err(CommitError::Conflict { .. })),
            "Alice's stale shot-order edit landed over Bob's order intent: {stale_shot_order:?}"
        );
    }

    #[test]
    fn law_unbroken_history(
        generated in project_strategy(),
        titles in prop::collection::vec("[A-Za-z0-9]{1,10}", 1..8),
        torn in "[A-Za-z0-9{:]{1,12}",
    ) {
        let directory = generated_project(&generated.project);
        let initial = open(directory.path()).expect("initial open").version;
        let mut current = initial.clone();
        for (index, title) in titles.iter().enumerate() {
            let record = commit(
                directory.path(),
                &current,
                vec![add_scene(&format!("scene-{index}"), title)],
                Author::Human,
                None,
            ).expect("generated commit");
            prop_assert_eq!(&record.parent, &current);
            current = record.version;
        }

        let log = directory.path().join("history/log.jsonl");
        OpenOptions::new().append(true).open(&log).expect("open log").write_all(torn.as_bytes()).expect("write torn tail");
        prop_assert!(open(directory.path()).is_ok(), "a torn log tail made the project unopenable");
        let final_record = commit(
            directory.path(),
            &current,
            vec![add_scene("scene-final", "Final")],
            Author::Human,
            None,
        ).expect("commit repairs torn tail");

        let mut records = history(directory.path(), usize::MAX).expect("history");
        records.reverse();
        let mut parent = initial;
        for record in &records {
            prop_assert_eq!(&record.parent, &parent);
            let archive = directory.path().join("history").join(format!("archive-{}.json", record.parent));
            let archived = fs::read(&archive).expect("parent archive");
            prop_assert_eq!(version_of(&archived), record.parent.clone());
            parent = record.version.clone();
        }
        let disk = fs::read(directory.path().join("project.json")).expect("project bytes");
        prop_assert_eq!(version_of(&disk), final_record.version);
        prop_assert_eq!(parent, version_of(&disk));
        prop_assert!(open(directory.path()).is_ok());
    }

    #[test]
    fn law_honest_authorship(generated in project_strategy(), other_agent in any::<bool>()) {
        let directory = generated_project(&generated.project);
        let initial = open(directory.path()).expect("initial open").version;

        let human = commit(
            directory.path(),
            &initial,
            vec![sticky("sticky-human", Author::Human)],
            Author::Human,
            None,
        ).expect("human authors human sticky");

        let committer = agent("committer");
        let dishonest_author = if other_agent { agent("someone-else") } else { Author::Human };
        let dishonest = commit(
            directory.path(),
            &human.version,
            vec![sticky("sticky-dishonest", dishonest_author)],
            committer.clone(),
            None,
        );
        prop_assert!(dishonest.is_err(), "an agent landed a sticky under another author");
        prop_assert!(!open(directory.path()).expect("open").project.stickies.iter().any(|item| item.id == "sticky-dishonest"));

        let dishonest_reply = commit(
            directory.path(),
            &human.version,
            vec![Op::ReplySticky {
                sticky: "sticky-human".into(),
                id: Some("sticky-dishonest-reply".into()),
                text: "Generated reply".into(),
                kind: "note".into(),
                author: if other_agent { agent("someone-else") } else { Author::Human },
                created: "2026-11-07T00:00:01Z".into(),
            }],
            committer.clone(),
            None,
        );
        prop_assert!(dishonest_reply.is_err(), "an agent landed a reply under another author");
        prop_assert!(!open(directory.path()).expect("open").project.stickies.iter().any(|item| item.id == "sticky-dishonest-reply"));

        let honest = commit(
            directory.path(),
            &human.version,
            vec![sticky("sticky-agent", committer.clone())],
            committer.clone(),
            None,
        ).expect("agent authors its own sticky");
        let internal_authorship_change = commit(
            directory.path(),
            &honest.version,
            vec![Op::RestoreChanges {
                changes: vec![storyboard_core::RestoreChange {
                    action: RestoreAction::SetField {
                        target: RestoreTarget::Sticky {
                            sticky: "sticky-agent".into(),
                        },
                        field: "author".into(),
                        value: Some(json!("human")),
                    },
                }],
            }],
            committer.clone(),
            None,
        );
        prop_assert!(internal_authorship_change.is_err(), "public commit accepted an internal author rewrite");
        let change_author = commit(
            directory.path(),
            &honest.version,
            vec![Op::UpdateSticky {
                sticky: "sticky-agent".into(),
                patch: Map::from_iter([("author".into(), json!("human"))]),
            }],
            committer.clone(),
            None,
        );
        prop_assert!(change_author.is_err(), "stored authorship was mutable");
        let stored = open(directory.path()).expect("open").project.stickies.into_iter().find(|item| item.id == "sticky-agent").expect("agent sticky");
        prop_assert_eq!(stored.author, committer);
    }

    #[test]
    #[cfg(unix)]
    fn law_containment(
        generated in project_strategy(),
        segment in "[A-Za-z0-9]{1,12}",
        payload in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        use std::os::unix::fs::symlink;

        let directory = generated_project(&generated.project);
        let input = tempfile::NamedTempFile::new().expect("input");
        fs::write(input.path(), &payload).expect("input bytes");
        let traversal = Path::new("sources").join("..").join(format!("{segment}.bin"));
        prop_assert!(import_source(directory.path(), input.path(), &traversal).is_err());
        let escaped = directory.path().join(format!("{segment}.bin"));
        prop_assert!(!escaped.exists());

        let outside = tempfile::tempdir().expect("outside cache");
        fs::create_dir(outside.path().join("content")).expect("outside content");
        let marker = outside.path().join("content/keep");
        fs::write(&marker, b"keep").expect("marker");
        fs::rename(directory.path().join(".cache"), directory.path().join(".cache-real")).expect("move cache");
        symlink(outside.path(), directory.path().join(".cache")).expect("cache symlink");

        prop_assert!(cache_put(directory.path(), &payload).is_err(), "cache_put followed a symlink outside the project");
        prop_assert!(prune_cache(directory.path(), 0).is_err(), "prune_cache followed a symlink outside the project");
        prop_assert_eq!(fs::read(&marker).expect("outside marker survived"), b"keep");
        prop_assert_eq!(fs::read_dir(outside.path().join("content")).expect("outside content").count(), 1);

        let outside_sources = tempfile::tempdir().expect("outside sources");
        let source_marker = outside_sources.path().join("keep");
        fs::write(&source_marker, b"keep").expect("source marker");
        symlink(outside_sources.path(), directory.path().join("sources/link")).expect("source symlink");
        let source_input = tempfile::NamedTempFile::new().expect("source input");
        fs::write(source_input.path(), &payload).expect("source input bytes");
        prop_assert!(
            import_source(directory.path(), source_input.path(), Path::new("sources/link/escaped.bin")).is_err(),
            "import_source followed a symlink outside sources"
        );
        prop_assert_eq!(fs::read(&source_marker).expect("source marker survived"), b"keep");
        prop_assert!(!outside_sources.path().join("escaped.bin").exists());
    }

    #[test]
    fn law_round_trip(
        generated in project_strategy(),
        key in "[a-z]{1,10}",
        value in "[A-Za-z0-9]{0,20}",
    ) {
        let directory = generated_project(&generated.project);
        let initial = open(directory.path()).expect("initial open").version;
        let scene = commit(directory.path(), &initial, vec![add_scene("scene-1", "One")], Author::Human, None).expect("scene");
        let shot = commit(
            directory.path(),
            &scene.version,
            vec![Op::AddShot {
                scene: "scene-1".into(),
                id: Some("shot-1".into()),
                title: "Shot".into(),
                index: None,
            }],
            Author::Human,
            None,
        ).expect("shot");

        let path = directory.path().join("project.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&path).expect("project bytes")).expect("project json");
        let name = format!("future_{key}");
        let future = json!({"value": value, "nested": [1, 2, 3]});
        document.as_object_mut().expect("project object").insert(name.clone(), future.clone());
        document["format"].as_object_mut().expect("format object").insert(name.clone(), future.clone());
        document["layout"].as_object_mut().expect("layout object").insert(name.clone(), future.clone());
        document["scenes"][0].as_object_mut().expect("scene object").insert(name.clone(), future.clone());
        document["scenes"][0]["shots"][0].as_object_mut().expect("shot object").insert(name.clone(), future.clone());
        document["scenes"][0]["shots"][0]["takes"][0].as_object_mut().expect("take object").insert(name.clone(), future.clone());
        document["scenes"][0]["shots"][0]["takes"][0]["panels"][0].as_object_mut().expect("panel object").insert(name.clone(), future.clone());
        let bytes = serde_json::to_vec_pretty(&document).expect("serialize unknown fields");
        fs::write(&path, &bytes).expect("write unknown fields");
        let hand_version = version_of(&bytes);
        let _ = shot;
        commit(
            directory.path(),
            &hand_version,
            vec![update_scene("scene-1", "Rewritten")],
            Author::Human,
            None,
        ).expect("rewrite through store");

        let rewritten: Value = serde_json::from_slice(&fs::read(&path).expect("rewritten bytes")).expect("rewritten json");
        prop_assert_eq!(&rewritten[&name], &future);
        prop_assert_eq!(&rewritten["format"][&name], &future);
        prop_assert_eq!(&rewritten["layout"][&name], &future);
        prop_assert_eq!(&rewritten["scenes"][0][&name], &future);
        prop_assert_eq!(&rewritten["scenes"][0]["shots"][0][&name], &future);
        prop_assert_eq!(&rewritten["scenes"][0]["shots"][0]["takes"][0][&name], &future);
        prop_assert_eq!(&rewritten["scenes"][0]["shots"][0]["takes"][0]["panels"][0][&name], &future);
    }
}

#[test]
fn law_validity_commit_rejects_invalid_result() {
    let directory = project();
    let initial = open(directory.path()).expect("initial open");
    let invalid = commit(
        directory.path(),
        &initial.version,
        vec![add_scene("bad/id", "Invalid")],
        Author::Human,
        None,
    );
    assert!(invalid.is_err(), "commit landed an invalid project");
    let loaded = open(directory.path()).expect("project remains valid");
    assert_eq!(loaded.version, initial.version);
    assert!(loaded.project.scenes.is_empty());
}

#[test]
fn law_no_lost_work_external_edit_has_real_footprints() {
    let directory = project();
    let initial = open(directory.path()).expect("initial open").version;
    let scene = commit(
        directory.path(),
        &initial,
        vec![add_scene("scene-a", "Original")],
        Author::Human,
        None,
    )
    .expect("seed scene");

    let path = directory.path().join("project.json");
    let mut document: Value =
        serde_json::from_slice(&fs::read(&path).expect("project bytes")).expect("project json");
    document["scenes"][0]["title"] = json!("Hand edit");
    let edited = serde_json::to_vec_pretty(&document).expect("serialize hand edit");
    fs::write(&path, &edited).expect("write hand edit");

    let stale = commit(
        directory.path(),
        &scene.version,
        vec![update_scene("scene-a", "Agent edit")],
        Author::Human,
        None,
    );
    assert!(matches!(stale, Err(CommitError::Conflict { .. })));
    let loaded = open(directory.path()).expect("open after conflict");
    assert_eq!(loaded.project.scenes[0].title, "Hand edit");
}

#[test]
fn law_containment_cache_put_creates_missing_cache() {
    let directory = tempfile::tempdir().expect("fresh clone root");
    let relative = cache_put(directory.path(), b"generated cache value")
        .expect("cache_put creates a missing .cache");
    assert!(relative.starts_with(".cache/content"));
    assert_eq!(
        fs::read(directory.path().join(relative)).expect("cached bytes"),
        b"generated cache value"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 18,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn law_store_sequences_commit_undo_and_concurrent(
        generated in project_strategy(),
        titles in prop::collection::vec("[A-Za-z0-9]{1,10}", 3..7),
        nested_depth in 1usize..4,
    ) {
        let directory = generated_project(&generated.project);
        let initial = open(directory.path()).expect("initial open").version;
        let scene = choose_existing(&all_scene_ids(&generated.project), generated.token);
        let shot = choose_existing(&all_shot_ids(&generated.project), generated.token);
        let mut current = initial;

        for (index, title) in titles.iter().enumerate() {
            let generated_title = format!("{title}-{index}");
            let mut op = if index % 2 == 0 {
                update_scene(&scene, &generated_title)
            } else {
                update_shot(&shot, &generated_title)
            };
            for _ in 0..nested_depth {
                op = Op::Batch { ops: vec![op] };
            }
            current = commit(directory.path(), &current, vec![op], Author::Human, None)
                .expect("generated commit")
                .version;
        }
        let before_undo = open(directory.path()).expect("before undo").project;
        current = undo(directory.path(), &current, Author::Human, 1)
            .expect("single-step undo")
            .version;
        prop_assert_ne!(open(directory.path()).expect("after single undo").project, before_undo);
        current = undo(directory.path(), &current, Author::Human, 2)
            .expect("multi-step undo")
            .version;
        prop_assert_eq!(open(directory.path()).expect("after multi undo").version, current.clone());

        let base = current;
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for op in [
            update_scene(&scene, "Thread A"),
            update_shot(&shot, "Thread B"),
        ] {
            let path = directory.path().to_path_buf();
            let base = base.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                commit(&path, &base, vec![op], Author::Human, None)
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().expect("commit thread").expect("disjoint concurrent commit");
        }
        let loaded = open(directory.path()).expect("after concurrent commits");
        prop_assert_eq!(loaded.project.scenes.iter().find(|item| item.id == scene).expect("scene").title.as_str(), "Thread A");
        prop_assert_eq!(loaded.project.scenes.iter().flat_map(|item| &item.shots).find(|item| item.id == shot).expect("shot").title.as_str(), "Thread B");
    }
}

#[test]
#[ignore = "subprocess helper"]
fn law_unbroken_history_crash_worker() {
    let Ok(dir) = std::env::var("NB_LAW_PROJECT") else {
        return;
    };
    let base = std::env::var("NB_LAW_BASE").expect("base");
    let id = std::env::var("NB_LAW_SCENE").expect("scene");
    commit(
        Path::new(&dir),
        &base,
        vec![add_scene(&id, "Crash")],
        agent("crash-worker"),
        None,
    )
    .expect("commit");
}

#[test]
fn law_unbroken_history_recovers_each_crash_point() {
    let executable = std::env::current_exe().expect("test executable");
    for point in ["after_archive", "after_tmp", "after_rename", "after_log"] {
        let directory = project();
        let base = open(directory.path()).expect("open").version;
        let status = Command::new(&executable)
            .arg("--exact")
            .arg("law_unbroken_history_crash_worker")
            .arg("--ignored")
            .env("NB_LAW_PROJECT", directory.path())
            .env("NB_LAW_BASE", &base)
            .env("NB_LAW_SCENE", format!("scene-{point}"))
            .env("NB_STORYBOARD_TEST_CRASH_AT", point)
            .status()
            .expect("crash worker");
        assert!(!status.success(), "worker did not crash at {point}");

        let reopened =
            open(directory.path()).unwrap_or_else(|error| panic!("reopen after {point}: {error}"));
        let crashed_scene = format!("scene-{point}");
        let landed = matches!(point, "after_rename" | "after_log");
        assert_eq!(
            reopened
                .project
                .scenes
                .iter()
                .any(|scene| scene.id == crashed_scene),
            landed,
            "unexpected landing state at {point}"
        );
        let current = reopened.version;
        commit(
            directory.path(),
            &current,
            vec![add_scene(&format!("followup-{point}"), "Followup")],
            Author::Human,
            None,
        )
        .expect("follow-up commit after recovery");
        let mut records = history(directory.path(), usize::MAX).expect("history");
        records.reverse();
        if landed {
            assert!(
                records.iter().any(|record| {
                    record.author == agent("crash-worker")
                        && record.ops.iter().any(|op| {
                            matches!(op, Op::AddScene { id: Some(id), .. } if id == &crashed_scene)
                        })
                }),
                "landed commit lost its original author at {point}"
            );
        }
        for pair in records.windows(2) {
            assert_eq!(pair[1].parent, pair[0].version, "broken history at {point}");
        }
    }
}
