use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use storyboard_core::ops::{RestoreAction, RestoreParent, RestoreTarget};
use storyboard_core::{
    Author, ClipSource, Op, Pin, Position, Project, RefTarget, Scene, ShotSource, apply,
    footprint::{disjoint, overlaps},
    validate,
};

const SMALL: &str = include_str!("fixtures/small.json");

fn fixture() -> Project {
    serde_json::from_str(SMALL).expect("the small fixture is a project")
}

fn patch(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn shot_source(shot: &str, take: Option<&str>) -> ClipSource {
    ClipSource::Shot(ShotSource {
        shot: shot.into(),
        take: take.map(str::to_owned),
        extra: Map::new(),
        source_extra: Map::new(),
    })
}

fn seed_unknown_fields(project: &mut Project) {
    project.extra.insert("project_future".into(), json!(true));
    project.scenes[0]
        .extra
        .insert("scene_future".into(), json!({"kept": true}));
    project.scenes[0].shots[0]
        .extra
        .insert("shot_future".into(), json!([1, 2, 3]));
    project.scenes[0].shots[0].takes[0]
        .extra
        .insert("take_future".into(), json!("yes"));
    project.scenes[0].shots[0].takes[0].panels[0]
        .extra
        .insert("panel_future".into(), json!(7));
    project.scenes[0].shots[0].takes[0].panels[0].layers[0]
        .extra
        .insert("layer_future".into(), json!(false));
    project
        .layout
        .positions
        .get_mut("shot:shot-1")
        .unwrap()
        .extra
        .insert("position_future".into(), json!("preserved"));
}

fn run_setup(project: &mut Project, op: Op) {
    apply(project, &op).unwrap_or_else(|error| panic!("setup {op:?} failed: {error:?}"));
}

fn no_setup(_: &mut Project) {}

fn setup_empty_scene(project: &mut Project) {
    project.scenes.insert(
        1,
        Scene {
            id: "scene-empty".into(),
            title: "Empty".into(),
            shots: Vec::new(),
            extra: Map::new(),
        },
    );
}

fn setup_extra_shot(project: &mut Project) {
    run_setup(
        project,
        Op::AddShot {
            scene: "scene-1".into(),
            id: Some("shot-extra".into()),
            title: "Extra".into(),
            index: Some(0),
        },
    );
}

fn setup_extra_panel(project: &mut Project) {
    run_setup(
        project,
        Op::AddPanel {
            shot: "shot-1".into(),
            take: "take-1".into(),
            id: Some("panel-extra".into()),
            time: Some(1.0),
            index: Some(1),
        },
    );
}

fn setup_extra_layer(project: &mut Project) {
    run_setup(
        project,
        Op::AddLayer {
            shot: "shot-1".into(),
            take: "take-1".into(),
            panel: "panel-1".into(),
            id: Some("layer-extra".into()),
            layer_type: "box".into(),
            fields: Map::new(),
            index: Some(1),
        },
    );
}

fn setup_extra_sticky(project: &mut Project) {
    run_setup(
        project,
        Op::AddSticky {
            id: Some("sticky-extra".into()),
            text: "Extra".into(),
            kind: "note".into(),
            author: Author::Human,
            pin: Pin::None,
            created: "2026-11-07T00:00:01Z".into(),
        },
    );
}

fn setup_refs(project: &mut Project) {
    project
        .scenes
        .iter_mut()
        .flat_map(|scene| &mut scene.shots)
        .find(|shot| shot.id == "shot-1")
        .unwrap()
        .refs = vec!["refs/first.png".into(), "refs/second.png".into()];
}

#[derive(Clone)]
struct Case {
    name: &'static str,
    setup: fn(&mut Project),
    op: Op,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "batch",
            setup: no_setup,
            op: Op::Batch {
                ops: vec![
                    Op::UpdateShot {
                        shot: "shot-1".into(),
                        patch: patch([("title", json!("Nested"))]),
                    },
                    Op::Batch {
                        ops: vec![Op::SetActiveCut { cut: None }],
                    },
                ],
            },
        },
        Case {
            name: "add_scene",
            setup: no_setup,
            op: Op::AddScene {
                id: Some("scene-new".into()),
                title: "New".into(),
                index: Some(1),
            },
        },
        Case {
            name: "remove_scene",
            setup: setup_empty_scene,
            op: Op::RemoveScene {
                scene: "scene-empty".into(),
            },
        },
        Case {
            name: "move_scene",
            setup: no_setup,
            op: Op::MoveScene {
                scene: "scene-2".into(),
                index: 0,
            },
        },
        Case {
            name: "update_scene",
            setup: no_setup,
            op: Op::UpdateScene {
                scene: "scene-1".into(),
                patch: patch([("title", json!("Changed"))]),
            },
        },
        Case {
            name: "add_shot",
            setup: no_setup,
            op: Op::AddShot {
                scene: "scene-1".into(),
                id: Some("shot-new".into()),
                title: "New".into(),
                index: Some(1),
            },
        },
        Case {
            name: "remove_shot",
            setup: setup_extra_shot,
            op: Op::RemoveShot {
                shot: "shot-extra".into(),
            },
        },
        Case {
            name: "move_shot",
            setup: no_setup,
            op: Op::MoveShot {
                shot: "shot-2".into(),
                index: 0,
            },
        },
        Case {
            name: "update_shot",
            setup: no_setup,
            op: Op::UpdateShot {
                shot: "shot-2".into(),
                patch: patch([("action", json!("Changed"))]),
            },
        },
        Case {
            name: "add_take",
            setup: no_setup,
            op: Op::AddTake {
                shot: "shot-2".into(),
                id: Some("take-new".into()),
                name: "Alternate".into(),
                index: Some(0),
            },
        },
        Case {
            name: "remove_take",
            setup: no_setup,
            op: Op::RemoveTake {
                shot: "shot-1".into(),
                take: "take-1".into(),
            },
        },
        Case {
            name: "move_take",
            setup: no_setup,
            op: Op::MoveTake {
                shot: "shot-1".into(),
                take: "take-2".into(),
                index: 0,
            },
        },
        Case {
            name: "update_take",
            setup: no_setup,
            op: Op::UpdateTake {
                shot: "shot-1".into(),
                take: "take-1".into(),
                patch: patch([("name", json!("Changed"))]),
            },
        },
        Case {
            name: "select_take",
            setup: no_setup,
            op: Op::SelectTake {
                shot: "shot-1".into(),
                take: "take-2".into(),
            },
        },
        Case {
            name: "duplicate_take",
            setup: no_setup,
            op: Op::DuplicateTake {
                shot: "shot-1".into(),
                take: "take-1".into(),
                id: Some("take-copy".into()),
                index: Some(1),
            },
        },
        Case {
            name: "add_panel",
            setup: no_setup,
            op: Op::AddPanel {
                shot: "shot-2".into(),
                take: "take-3".into(),
                id: Some("panel-new".into()),
                time: None,
                index: Some(0),
            },
        },
        Case {
            name: "remove_panel",
            setup: setup_extra_panel,
            op: Op::RemovePanel {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
            },
        },
        Case {
            name: "move_panel",
            setup: setup_extra_panel,
            op: Op::MovePanel {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-extra".into(),
                index: 0,
            },
        },
        Case {
            name: "update_panel",
            setup: no_setup,
            op: Op::UpdatePanel {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                patch: patch([("title", json!("Changed"))]),
            },
        },
        Case {
            name: "add_layer",
            setup: no_setup,
            op: Op::AddLayer {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                id: Some("layer-new".into()),
                layer_type: "box".into(),
                fields: Map::new(),
                index: Some(0),
            },
        },
        Case {
            name: "remove_layer",
            setup: setup_extra_layer,
            op: Op::RemoveLayer {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                layer: "layer-1".into(),
            },
        },
        Case {
            name: "move_layer",
            setup: setup_extra_layer,
            op: Op::MoveLayer {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                layer: "layer-extra".into(),
                index: 0,
            },
        },
        Case {
            name: "update_layer",
            setup: no_setup,
            op: Op::UpdateLayer {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                layer: "layer-1".into(),
                patch: patch([("label", json!("Changed"))]),
            },
        },
        Case {
            name: "add_sticky",
            setup: no_setup,
            op: Op::AddSticky {
                id: Some("sticky-new".into()),
                text: "Note".into(),
                kind: "note".into(),
                author: Author::Human,
                pin: Pin::None,
                created: "2026-11-07T00:00:01Z".into(),
            },
        },
        Case {
            name: "remove_sticky",
            setup: setup_extra_sticky,
            op: Op::RemoveSticky {
                sticky: "sticky-1".into(),
            },
        },
        Case {
            name: "move_sticky",
            setup: setup_extra_sticky,
            op: Op::MoveSticky {
                sticky: "sticky-extra".into(),
                index: 0,
            },
        },
        Case {
            name: "update_sticky",
            setup: no_setup,
            op: Op::UpdateSticky {
                sticky: "sticky-1".into(),
                patch: patch([("pin", json!("shot:shot-2"))]),
            },
        },
        Case {
            name: "add_clip",
            setup: no_setup,
            op: Op::AddClip {
                cut: "cut-alt".into(),
                id: Some("clip-new".into()),
                source: shot_source("shot-2", None),
                index: Some(0),
            },
        },
        Case {
            name: "remove_clip",
            setup: no_setup,
            op: Op::RemoveClip {
                cut: "cut-main".into(),
                clip: "clip-1".into(),
            },
        },
        Case {
            name: "reorder_clip",
            setup: no_setup,
            op: Op::ReorderClip {
                cut: "cut-main".into(),
                clip: "clip-2".into(),
                index: 0,
            },
        },
        Case {
            name: "update_clip",
            setup: no_setup,
            op: Op::UpdateClip {
                cut: "cut-main".into(),
                clip: "clip-1".into(),
                patch: patch([("in", json!(1)), ("out", json!(12)), ("slot", json!(14))]),
            },
        },
        Case {
            name: "add_cut",
            setup: no_setup,
            op: Op::AddCut {
                id: Some("cut-new".into()),
                name: "New".into(),
                index: Some(1),
            },
        },
        Case {
            name: "remove_cut",
            setup: no_setup,
            op: Op::RemoveCut {
                cut: "cut-main".into(),
            },
        },
        Case {
            name: "set_active_cut",
            setup: no_setup,
            op: Op::SetActiveCut {
                cut: Some("cut-alt".into()),
            },
        },
        Case {
            name: "move_shot_between_scenes",
            setup: no_setup,
            op: Op::MoveShotBetweenScenes {
                shot: "shot-2".into(),
                scene: "scene-2".into(),
                index: 1,
            },
        },
        Case {
            name: "add_ref",
            setup: no_setup,
            op: Op::AddRef {
                target: RefTarget::Shot {
                    shot: "shot-1".into(),
                },
                reference: "refs/door.png".into(),
                index: Some(0),
            },
        },
        Case {
            name: "remove_ref",
            setup: setup_refs,
            op: Op::RemoveRef {
                target: RefTarget::Shot {
                    shot: "shot-1".into(),
                },
                reference: "refs/first.png".into(),
            },
        },
        Case {
            name: "resolve_sticky",
            setup: no_setup,
            op: Op::ResolveSticky {
                sticky: "sticky-1".into(),
            },
        },
        Case {
            name: "reply_sticky",
            setup: no_setup,
            op: Op::ReplySticky {
                sticky: "sticky-1".into(),
                id: Some("sticky-reply".into()),
                text: "Reply".into(),
                kind: "note".into(),
                author: Author::Human,
                created: "2026-11-07T00:00:02Z".into(),
            },
        },
        Case {
            name: "move_node",
            setup: no_setup,
            op: Op::MoveNode {
                node: "shot:shot-1".into(),
                position: Some(Position {
                    x: 50.0,
                    y: 60.0,
                    extra: Map::new(),
                }),
            },
        },
    ]
}

fn named_case(name: &str) -> Case {
    cases()
        .into_iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("missing case {name}"))
}

fn assert_case_inverts(name: &str) {
    let case = named_case(name);
    let mut project = fixture();
    seed_unknown_fields(&mut project);
    (case.setup)(&mut project);
    let original = project.clone();
    let applied = apply(&mut project, &case.op)
        .unwrap_or_else(|error| panic!("{} failed: {error:?}", case.name));
    assert_ne!(project, original, "{} body must not be a no-op", case.name);
    for inverse in &applied.inverse {
        assert!(
            inverse.is_internal(),
            "inverse must be internal: {inverse:?}"
        );
        inverse
            .check_public()
            .expect_err("an inverse must be refused by the public operation layer");
        apply(&mut project, inverse).expect("internal inverse applies inside the core");
    }
    assert_eq!(project, original, "{} must invert exactly", case.name);
}

macro_rules! inverse_tests {
    ($($test:ident => $name:literal;)+) => {$ (
        #[test]
        fn $test() { assert_case_inverts($name); }
    )+};
}

inverse_tests! {
    op_batch_inverts => "batch";
    op_add_scene_inverts => "add_scene";
    op_remove_scene_inverts => "remove_scene";
    op_move_scene_inverts => "move_scene";
    op_update_scene_inverts => "update_scene";
    op_add_shot_inverts => "add_shot";
    op_remove_shot_inverts => "remove_shot";
    op_move_shot_inverts => "move_shot";
    op_update_shot_inverts => "update_shot";
    op_add_take_inverts => "add_take";
    op_remove_take_inverts => "remove_take";
    op_move_take_inverts => "move_take";
    op_update_take_inverts => "update_take";
    op_select_take_inverts => "select_take";
    op_duplicate_take_inverts => "duplicate_take";
    op_add_panel_inverts => "add_panel";
    op_remove_panel_inverts => "remove_panel";
    op_move_panel_inverts => "move_panel";
    op_update_panel_inverts => "update_panel";
    op_add_layer_inverts => "add_layer";
    op_remove_layer_inverts => "remove_layer";
    op_move_layer_inverts => "move_layer";
    op_update_layer_inverts => "update_layer";
    op_add_sticky_inverts => "add_sticky";
    op_remove_sticky_inverts => "remove_sticky";
    op_move_sticky_inverts => "move_sticky";
    op_update_sticky_inverts => "update_sticky";
    op_add_clip_inverts => "add_clip";
    op_remove_clip_inverts => "remove_clip";
    op_reorder_clip_inverts => "reorder_clip";
    op_update_clip_inverts => "update_clip";
    op_add_cut_inverts => "add_cut";
    op_remove_cut_inverts => "remove_cut";
    op_set_active_cut_inverts => "set_active_cut";
    op_move_shot_between_scenes_inverts => "move_shot_between_scenes";
    op_add_ref_inverts => "add_ref";
    op_remove_ref_inverts => "remove_ref";
    op_resolve_sticky_inverts => "resolve_sticky";
    op_reply_sticky_inverts => "reply_sticky";
    op_move_node_inverts => "move_node";
}

#[test]
fn every_public_op_changes_the_project_and_has_no_noop_body() {
    for case in cases() {
        assert_case_inverts(case.name);
        println!("| {} | detected by exact-change/inverse test |", case.name);
    }
}

#[test]
fn op_json_round_trips_every_op_and_every_inverse() {
    for case in cases() {
        let wire = serde_json::to_string(&case.op).expect("public op serializes");
        let decoded: Op = serde_json::from_str(&wire).expect("public op deserializes");
        assert_eq!(decoded, case.op, "public op round trip: {}", case.name);

        let mut project = fixture();
        (case.setup)(&mut project);
        let applied = apply(&mut project, &case.op).expect("case applies");
        for inverse in applied.inverse {
            let wire = serde_json::to_string(&inverse).expect("inverse serializes");
            let decoded: Op = serde_json::from_str(&wire).unwrap_or_else(|error| {
                panic!("inverse deserializes for {}: {error}; {wire}", case.name)
            });
            assert_eq!(decoded, inverse, "inverse round trip: {}", case.name);
        }
    }
}

#[test]
fn check_public_recurses_through_batches_and_sticky_authors_recurse() {
    let public = Op::Batch {
        ops: vec![Op::Batch {
            ops: vec![Op::AddSticky {
                id: Some("nested".into()),
                text: "Nested".into(),
                kind: "note".into(),
                author: Author::Human,
                pin: Pin::None,
                created: "2026-11-07T00:00:01Z".into(),
            }],
        }],
    };
    public
        .check_public()
        .expect("public nested batch is accepted");
    assert_eq!(public.sticky_authors(), [&Author::Human]);

    let mut project = fixture();
    let inverse = apply(
        &mut project,
        &Op::UpdateScene {
            scene: "scene-1".into(),
            patch: patch([("title", json!("Changed"))]),
        },
    )
    .unwrap()
    .inverse
    .pop()
    .unwrap();
    let nested = Op::Batch {
        ops: vec![Op::Batch { ops: vec![inverse] }],
    };
    assert!(nested.is_internal());
    assert!(nested.check_public().is_err());
}

#[test]
fn op_patch_rejects_unknown_field_with_field_and_options() {
    let mut project = fixture();
    let error = apply(
        &mut project,
        &Op::UpdateShot {
            shot: "shot-1".into(),
            patch: patch([("future", json!(true))]),
        },
    )
    .expect_err("unknown patches must be rejected");
    assert_eq!(error.field, "patch.future");
    assert!(error.valid.contains(&"title".to_owned()));
    assert!(error.issues.iter().all(|issue| !issue.field.is_empty()));
}

#[test]
fn op_errors_always_name_fields_and_valid_options_when_choices_exist() {
    let mut project = fixture();
    let errors = [
        apply(
            &mut project,
            &Op::MoveScene {
                scene: "missing".into(),
                index: 0,
            },
        )
        .unwrap_err(),
        apply(
            &mut project,
            &Op::SetActiveCut {
                cut: Some("missing".into()),
            },
        )
        .unwrap_err(),
        apply(
            &mut project,
            &Op::AddLayer {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                id: None,
                layer_type: "video".into(),
                fields: Map::new(),
                index: None,
            },
        )
        .unwrap_err(),
    ];
    for error in errors {
        assert!(!error.field.is_empty(), "{error:?}");
        assert!(!error.valid.is_empty(), "{error:?}");
        assert!(error.issues.iter().all(|issue| !issue.field.is_empty()));
    }
}

#[test]
fn op_new_shot_has_take_and_panel_and_reports_every_created_id() {
    let mut project = fixture();
    let applied = apply(
        &mut project,
        &Op::AddShot {
            scene: "scene-1".into(),
            id: Some("shot-new".into()),
            title: "New".into(),
            index: None,
        },
    )
    .unwrap();
    let shot = project.scenes[0].shots.last().unwrap();
    assert_eq!(shot.takes.len(), 1);
    assert_eq!(shot.takes[0].panels.len(), 1);
    assert_eq!(shot.selected_take, shot.takes[0].id);
    assert_eq!(
        applied.created_ids.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            shot.id.clone(),
            shot.takes[0].id.clone(),
            shot.takes[0].panels[0].id.clone(),
        ])
    );
}

#[test]
fn duplicate_take_reports_take_panel_and_layer_ids() {
    let mut project = fixture();
    let applied = apply(
        &mut project,
        &Op::DuplicateTake {
            shot: "shot-1".into(),
            take: "take-1".into(),
            id: Some("take-copy".into()),
            index: None,
        },
    )
    .unwrap();
    let copy = project.scenes[0].shots[0].takes.last().unwrap();
    let expected = BTreeSet::from([
        copy.id.clone(),
        copy.panels[0].id.clone(),
        copy.panels[0].layers[0].id.clone(),
    ]);
    assert_eq!(
        applied.created_ids.into_iter().collect::<BTreeSet<_>>(),
        expected
    );
}

#[test]
fn op_remove_shot_in_use_names_cuts() {
    let mut project = fixture();
    let error = apply(
        &mut project,
        &Op::RemoveShot {
            shot: "shot-1".into(),
        },
    )
    .unwrap_err();
    assert_eq!(error.field, "shot.shot-1");
    assert!(error.message.contains("Main"));
    assert_eq!(error.valid, ["cut-main"]);
}

#[test]
fn op_remove_selected_take_reselects_and_inverse_restores_selection() {
    assert_eq!(fixture().scenes[0].shots[0].selected_take, "take-1");
    assert_case_inverts("remove_take");
}

#[test]
fn op_remove_unpins_and_reports() {
    let mut project = fixture();
    setup_extra_panel(&mut project);
    let applied = apply(
        &mut project,
        &Op::RemovePanel {
            shot: "shot-1".into(),
            take: "take-1".into(),
            panel: "panel-1".into(),
        },
    )
    .unwrap();
    assert_eq!(applied.unpinned_stickies, ["sticky-1"]);
    assert_eq!(project.stickies[0].pin, Pin::None);
}

#[test]
fn footprint_remove_claims_subtree_pins_layout_references_order_and_freed_ids() {
    let mut project = fixture();
    setup_extra_panel(&mut project);
    let applied = apply(
        &mut project,
        &Op::RemovePanel {
            shot: "shot-1".into(),
            take: "take-1".into(),
            panel: "panel-1".into(),
        },
    )
    .unwrap();
    for expected in [
        "shot/shot-1/take/take-1/panel/panel-1",
        "shot/shot-1/take/take-1/order",
        "shot/shot-1/timing",
        "sticky/sticky-1",
        "ids/panel",
        "ids/layer",
    ] {
        assert!(
            applied.footprints.iter().any(|path| path == expected),
            "{expected}"
        );
    }
}

#[test]
fn footprint_groups_timing_trim_and_required_reads() {
    let mut project = fixture();
    let duration = apply(
        &mut project,
        &Op::UpdateShot {
            shot: "shot-2".into(),
            patch: patch([("duration", json!(2.0))]),
        },
    )
    .unwrap();
    let mut project = fixture();
    let panel_time = apply(
        &mut project,
        &Op::UpdatePanel {
            shot: "shot-2".into(),
            take: "take-3".into(),
            panel: "panel-3".into(),
            patch: patch([("time", json!(0.5))]),
        },
    )
    .unwrap();
    assert!(!disjoint(&duration.footprints, &panel_time.footprints));
    assert!(duration.footprints.contains(&"shot/shot-2/timing".into()));
    assert!(panel_time.footprints.contains(&"shot/shot-2/timing".into()));

    let mut project = fixture();
    let trim = apply(
        &mut project,
        &Op::UpdateClip {
            cut: "cut-main".into(),
            clip: "clip-1".into(),
            patch: patch([("in", json!(1)), ("slot", json!(2))]),
        },
    )
    .unwrap();
    assert!(
        trim.footprints
            .contains(&"cut/cut-main/clip/clip-1/trim".into())
    );

    let mut project = fixture();
    let add_clip = apply(
        &mut project,
        &Op::AddClip {
            cut: "cut-alt".into(),
            id: Some("clip-read".into()),
            source: shot_source("shot-1", Some("take-1")),
            index: None,
        },
    )
    .unwrap();
    assert!(add_clip.footprints.contains(&"shot/shot-1".into()));
    assert!(
        add_clip
            .footprints
            .contains(&"shot/shot-1/take/take-1".into())
    );
}

#[test]
fn footprints_are_hierarchical_on_segment_boundaries() {
    assert!(overlaps(
        "shot/H/take/T",
        "shot/H/take/T/panel/P/field/title"
    ));
    assert!(!overlaps("shot/H", "shot/H2"));
    assert!(!overlaps("shot/H/take/T", "shot/H/take/T2"));
}

#[test]
fn add_panel_defaults_follow_the_last_panel_and_name_duration_when_full() {
    let mut project = fixture();
    let applied = apply(
        &mut project,
        &Op::AddPanel {
            shot: "shot-2".into(),
            take: "take-3".into(),
            id: Some("panel-default".into()),
            time: None,
            index: None,
        },
    )
    .unwrap();
    let panel = project.scenes[0].shots[1].takes[0].panels.last().unwrap();
    assert_eq!(panel.time, 1.0 / 24.0);
    assert_eq!(applied.created_ids, ["panel-default"]);

    let mut empty = fixture();
    empty.scenes[0].shots[1].takes[0].panels.clear();
    apply(
        &mut empty,
        &Op::AddPanel {
            shot: "shot-2".into(),
            take: "take-3".into(),
            id: Some("panel-zero".into()),
            time: None,
            index: None,
        },
    )
    .unwrap();
    assert_eq!(empty.scenes[0].shots[1].takes[0].panels[0].time, 0.0);

    let mut full = fixture();
    full.scenes[0].shots[1].takes[0].panels[0].time = 23.0 / 24.0;
    let error = apply(
        &mut full,
        &Op::AddPanel {
            shot: "shot-2".into(),
            take: "take-3".into(),
            id: None,
            time: None,
            index: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.field, "shot.shot-2.duration");
    assert!(error.message.contains("duration"));
}

#[test]
fn add_layer_image_requires_src_and_text_requires_only_text() {
    let mut project = fixture();
    let missing = apply(
        &mut project,
        &Op::AddLayer {
            shot: "shot-2".into(),
            take: "take-3".into(),
            panel: "panel-3".into(),
            id: None,
            layer_type: "image".into(),
            fields: Map::new(),
            index: None,
        },
    )
    .unwrap_err();
    assert_eq!(missing.field, "fields.src");

    apply(
        &mut project,
        &Op::AddLayer {
            shot: "shot-2".into(),
            take: "take-3".into(),
            panel: "panel-3".into(),
            id: Some("image-layer".into()),
            layer_type: "image".into(),
            fields: patch([("src", json!("refs/image.png"))]),
            index: None,
        },
    )
    .unwrap();
    apply(
        &mut project,
        &Op::AddLayer {
            shot: "shot-2".into(),
            take: "take-3".into(),
            panel: "panel-3".into(),
            id: Some("text-layer".into()),
            layer_type: "text".into(),
            fields: patch([("text", json!("Hello"))]),
            index: None,
        },
    )
    .unwrap();
}

#[test]
fn op_move_shot_keeps_identity() {
    let mut project = fixture();
    apply(
        &mut project,
        &Op::MoveShotBetweenScenes {
            shot: "shot-2".into(),
            scene: "scene-2".into(),
            index: 1,
        },
    )
    .unwrap();
    let shot = project.scenes[1]
        .shots
        .iter()
        .find(|shot| shot.id == "shot-2")
        .unwrap();
    assert_eq!(shot.takes[0].id, "take-3");
}

#[test]
fn move_shot_between_scenes_inverse_is_one_move_by_id() {
    let mut project = fixture();
    let applied = apply(
        &mut project,
        &Op::MoveShotBetweenScenes {
            shot: "shot-2".into(),
            scene: "scene-2".into(),
            index: 1,
        },
    )
    .unwrap();
    let [Op::RestoreChanges { changes }] = applied.inverse.as_slice() else {
        panic!("move-shot inverse must be one internal restore op");
    };
    assert_eq!(
        changes.len(),
        1,
        "a move must not invert as delete plus insert"
    );
    assert!(matches!(
        &changes[0].action,
        RestoreAction::Move {
            target: RestoreTarget::Shot { shot },
            parent: RestoreParent::Shots { scene },
            index: 1,
        } if shot == "shot-2" && scene == "scene-1"
    ));
}

#[test]
fn batch_scene_lifecycle_around_cross_scene_moves_inverts_exactly() {
    let mut project = fixture();
    seed_unknown_fields(&mut project);
    let original = project.clone();
    let applied = apply(
        &mut project,
        &Op::Batch {
            ops: vec![
                Op::AddScene {
                    id: Some("scene-batch".into()),
                    title: "Batch".into(),
                    index: Some(1),
                },
                Op::MoveShotBetweenScenes {
                    shot: "shot-2".into(),
                    scene: "scene-batch".into(),
                    index: 0,
                },
            ],
        },
    )
    .unwrap();
    assert_eq!(applied.inverse.len(), 2);
    for inverse in &applied.inverse {
        apply(&mut project, inverse).unwrap();
    }
    assert_eq!(project, original);
}

#[test]
fn remove_ref_unknown_names_valid_refs() {
    let mut project = fixture();
    setup_refs(&mut project);
    let error = apply(
        &mut project,
        &Op::RemoveRef {
            target: RefTarget::Shot {
                shot: "shot-1".into(),
            },
            reference: "refs/missing.png".into(),
        },
    )
    .expect_err("an unknown ref must be rejected");
    assert_eq!(error.field, "reference");
    assert_eq!(error.valid, ["refs/first.png", "refs/second.png"]);
}

#[test]
fn move_node_uses_typed_layout_keys_and_preserves_unknown_fields() {
    let mut project = fixture();
    project
        .layout
        .positions
        .get_mut("shot:shot-1")
        .unwrap()
        .extra
        .insert("future".into(), json!(true));
    apply(
        &mut project,
        &Op::MoveNode {
            node: "shot:shot-1".into(),
            position: Some(Position {
                x: 77.0,
                y: 88.0,
                extra: Map::new(),
            }),
        },
    )
    .unwrap();
    assert_eq!(
        project.layout.positions["shot:shot-1"].extra["future"],
        json!(true)
    );
    let error = apply(
        &mut project,
        &Op::MoveNode {
            node: "shot-1".into(),
            position: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.field, "node");
    assert!(error.valid.contains(&"shot:shot-1".into()));
}

fn pair_fixture() -> Project {
    let mut project = fixture();
    seed_unknown_fields(&mut project);
    setup_empty_scene(&mut project);
    setup_extra_shot(&mut project);
    setup_extra_panel(&mut project);
    setup_extra_layer(&mut project);
    setup_extra_sticky(&mut project);
    setup_refs(&mut project);

    let mut removable_shot = project.scenes[0]
        .shots
        .iter()
        .find(|shot| shot.id == "shot-1")
        .unwrap()
        .clone();
    removable_shot.id = "shot-removable".into();
    removable_shot.selected_take = "take-removable-1".into();
    removable_shot.refs = vec!["refs/removable-a.png".into(), "refs/removable-b.png".into()];
    removable_shot.takes[0].id = "take-removable-1".into();
    removable_shot.takes[0].refs = vec!["refs/removable-take.png".into()];
    removable_shot.takes[0].panels[0].id = "panel-removable-1".into();
    removable_shot.takes[0].panels[0].layers[0].id = "layer-removable-1".into();
    removable_shot.takes[0].panels[0].layers[1].id = "layer-removable-2".into();
    removable_shot.takes[0].panels[1].id = "panel-removable-1b".into();
    removable_shot.takes[0].panels[1].time = 1.0;
    removable_shot.takes[0].panels[1].layers.clear();
    removable_shot.takes[1].id = "take-removable-2".into();
    removable_shot.takes[1].panels[0].id = "panel-removable-2".into();

    let mut removable_only_shot = project.scenes[0]
        .shots
        .iter()
        .find(|shot| shot.id == "shot-2")
        .unwrap()
        .clone();
    removable_only_shot.id = "shot-remove-only".into();
    removable_only_shot.selected_take = "take-remove-only".into();
    removable_only_shot.takes[0].id = "take-remove-only".into();
    removable_only_shot.takes[0].panels[0].id = "panel-remove-only".into();
    project.scenes.push(Scene {
        id: "scene-removable".into(),
        title: "Removable".into(),
        shots: vec![removable_shot, removable_only_shot],
        extra: Map::new(),
    });

    let sticky_template = project.stickies[0].clone();
    for (id, pin) in [
        ("sticky-remove", Pin::None),
        ("sticky-resolve", Pin::None),
        ("sticky-pin-scene", Pin::Scene("scene-removable".into())),
        ("sticky-pin-shot", Pin::Shot("shot-removable".into())),
        (
            "sticky-pin-take",
            Pin::Take {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
            },
        ),
        (
            "sticky-pin-panel",
            Pin::Panel {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                panel: "panel-removable-1".into(),
            },
        ),
        (
            "sticky-pin-layer",
            Pin::Layer {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                panel: "panel-removable-1".into(),
                layer: "layer-removable-1".into(),
            },
        ),
    ] {
        let mut sticky = sticky_template.clone();
        sticky.id = id.into();
        sticky.text = id.into();
        sticky.pin = pin;
        sticky.reply_to = None;
        sticky.resolved = false;
        project.stickies.push(sticky);
    }

    let mut removable_cut = project.cuts[1].clone();
    removable_cut.id = "cut-removable".into();
    removable_cut.name = "Removable".into();
    removable_cut.clips.clear();
    project.cuts.push(removable_cut);
    for node in [
        "shot:shot-removable",
        "sticky:sticky-extra",
        "cut:cut-removable",
    ] {
        project.layout.positions.insert(
            node.into(),
            Position {
                x: 12.0,
                y: 34.0,
                extra: Map::new(),
            },
        );
    }
    let issues = validate(&project);
    assert!(issues.is_empty(), "extended pair fixture: {issues:#?}");
    project
}

fn pair_ops() -> Vec<(&'static str, Op)> {
    let mut operations: Vec<_> = cases()
        .into_iter()
        .map(|case| (case.name, case.op))
        .collect();
    operations.extend([
        (
            "batch_secondary",
            Op::Batch {
                ops: vec![
                    Op::UpdateScene {
                        scene: "scene-2".into(),
                        patch: patch([("title", json!("Exit revised"))]),
                    },
                    Op::UpdateShot {
                        shot: "shot-3".into(),
                        patch: patch([("notes", json!("Batch note"))]),
                    },
                ],
            },
        ),
        (
            "add_scene_secondary",
            Op::AddScene {
                id: Some("scene-matrix".into()),
                title: "Matrix".into(),
                index: Some(2),
            },
        ),
        (
            "remove_scene_with_shots",
            Op::RemoveScene {
                scene: "scene-removable".into(),
            },
        ),
        (
            "move_scene_secondary",
            Op::MoveScene {
                scene: "scene-1".into(),
                index: 2,
            },
        ),
        (
            "update_scene_secondary",
            Op::UpdateScene {
                scene: "scene-2".into(),
                patch: patch([("title", json!("Changed again"))]),
            },
        ),
        (
            "add_shot_secondary",
            Op::AddShot {
                scene: "scene-2".into(),
                id: Some("shot-matrix".into()),
                title: "Matrix".into(),
                index: Some(1),
            },
        ),
        (
            "remove_shot_secondary",
            Op::RemoveShot {
                shot: "shot-remove-only".into(),
            },
        ),
        (
            "move_shot_secondary",
            Op::MoveShot {
                shot: "shot-1".into(),
                index: 2,
            },
        ),
        (
            "update_shot_title",
            Op::UpdateShot {
                shot: "shot-1".into(),
                patch: patch([("title", json!("Door revised"))]),
            },
        ),
        (
            "update_shot_duration",
            Op::UpdateShot {
                shot: "shot-1".into(),
                patch: patch([("duration", json!(3.0))]),
            },
        ),
        (
            "add_take_secondary",
            Op::AddTake {
                shot: "shot-1".into(),
                id: Some("take-matrix".into()),
                name: "Matrix".into(),
                index: Some(1),
            },
        ),
        (
            "remove_take_secondary",
            Op::RemoveTake {
                shot: "shot-1".into(),
                take: "take-2".into(),
            },
        ),
        (
            "move_take_secondary",
            Op::MoveTake {
                shot: "shot-1".into(),
                take: "take-1".into(),
                index: 1,
            },
        ),
        (
            "update_take_secondary",
            Op::UpdateTake {
                shot: "shot-1".into(),
                take: "take-2".into(),
                patch: patch([("name", json!("Close revised"))]),
            },
        ),
        (
            "select_take_secondary",
            Op::SelectTake {
                shot: "shot-removable".into(),
                take: "take-removable-2".into(),
            },
        ),
        (
            "duplicate_take_secondary",
            Op::DuplicateTake {
                shot: "shot-1".into(),
                take: "take-2".into(),
                id: Some("take-matrix-copy".into()),
                index: Some(1),
            },
        ),
        (
            "add_panel_secondary",
            Op::AddPanel {
                shot: "shot-1".into(),
                take: "take-2".into(),
                id: Some("panel-matrix".into()),
                time: Some(1.0),
                index: Some(0),
            },
        ),
        (
            "remove_panel_secondary",
            Op::RemovePanel {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                panel: "panel-removable-1".into(),
            },
        ),
        (
            "move_panel_secondary",
            Op::MovePanel {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                panel: "panel-removable-1b".into(),
                index: 0,
            },
        ),
        (
            "update_panel_time",
            Op::UpdatePanel {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                patch: patch([("time", json!(0.5))]),
            },
        ),
        (
            "update_panel_title_secondary",
            Op::UpdatePanel {
                shot: "shot-1".into(),
                take: "take-2".into(),
                panel: "panel-2".into(),
                patch: patch([("title", json!("Close title"))]),
            },
        ),
        (
            "add_layer_secondary",
            Op::AddLayer {
                shot: "shot-1".into(),
                take: "take-2".into(),
                panel: "panel-2".into(),
                id: Some("layer-matrix".into()),
                layer_type: "box".into(),
                fields: Map::new(),
                index: Some(0),
            },
        ),
        (
            "remove_layer_secondary",
            Op::RemoveLayer {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                panel: "panel-removable-1".into(),
                layer: "layer-removable-1".into(),
            },
        ),
        (
            "move_layer_secondary",
            Op::MoveLayer {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                panel: "panel-removable-1".into(),
                layer: "layer-removable-2".into(),
                index: 0,
            },
        ),
        (
            "update_layer_secondary",
            Op::UpdateLayer {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                layer: "layer-1".into(),
                patch: patch([("opacity", json!(0.5))]),
            },
        ),
        (
            "add_sticky_secondary",
            Op::AddSticky {
                id: Some("sticky-matrix".into()),
                text: "Matrix".into(),
                kind: "question".into(),
                author: Author::Human,
                pin: Pin::Shot("shot-removable".into()),
                created: "2026-11-07T00:00:03Z".into(),
            },
        ),
        (
            "remove_sticky_secondary",
            Op::RemoveSticky {
                sticky: "sticky-extra".into(),
            },
        ),
        (
            "move_sticky_secondary",
            Op::MoveSticky {
                sticky: "sticky-1".into(),
                index: 2,
            },
        ),
        (
            "update_sticky_pin",
            Op::UpdateSticky {
                sticky: "sticky-extra".into(),
                patch: patch([("pin", json!("shot:shot-removable"))]),
            },
        ),
        (
            "update_sticky_text",
            Op::UpdateSticky {
                sticky: "sticky-1".into(),
                patch: patch([("text", json!("Check framing again"))]),
            },
        ),
        (
            "add_clip_secondary",
            Op::AddClip {
                cut: "cut-alt".into(),
                id: Some("clip-matrix".into()),
                source: shot_source("shot-1", Some("take-1")),
                index: Some(0),
            },
        ),
        (
            "remove_clip_secondary",
            Op::RemoveClip {
                cut: "cut-alt".into(),
                clip: "clip-3".into(),
            },
        ),
        (
            "reorder_clip_secondary",
            Op::ReorderClip {
                cut: "cut-main".into(),
                clip: "clip-1".into(),
                index: 1,
            },
        ),
        (
            "update_clip_trim_secondary",
            Op::UpdateClip {
                cut: "cut-alt".into(),
                clip: "clip-3".into(),
                patch: patch([("in", json!(1)), ("slot", json!(10))]),
            },
        ),
        (
            "update_clip_source_secondary",
            Op::UpdateClip {
                cut: "cut-alt".into(),
                clip: "clip-3".into(),
                patch: patch([("source", json!({"shot": {"shot": "shot-4"}}))]),
            },
        ),
        (
            "add_cut_secondary",
            Op::AddCut {
                id: Some("cut-matrix".into()),
                name: "Matrix".into(),
                index: Some(1),
            },
        ),
        (
            "remove_cut_secondary",
            Op::RemoveCut {
                cut: "cut-removable".into(),
            },
        ),
        (
            "set_active_cut_removable",
            Op::SetActiveCut {
                cut: Some("cut-removable".into()),
            },
        ),
        (
            "move_shot_between_secondary",
            Op::MoveShotBetweenScenes {
                shot: "shot-removable".into(),
                scene: "scene-2".into(),
                index: 0,
            },
        ),
        (
            "add_ref_take",
            Op::AddRef {
                target: RefTarget::Take {
                    shot: "shot-1".into(),
                    take: "take-1".into(),
                },
                reference: "refs/take-matrix.png".into(),
                index: Some(0),
            },
        ),
        (
            "add_ref_shot_secondary",
            Op::AddRef {
                target: RefTarget::Shot {
                    shot: "shot-2".into(),
                },
                reference: "refs/shot-matrix.png".into(),
                index: Some(0),
            },
        ),
        (
            "remove_ref_secondary",
            Op::RemoveRef {
                target: RefTarget::Shot {
                    shot: "shot-1".into(),
                },
                reference: "refs/second.png".into(),
            },
        ),
        (
            "remove_ref_take",
            Op::RemoveRef {
                target: RefTarget::Take {
                    shot: "shot-removable".into(),
                    take: "take-removable-1".into(),
                },
                reference: "refs/removable-take.png".into(),
            },
        ),
        (
            "resolve_sticky_secondary",
            Op::ResolveSticky {
                sticky: "sticky-resolve".into(),
            },
        ),
        (
            "reply_sticky_secondary",
            Op::ReplySticky {
                sticky: "sticky-extra".into(),
                id: Some("sticky-matrix-reply".into()),
                text: "Matrix reply".into(),
                kind: "note".into(),
                author: Author::Human,
                created: "2026-11-07T00:00:04Z".into(),
            },
        ),
        (
            "move_node_cut",
            Op::MoveNode {
                node: "cut:cut-alt".into(),
                position: Some(Position {
                    x: 300.0,
                    y: 400.0,
                    extra: Map::new(),
                }),
            },
        ),
        (
            "move_node_sticky",
            Op::MoveNode {
                node: "sticky:sticky-extra".into(),
                position: Some(Position {
                    x: 500.0,
                    y: 600.0,
                    extra: Map::new(),
                }),
            },
        ),
        (
            "update_panel_time_secondary",
            Op::UpdatePanel {
                shot: "shot-removable".into(),
                take: "take-removable-2".into(),
                panel: "panel-removable-2".into(),
                patch: patch([("time", json!(0.25))]),
            },
        ),
        (
            "duplicate_take_shared",
            Op::DuplicateTake {
                shot: "shot-removable".into(),
                take: "take-removable-1".into(),
                id: Some("take-removable-copy".into()),
                index: Some(1),
            },
        ),
        ("set_active_cut_none", Op::SetActiveCut { cut: None }),
        (
            "move_shot_between_third",
            Op::MoveShotBetweenScenes {
                shot: "shot-1".into(),
                scene: "scene-2".into(),
                index: 0,
            },
        ),
        (
            "update_panel_background",
            Op::UpdatePanel {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                patch: patch([("background", json!("#334455"))]),
            },
        ),
    ]);
    operations
}

#[test]
fn footprints_claim_known_success_dependencies() {
    let base = pair_fixture();
    let cases = [
        (
            "move shot reads its subtree",
            Op::MoveShotBetweenScenes {
                shot: "shot-removable".into(),
                scene: "scene-2".into(),
                index: 0,
            },
            "shot/shot-removable",
        ),
        (
            "remove scene reads each shot subtree",
            Op::RemoveScene {
                scene: "scene-removable".into(),
            },
            "shot/shot-removable",
        ),
        (
            "panel time reads its panel",
            Op::UpdatePanel {
                shot: "shot-1".into(),
                take: "take-1".into(),
                panel: "panel-1".into(),
                patch: patch([("time", json!(0.5))]),
            },
            "shot/shot-1/take/take-1/panel/panel-1",
        ),
        (
            "active cut reads the selected cut",
            Op::SetActiveCut {
                cut: Some("cut-removable".into()),
            },
            "cut/cut-removable",
        ),
    ];
    for (name, op, expected) in cases {
        let mut project = base.clone();
        let applied = apply(&mut project, &op).unwrap();
        assert!(
            applied.footprints.iter().any(|path| path == expected),
            "{name}: missing {expected} in {:?}",
            applied.footprints
        );
    }
}

#[test]
fn footprint_disjoint_pairs_commute_and_inverse_keeps_the_other_change() {
    let base = pair_fixture();
    let operations = pair_ops();
    assert_eq!(
        operations.len(),
        92,
        "the matrix must keep its review breadth"
    );
    let mut effects = Vec::new();
    for (name, op) in &operations {
        let mut project = base.clone();
        let effect = apply(&mut project, op)
            .unwrap_or_else(|error| panic!("pair operation {name} must apply: {error:?}"));
        assert_ne!(
            project, base,
            "pair operation {name} must change the fixture"
        );
        effects.push(effect);
    }

    let mut ordered_pairs = 0usize;
    let mut checked = 0usize;
    let mut commute_failures = 0usize;
    let mut inverse_failures = 0usize;
    let mut failure_details = Vec::new();
    for left_index in 0..operations.len() {
        for right_index in 0..operations.len() {
            if left_index == right_index {
                continue;
            }
            ordered_pairs += 1;
            if !disjoint(
                &effects[left_index].footprints,
                &effects[right_index].footprints,
            ) {
                continue;
            }
            checked += 1;
            let (left_name, left) = &operations[left_index];
            let (right_name, right) = &operations[right_index];

            let mut right_only = base.clone();
            apply(&mut right_only, right).unwrap_or_else(|error| {
                panic!("standalone {right_name} failed during pair check: {error:?}")
            });

            let mut left_then_right = base.clone();
            let left_applied = apply(&mut left_then_right, left);
            let left_then_right_error = match &left_applied {
                Ok(_) => apply(&mut left_then_right, right).err(),
                Err(error) => Some(error.clone()),
            };

            let mut right_then_left = base.clone();
            let left_after_right = match apply(&mut right_then_left, right) {
                Ok(_) => apply(&mut right_then_left, left),
                Err(error) => Err(error),
            };

            let commute_failed = left_then_right_error.is_some()
                || left_after_right.is_err()
                || left_then_right != right_then_left;
            if commute_failed {
                commute_failures += 1;
                if failure_details.len() < 80 {
                    failure_details.push(format!(
                        "commute {left_name} / {right_name}: AB={:?}, BA={:?}",
                        left_then_right_error,
                        left_after_right.as_ref().err()
                    ));
                }
            }

            let mut inverse_failed = false;
            if let Ok(left_applied) = left_applied {
                if left_then_right_error.is_none() {
                    for inverse in &left_applied.inverse {
                        if apply(&mut left_then_right, inverse).is_err() {
                            inverse_failed = true;
                            break;
                        }
                    }
                    inverse_failed |= left_then_right != right_only;
                } else {
                    inverse_failed = true;
                }
            } else {
                inverse_failed = true;
            }
            if let Ok(left_after_right) = left_after_right {
                for inverse in &left_after_right.inverse {
                    if apply(&mut right_then_left, inverse).is_err() {
                        inverse_failed = true;
                        break;
                    }
                }
                inverse_failed |= right_then_left != right_only;
            } else {
                inverse_failed = true;
            }
            if inverse_failed {
                inverse_failures += 1;
                if failure_details.len() < 80 {
                    failure_details.push(format!(
                        "inverse {left_name} around {right_name} did not preserve the right op"
                    ));
                }
            }
        }
    }
    assert_eq!(ordered_pairs, 8_372);
    assert!(
        checked >= 1_000,
        "only {checked} disjoint pairs were exercised"
    );
    assert_eq!(
        (commute_failures, inverse_failures),
        (0, 0),
        "checked {checked} disjoint ordered pairs; failures:\n{}",
        failure_details.join("\n")
    );
    println!(
        "ordered pair matrix: {ordered_pairs} total, {checked} disjoint, 0 commute failures, 0 inverse failures"
    );
}

#[cfg(feature = "schema")]
#[test]
fn op_schema_publishes_external_wire_shapes_only() {
    let schema = schemars::schema_for!(Op);
    let wire = serde_json::to_value(schema).unwrap().to_string();
    assert!(wire.contains("add_scene"));
    assert!(!wire.contains("restore_changes"));
}

#[test]
fn footprint_cut_sources_share_the_cut_graph() {
    let mut project = fixture();
    for id in ["cut-a", "cut-b", "cut-c", "cut-d"] {
        apply(
            &mut project,
            &Op::AddCut {
                id: Some(id.into()),
                name: id.into(),
                index: None,
            },
        )
        .expect("cut added");
    }
    let first_shot = project.scenes[0].shots[0].id.clone();
    for (cut, clip) in [("cut-a", "clip-a"), ("cut-c", "clip-c")] {
        apply(
            &mut project,
            &Op::AddClip {
                cut: cut.into(),
                id: Some(clip.into()),
                source: shot_source(&first_shot, None),
                index: None,
            },
        )
        .expect("clip added");
    }
    let cut_source = |cut: &str| {
        serde_json::to_value(ClipSource::Cut(storyboard_core::CutSource {
            cut: cut.into(),
            extra: Default::default(),
            source_extra: Default::default(),
        }))
        .expect("source serializes")
    };
    let update = |cut: &str, clip: &str, target: &str| Op::UpdateClip {
        cut: cut.into(),
        clip: clip.into(),
        patch: patch([("source", cut_source(target))]),
    };
    let a = apply(&mut project.clone(), &update("cut-a", "clip-a", "cut-b")).expect("a applies");
    let b = apply(&mut project.clone(), &update("cut-c", "clip-c", "cut-d")).expect("b applies");
    assert!(
        !disjoint(&a.footprints, &b.footprints),
        "two ops that change cut edges can form a cycle together, so they must overlap: {:?} {:?}",
        a.footprints,
        b.footprints
    );
}
