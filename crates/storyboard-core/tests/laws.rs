use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use proptest::prelude::*;
use serde_json::{Map, Value, json};
use storyboard_core::{
    AgentAuthor, Author, ClipSource, Op, Pin, Position, Project, RefTarget, RenderMode, ShotSource,
    apply, compile_active, footprint::disjoint, render_panel, validate,
};

mod law_support;

use law_support::{
    all_scene_ids, all_shot_ids, choose_target, drawable_project_strategy,
    operation_project_strategy, project_strategy,
};

const SMALL: &str = include_str!("fixtures/small.json");
const OP_COUNT: usize = 40;

fn fixture() -> Project {
    serde_json::from_str(SMALL).expect("small fixture")
}

fn first_scene(project: &Project) -> String {
    project.scenes[0].id.clone()
}

fn first_shot(project: &Project) -> String {
    project.scenes[0].shots[0].id.clone()
}

fn first_take(project: &Project) -> String {
    project.scenes[0].shots[0].takes[0].id.clone()
}

fn first_panel(project: &Project) -> String {
    project.scenes[0].shots[0].takes[0].panels[0].id.clone()
}

fn first_sticky(project: &Project) -> String {
    project.stickies[0].id.clone()
}

fn removable_sticky(project: &Project) -> String {
    project
        .stickies
        .iter()
        .find(|candidate| {
            !project
                .stickies
                .iter()
                .any(|sticky| sticky.reply_to.as_deref() == Some(candidate.id.as_str()))
        })
        .expect("generated project has a leaf sticky")
        .id
        .clone()
}

fn generated_id(kind: &str, token: u64, suffix: &str) -> String {
    format!("{kind}-{token:016x}-{suffix}")
}

fn take_contexts(project: &Project) -> Vec<(String, String)> {
    project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .flat_map(|shot| {
            shot.takes
                .iter()
                .map(|take| (shot.id.clone(), take.id.clone()))
        })
        .collect()
}

fn panel_contexts(project: &Project) -> Vec<(String, String, String)> {
    project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .flat_map(|shot| {
            shot.takes.iter().flat_map(|take| {
                take.panels
                    .iter()
                    .map(|panel| (shot.id.clone(), take.id.clone(), panel.id.clone()))
            })
        })
        .collect()
}

fn layer_contexts(project: &Project) -> Vec<(String, String, String, String)> {
    project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .flat_map(|shot| {
            shot.takes.iter().flat_map(|take| {
                take.panels.iter().flat_map(|panel| {
                    panel.layers.iter().map(|layer| {
                        (
                            shot.id.clone(),
                            take.id.clone(),
                            panel.id.clone(),
                            layer.id.clone(),
                        )
                    })
                })
            })
        })
        .collect()
}

fn clip_contexts(project: &Project) -> Vec<(String, String)> {
    project
        .cuts
        .iter()
        .flat_map(|cut| {
            cut.clips
                .iter()
                .map(|clip| (cut.id.clone(), clip.id.clone()))
        })
        .collect()
}

fn patch(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn agent(name: impl Into<String>) -> Author {
    Author::Agent {
        agent: AgentAuthor {
            name: name.into(),
            extra: Map::new(),
        },
        extra: Map::new(),
    }
}

fn generated_author(agent_authored: bool, salt: &str) -> Author {
    if agent_authored {
        agent(format!("agent-{salt}"))
    } else {
        Author::Human
    }
}

fn shot_source(shot: &str) -> ClipSource {
    ClipSource::Shot(ShotSource {
        shot: shot.into(),
        take: None,
        extra: Map::new(),
        source_extra: Map::new(),
    })
}

fn setup(project: &mut Project, op: Op) {
    apply(project, &op).unwrap_or_else(|error| panic!("law setup failed for {op:?}: {error:?}"));
}

fn nested_batch(mut op: Op, depth: usize) -> Op {
    for _ in 0..depth {
        op = Op::Batch { ops: vec![op] };
    }
    op
}

fn sequence_op(project: &Project, selector: u64, index: usize, token: u64) -> Op {
    let changed = format!("sequence-{token:x}-{index}");
    match selector % 8 {
        0 => Op::UpdateScene {
            scene: choose_target(&all_scene_ids(project), selector, "scene"),
            patch: patch([("title", json!(changed))]),
        },
        1 => Op::UpdateShot {
            shot: choose_target(&all_shot_ids(project), selector, "shot"),
            patch: patch([("action", json!(changed))]),
        },
        2 => {
            let contexts = take_contexts(project);
            let (shot, take) = &contexts[selector as usize % contexts.len()];
            Op::UpdateTake {
                shot: shot.clone(),
                take: if selector.is_multiple_of(17) {
                    format!("missing-take-{selector:x}")
                } else {
                    take.clone()
                },
                patch: patch([("name", json!(changed))]),
            }
        }
        3 => {
            let contexts = panel_contexts(project);
            let (shot, take, panel) = &contexts[selector as usize % contexts.len()];
            Op::UpdatePanel {
                shot: shot.clone(),
                take: take.clone(),
                panel: if selector.is_multiple_of(17) {
                    format!("missing-panel-{selector:x}")
                } else {
                    panel.clone()
                },
                patch: patch([("title", json!(changed))]),
            }
        }
        4 => {
            let contexts = layer_contexts(project);
            if let Some((shot, take, panel, layer)) =
                contexts.get(selector as usize % contexts.len().max(1))
            {
                Op::UpdateLayer {
                    shot: shot.clone(),
                    take: take.clone(),
                    panel: panel.clone(),
                    layer: if selector.is_multiple_of(17) {
                        format!("missing-layer-{selector:x}")
                    } else {
                        layer.clone()
                    },
                    patch: patch([("label", json!(changed))]),
                }
            } else {
                let (shot, take, panel) = &panel_contexts(project)[0];
                Op::UpdateLayer {
                    shot: shot.clone(),
                    take: take.clone(),
                    panel: panel.clone(),
                    layer: format!("missing-layer-{selector:x}"),
                    patch: patch([("label", json!(changed))]),
                }
            }
        }
        5 => Op::UpdateSticky {
            sticky: choose_target(
                &project
                    .stickies
                    .iter()
                    .map(|sticky| sticky.id.clone())
                    .collect::<Vec<_>>(),
                selector,
                "sticky",
            ),
            patch: patch([("text", json!(changed))]),
        },
        6 => {
            let contexts = clip_contexts(project);
            if let Some((cut, clip)) = contexts.get(selector as usize % contexts.len().max(1)) {
                Op::UpdateClip {
                    cut: cut.clone(),
                    clip: if selector.is_multiple_of(17) {
                        format!("missing-clip-{selector:x}")
                    } else {
                        clip.clone()
                    },
                    patch: patch([("in", json!((index + 1) as u64))]),
                }
            } else {
                Op::UpdateClip {
                    cut: format!("missing-cut-{selector:x}"),
                    clip: format!("missing-clip-{selector:x}"),
                    patch: patch([("in", json!((index + 1) as u64))]),
                }
            }
        }
        _ => Op::MoveNode {
            node: format!(
                "shot:{}",
                choose_target(&all_shot_ids(project), selector, "shot")
            ),
            position: Some(Position {
                x: index as f64,
                y: (token % 100) as f64,
                extra: Map::from_iter([("future_position".into(), json!(token))]),
            }),
        },
    }
}

fn invalid_sequence_op(index: usize) -> Op {
    Op::UpdateScene {
        scene: format!("missing-scene-{index}"),
        patch: patch([("title", json!("invalid"))]),
    }
}

fn ensure_cut(project: &mut Project, token: u64, suffix: &str) -> String {
    if let Some(cut) = project.cuts.first() {
        return cut.id.clone();
    }
    let cut = generated_id("cut", token, suffix);
    setup(
        project,
        Op::AddCut {
            id: Some(cut.clone()),
            name: "Generated cut".into(),
            index: None,
        },
    );
    cut
}

fn add_generated_clip(project: &mut Project, cut: &str, token: u64, suffix: &str) -> String {
    let clip = generated_id("clip", token, suffix);
    setup(
        project,
        Op::AddClip {
            cut: cut.into(),
            id: Some(clip.clone()),
            source: shot_source(&first_shot(project)),
            index: None,
        },
    );
    clip
}

/// Returns one applicable public operation for every public operation variant. Every target is
/// selected from the generated project (or from setup applied to that project); only add ids are
/// necessarily new.
fn generated_op_case(
    kind: usize,
    base: &Project,
    token: u64,
    agent_authored: bool,
) -> (&'static str, Project, Op) {
    let mut project = base.clone();
    let changed = format!("generated-{token:x}-{kind}");
    let fresh = |entity: &str, suffix: &str| generated_id(entity, token, suffix);
    let scene = first_scene(&project);
    let shot = first_shot(&project);
    let take = first_take(&project);
    let panel = first_panel(&project);
    let author = generated_author(agent_authored, &format!("{token:x}"));
    let case = match kind {
        0 => (
            "batch",
            Op::Batch {
                ops: vec![
                    Op::UpdateShot {
                        shot: shot.clone(),
                        patch: patch([("title", json!(changed))]),
                    },
                    Op::Batch {
                        ops: vec![Op::UpdateScene {
                            scene: scene.clone(),
                            patch: patch([("title", json!(format!("batch-{token:x}")))]),
                        }],
                    },
                ],
            },
        ),
        1 => (
            "add_scene",
            Op::AddScene {
                id: Some(fresh("scene", "add")),
                title: changed,
                index: Some(0),
            },
        ),
        2 => {
            let removable = fresh("scene", "remove");
            setup(
                &mut project,
                Op::AddScene {
                    id: Some(removable.clone()),
                    title: changed,
                    index: None,
                },
            );
            setup(
                &mut project,
                Op::AddShot {
                    scene: removable.clone(),
                    id: Some(fresh("shot", "remove-scene")),
                    title: "Nested shot".into(),
                    index: None,
                },
            );
            ("remove_scene", Op::RemoveScene { scene: removable })
        }
        3 => {
            if project.scenes.len() == 1 {
                setup(
                    &mut project,
                    Op::AddScene {
                        id: Some(fresh("scene", "move")),
                        title: changed,
                        index: None,
                    },
                );
            }
            (
                "move_scene",
                Op::MoveScene {
                    scene: project.scenes.last().expect("scene").id.clone(),
                    index: 0,
                },
            )
        }
        4 => (
            "update_scene",
            Op::UpdateScene {
                scene,
                patch: patch([("title", json!(changed))]),
            },
        ),
        5 => (
            "add_shot",
            Op::AddShot {
                scene,
                id: Some(fresh("shot", "add")),
                title: changed,
                index: Some(0),
            },
        ),
        6 => {
            let removable = fresh("shot", "remove");
            setup(
                &mut project,
                Op::AddShot {
                    scene,
                    id: Some(removable.clone()),
                    title: changed,
                    index: None,
                },
            );
            ("remove_shot", Op::RemoveShot { shot: removable })
        }
        7 => {
            if project.scenes[0].shots.len() == 1 {
                setup(
                    &mut project,
                    Op::AddShot {
                        scene: scene.clone(),
                        id: Some(fresh("shot", "move")),
                        title: changed,
                        index: None,
                    },
                );
            }
            (
                "move_shot",
                Op::MoveShot {
                    shot: project.scenes[0].shots.last().expect("shot").id.clone(),
                    index: 0,
                },
            )
        }
        8 => (
            "update_shot",
            Op::UpdateShot {
                shot,
                patch: patch([("action", json!(changed))]),
            },
        ),
        9 => (
            "add_take",
            Op::AddTake {
                shot,
                id: Some(fresh("take", "add")),
                name: changed,
                index: Some(0),
            },
        ),
        10 => {
            let removable = fresh("take", "remove");
            setup(
                &mut project,
                Op::AddTake {
                    shot: shot.clone(),
                    id: Some(removable.clone()),
                    name: changed,
                    index: None,
                },
            );
            (
                "remove_take",
                Op::RemoveTake {
                    shot,
                    take: removable,
                },
            )
        }
        11 => {
            if project.scenes[0].shots[0].takes.len() == 1 {
                setup(
                    &mut project,
                    Op::AddTake {
                        shot: shot.clone(),
                        id: Some(fresh("take", "move")),
                        name: changed,
                        index: None,
                    },
                );
            }
            (
                "move_take",
                Op::MoveTake {
                    shot,
                    take: project.scenes[0].shots[0]
                        .takes
                        .last()
                        .expect("take")
                        .id
                        .clone(),
                    index: 0,
                },
            )
        }
        12 => (
            "update_take",
            Op::UpdateTake {
                shot,
                take,
                patch: patch([("name", json!(changed))]),
            },
        ),
        13 => {
            let selectable = fresh("take", "select");
            setup(
                &mut project,
                Op::AddTake {
                    shot: shot.clone(),
                    id: Some(selectable.clone()),
                    name: changed,
                    index: None,
                },
            );
            (
                "select_take",
                Op::SelectTake {
                    shot,
                    take: selectable,
                },
            )
        }
        14 => (
            "duplicate_take",
            Op::DuplicateTake {
                shot,
                take,
                id: Some(fresh("take", "duplicate")),
                index: Some(0),
            },
        ),
        15 => (
            "add_panel",
            Op::AddPanel {
                shot,
                take,
                id: Some(fresh("panel", "add")),
                time: Some(0.5),
                index: Some(0),
            },
        ),
        16 | 17 => {
            let generated = fresh("panel", "move-remove");
            setup(
                &mut project,
                Op::AddPanel {
                    shot: shot.clone(),
                    take: take.clone(),
                    id: Some(generated.clone()),
                    time: Some(0.5),
                    index: None,
                },
            );
            if kind == 16 {
                (
                    "remove_panel",
                    Op::RemovePanel {
                        shot,
                        take,
                        panel: generated,
                    },
                )
            } else {
                (
                    "move_panel",
                    Op::MovePanel {
                        shot,
                        take,
                        panel: generated,
                        index: 0,
                    },
                )
            }
        }
        18 => (
            "update_panel",
            Op::UpdatePanel {
                shot,
                take,
                panel,
                patch: patch([("title", json!(changed))]),
            },
        ),
        19 => (
            "add_layer",
            Op::AddLayer {
                shot,
                take,
                panel,
                id: Some(fresh("layer", "add")),
                layer_type: "box".into(),
                fields: Map::new(),
                index: Some(0),
            },
        ),
        20 | 21 => {
            if kind == 21
                && project.scenes[0].shots[0].takes[0].panels[0]
                    .layers
                    .is_empty()
            {
                setup(
                    &mut project,
                    Op::AddLayer {
                        shot: shot.clone(),
                        take: take.clone(),
                        panel: panel.clone(),
                        id: Some(fresh("layer", "move-anchor")),
                        layer_type: "box".into(),
                        fields: Map::new(),
                        index: None,
                    },
                );
            }
            let generated = fresh("layer", "move-remove");
            setup(
                &mut project,
                Op::AddLayer {
                    shot: shot.clone(),
                    take: take.clone(),
                    panel: panel.clone(),
                    id: Some(generated.clone()),
                    layer_type: "box".into(),
                    fields: Map::new(),
                    index: None,
                },
            );
            if kind == 20 {
                (
                    "remove_layer",
                    Op::RemoveLayer {
                        shot,
                        take,
                        panel,
                        layer: generated,
                    },
                )
            } else {
                (
                    "move_layer",
                    Op::MoveLayer {
                        shot,
                        take,
                        panel,
                        layer: generated,
                        index: 0,
                    },
                )
            }
        }
        22 => {
            let (layer_shot, layer_take, layer_panel, layer) =
                if let Some(context) = layer_contexts(&project).first() {
                    context.clone()
                } else {
                    let generated = fresh("layer", "update");
                    setup(
                        &mut project,
                        Op::AddLayer {
                            shot: shot.clone(),
                            take: take.clone(),
                            panel: panel.clone(),
                            id: Some(generated.clone()),
                            layer_type: "box".into(),
                            fields: Map::new(),
                            index: None,
                        },
                    );
                    (shot, take, panel, generated)
                };
            (
                "update_layer",
                Op::UpdateLayer {
                    shot: layer_shot,
                    take: layer_take,
                    panel: layer_panel,
                    layer,
                    patch: patch([("label", json!(changed))]),
                },
            )
        }
        23 => (
            "add_sticky",
            Op::AddSticky {
                id: Some(fresh("sticky", "add")),
                text: changed,
                kind: "note".into(),
                author,
                pin: Pin::None,
                created: "2026-11-07T00:00:02Z".into(),
            },
        ),
        24 => (
            "remove_sticky",
            Op::RemoveSticky {
                sticky: removable_sticky(&project),
            },
        ),
        25 => (
            "move_sticky",
            Op::MoveSticky {
                sticky: first_sticky(&project),
                index: project.stickies.len().saturating_sub(1),
            },
        ),
        26 => (
            "update_sticky",
            Op::UpdateSticky {
                sticky: first_sticky(&project),
                patch: patch([("text", json!(changed))]),
            },
        ),
        27 => {
            let cut = ensure_cut(&mut project, token, "add-clip");
            (
                "add_clip",
                Op::AddClip {
                    cut,
                    id: Some(fresh("clip", "add")),
                    source: shot_source(&first_shot(&project)),
                    index: Some(0),
                },
            )
        }
        28..=30 => {
            let cut = ensure_cut(&mut project, token, "clip-op");
            if kind == 29
                && project
                    .cuts
                    .iter()
                    .find(|item| item.id == cut)
                    .expect("cut")
                    .clips
                    .is_empty()
            {
                add_generated_clip(&mut project, &cut, token, "clip-anchor");
            }
            let clip = add_generated_clip(&mut project, &cut, token, "clip-op");
            if kind == 28 {
                ("remove_clip", Op::RemoveClip { cut, clip })
            } else if kind == 29 {
                (
                    "reorder_clip",
                    Op::ReorderClip {
                        cut,
                        clip,
                        index: 0,
                    },
                )
            } else {
                (
                    "update_clip",
                    Op::UpdateClip {
                        cut,
                        clip,
                        patch: patch([("in", json!(1))]),
                    },
                )
            }
        }
        31 => (
            "add_cut",
            Op::AddCut {
                id: Some(fresh("cut", "add")),
                name: changed,
                index: Some(0),
            },
        ),
        32 => {
            let cut = fresh("cut", "remove");
            setup(
                &mut project,
                Op::AddCut {
                    id: Some(cut.clone()),
                    name: changed,
                    index: None,
                },
            );
            ("remove_cut", Op::RemoveCut { cut })
        }
        33 => {
            if project.active_cut.is_some() {
                ("set_active_cut", Op::SetActiveCut { cut: None })
            } else {
                let cut = ensure_cut(&mut project, token, "active");
                ("set_active_cut", Op::SetActiveCut { cut: Some(cut) })
            }
        }
        34 => {
            if project.scenes.len() == 1 {
                setup(
                    &mut project,
                    Op::AddScene {
                        id: Some(fresh("scene", "destination")),
                        title: "Destination".into(),
                        index: None,
                    },
                );
            }
            (
                "move_shot_between_scenes",
                Op::MoveShotBetweenScenes {
                    shot,
                    scene: project.scenes[1].id.clone(),
                    index: 0,
                },
            )
        }
        35 => (
            "add_ref",
            Op::AddRef {
                target: RefTarget::Shot { shot },
                reference: format!("sources/{token:x}.png"),
                index: Some(0),
            },
        ),
        36 => {
            let reference = format!("sources/{token:x}.png");
            setup(
                &mut project,
                Op::AddRef {
                    target: RefTarget::Shot { shot: shot.clone() },
                    reference: reference.clone(),
                    index: None,
                },
            );
            (
                "remove_ref",
                Op::RemoveRef {
                    target: RefTarget::Shot { shot },
                    reference,
                },
            )
        }
        37 => (
            "resolve_sticky",
            Op::ResolveSticky {
                sticky: first_sticky(&project),
            },
        ),
        38 => (
            "reply_sticky",
            Op::ReplySticky {
                sticky: first_sticky(&project),
                id: Some(fresh("sticky", "reply")),
                text: changed,
                kind: "note".into(),
                author,
                created: "2026-11-07T00:00:02Z".into(),
            },
        ),
        39 => (
            "move_node",
            Op::MoveNode {
                node: format!("shot:{shot}"),
                position: Some(Position {
                    x: (token % 1000) as f64,
                    y: 42.0,
                    extra: Map::new(),
                }),
            },
        ),
        _ => unreachable!("all generated operation cases are enumerated"),
    };
    (case.0, project, case.1)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 18,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn law_inverse(
        generated in operation_project_strategy(),
        agent_authored in any::<bool>(),
        selectors in prop::collection::vec(any::<u64>(), 1..9),
        depth in 1usize..4,
    ) {
        for kind in 0..OP_COUNT {
            let (name, mut project, op) = generated_op_case(
                kind,
                &generated.project,
                generated.token,
                agent_authored,
            );
            let original = project.clone();
            let applied = apply(&mut project, &op)
                .unwrap_or_else(|error| panic!("generated {name} failed: {error:?}"));
            for inverse in applied.inverse {
                apply(&mut project, &inverse)
                    .unwrap_or_else(|error| panic!("inverse for {name} failed: {error:?}"));
            }
            prop_assert_eq!(&project, &original, "{}", name);
        }

        let mut project = generated.project.clone();
        let original = project.clone();
        let mut inverse_groups = Vec::new();
        for (index, selector) in selectors.iter().copied().enumerate() {
            if index % 3 == 1 {
                let before_invalid = project.clone();
                prop_assert!(apply(&mut project, &invalid_sequence_op(index)).is_err());
                prop_assert_eq!(&project, &before_invalid, "invalid op was not atomic");
            }
            let op = nested_batch(
                sequence_op(&project, selector, index, generated.token),
                depth,
            );
            let before = project.clone();
            match apply(&mut project, &op) {
                Ok(applied) => inverse_groups.push(applied.inverse),
                Err(_) => prop_assert_eq!(&project, &before, "invalid op was not atomic"),
            }
        }
        for group in inverse_groups.into_iter().rev() {
            for inverse in group {
                apply(&mut project, &inverse).expect("sequence inverse");
            }
        }
        prop_assert_eq!(project, original, "generated sequence did not invert");
    }

    #[test]
    fn law_validity(
        generated in operation_project_strategy(),
        agent_authored in any::<bool>(),
        selectors in prop::collection::vec(any::<u64>(), 1..9),
        depth in 1usize..4,
    ) {
        for kind in 0..OP_COUNT {
            let (name, mut project, op) = generated_op_case(
                kind,
                &generated.project,
                generated.token,
                agent_authored,
            );
            apply(&mut project, &op)
                .unwrap_or_else(|error| panic!("generated {name} failed: {error:?}"));
            prop_assert!(validate(&project).is_empty(), "{} left an invalid project", name);
        }

        let mut project = generated.project.clone();
        let round_trip: Project = serde_json::from_value(
            serde_json::to_value(&project).expect("serialize generated project")
        ).expect("deserialize generated project");
        prop_assert_eq!(&round_trip, &project, "unknown fields did not round trip");
        for (index, selector) in selectors.iter().copied().enumerate() {
            let op = nested_batch(
                sequence_op(&project, selector, index, generated.token),
                depth,
            );
            let before = project.clone();
            if apply(&mut project, &op).is_err() {
                prop_assert_eq!(&project, &before, "invalid op was not atomic");
            }
            prop_assert!(validate(&project).is_empty(), "sequence step {index} invalid");
            let before_invalid = project.clone();
            prop_assert!(apply(&mut project, &invalid_sequence_op(index)).is_err());
            prop_assert_eq!(&project, &before_invalid);
        }
    }

    #[test]
    fn law_drawable(
        generated in drawable_project_strategy(),
        title in "[A-Za-z0-9 ]{1,20}",
        color in prop::sample::select(vec!["#000000", "#123456", "white", "red"]),
        undecodable in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let mut project = generated.project;
        let shot = first_shot(&project);
        let take = first_take(&project);
        let panel = first_panel(&project);
        apply(&mut project, &Op::UpdatePanel {
            shot,
            take,
            panel,
            patch: patch([("title", json!(title)), ("background", json!(color))]),
        }).expect("generated drawable edit");
        let panel_count: usize = project.scenes.iter()
            .flat_map(|scene| &scene.shots)
            .flat_map(|shot| &shot.takes)
            .map(|take| take.panels.len())
            .sum();
        if panel_count == 1 {
            let extra_shot = first_shot(&project);
            let extra_take = first_take(&project);
            apply(&mut project, &Op::AddPanel {
                shot: extra_shot,
                take: extra_take,
                id: Some(generated_id("panel", generated.token, "drawable-extra")),
                time: Some(1.0),
                index: None,
            }).expect("second drawable panel");
        }
        prop_assert!(validate(&project).is_empty());
        let root = tempfile::tempdir().expect("render root");
        let sources = root.path().join("sources");
        std::fs::create_dir(&sources).expect("drawable sources");
        let random_src = format!("sources/random-{:016x}.png", generated.token);
        let truncated_src = format!("sources/truncated-{:016x}.png", generated.token);
        let mut random_bytes = b"not a png: ".to_vec();
        random_bytes.extend_from_slice(&undecodable);
        std::fs::write(root.path().join(&random_src), random_bytes)
            .expect("random undecodable PNG");
        let complete_png = sources.join(format!("complete-{:016x}.png", generated.token));
        image::RgbaImage::from_pixel(2, 2, image::Rgba([12, 34, 56, 255]))
            .save(&complete_png)
            .expect("real PNG fixture");
        let mut truncated_png = std::fs::read(&complete_png).expect("real PNG bytes");
        truncated_png.truncate(truncated_png.len() / 2);
        std::fs::write(root.path().join(&truncated_src), truncated_png)
            .expect("truncated PNG fixture");

        let target_shot = first_shot(&project);
        let target_take = first_take(&project);
        let target_panel = first_panel(&project);
        let panel = project.scenes.iter_mut()
            .flat_map(|scene| &mut scene.shots)
            .find(|shot| shot.id == target_shot)
            .expect("drawable shot")
            .takes.iter_mut()
            .find(|take| take.id == target_take)
            .expect("drawable take")
            .panels.iter_mut()
            .find(|panel| panel.id == target_panel)
            .expect("drawable panel");
        for (suffix, src) in [
            ("no-src", None),
            ("missing", Some("sources/missing-drawable.png".to_owned())),
            ("random", Some(random_src)),
            ("truncated", Some(truncated_src)),
        ] {
            panel.layers.push(serde_json::from_value(json!({
                "id": generated_id("layer", generated.token, suffix),
                "type": "image",
                "x": 0.0,
                "y": 0.0,
                "width": 24.0,
                "height": 24.0,
                "src": src,
                "fit": "contain"
            })).expect("drawable fault layer"));
        }
        let mut rendered_panels = 0usize;
        for scene in &project.scenes {
            for shot in &scene.shots {
                for take in &shot.takes {
                    for panel in &take.panels {
                        let rendered = render_panel(
                            &project,
                            root.path(),
                            &shot.id,
                            &take.id,
                            &panel.id,
                            64,
                            RenderMode::Stage,
                        );
                        prop_assert!(rendered.is_ok(), "accepted panel failed to render: {rendered:?}");
                        if panel.id == target_panel {
                            let warnings = &rendered.as_ref().expect("checked render").warnings;
                            prop_assert!(
                                warnings.iter().any(|warning| warning.message == "image layer has no source"),
                                "no-src image did not produce a warning: {warnings:?}"
                            );
                            prop_assert!(
                                warnings.iter().filter(|warning| warning.message.contains("could not decode source")).count() >= 3,
                                "missing and undecodable images did not all produce warnings: {warnings:?}"
                            );
                        }
                        rendered_panels += 1;
                    }
                }
            }
        }
        prop_assert!(rendered_panels > 1, "drawable law rendered only one panel");
    }

    #[test]
    fn law_order_independence(
        generated in project_strategy(),
        left in "[A-Za-z0-9 ]{1,20}",
        right in "[A-Za-z0-9 ]{1,20}",
    ) {
        let scene = first_scene(&generated.project);
        let shot = first_shot(&generated.project);
        let first = Op::UpdateScene {
            scene: scene.clone(),
            patch: patch([("title", json!(left))]),
        };
        let second = Op::UpdateShot {
            shot,
            patch: patch([("action", json!(right))]),
        };
        let mut probe = generated.project.clone();
        let left_footprints = apply(&mut probe, &first).expect("left edit").footprints;
        let mut probe = generated.project.clone();
        let right_footprints = apply(&mut probe, &second).expect("right edit").footprints;
        prop_assert!(disjoint(&left_footprints, &right_footprints));
        let overlapping = Op::UpdateScene {
            scene,
            patch: patch([("title", json!(right))]),
        };
        let mut probe = generated.project.clone();
        let overlapping_footprints = apply(&mut probe, &overlapping)
            .expect("overlapping edit")
            .footprints;
        prop_assert!(!disjoint(&left_footprints, &overlapping_footprints));

        let mut left_then_right = generated.project.clone();
        apply(&mut left_then_right, &first).expect("first edit");
        apply(&mut left_then_right, &second).expect("second edit");
        let mut right_then_left = generated.project;
        apply(&mut right_then_left, &second).expect("second edit");
        apply(&mut right_then_left, &first).expect("first edit");
        prop_assert_eq!(left_then_right, right_then_left);
    }

    #[test]
    fn law_determinism(
        generated in project_strategy(),
        title in "[A-Za-z0-9 ]{1,20}",
    ) {
        let mut project = generated.project;
        let shot = first_shot(&project);
        let take = first_take(&project);
        let panel = first_panel(&project);
        apply(&mut project, &Op::UpdateShot {
            shot: shot.clone(),
            patch: patch([("title", json!(title))]),
        }).expect("generated deterministic edit");
        let first_compile = compile_active(&project).expect("first compile");
        let second_compile = compile_active(&project).expect("second compile");
        prop_assert_eq!(first_compile, second_compile);

        let root = tempfile::tempdir().expect("render root");
        let first_render = render_panel(&project, root.path(), &shot, &take, &panel, 64, RenderMode::Stage).expect("first render");
        let second_render = render_panel(&project, root.path(), &shot, &take, &panel, 64, RenderMode::Stage).expect("second render");
        prop_assert_eq!(first_render, second_render);
    }
}

#[test]
fn law_determinism_cycle_worker() {
    if std::env::var_os("NB_LAW_CYCLE_WORKER").is_none() {
        return;
    }
    let mut project = fixture();
    project.cuts[1].clips[0].source = ClipSource::Cut(storyboard_core::CutSource {
        cut: "cut-main".into(),
        extra: Map::new(),
        source_extra: Map::new(),
    });
    assert!(compile_active(&project).is_err(), "cyclic cut compiled");
}

#[test]
fn law_determinism_cyclic_compile_terminates() {
    let executable = std::env::current_exe().expect("current law test executable");
    let mut child = Command::new(executable)
        .arg("law_determinism_cycle_worker")
        .arg("--exact")
        .arg("--nocapture")
        .env("NB_LAW_CYCLE_WORKER", "1")
        .spawn()
        .expect("spawn cyclic compile worker");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().expect("poll cyclic compile worker") {
            assert!(status.success(), "cyclic compile worker failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill non-terminating compile worker");
            let _ = child.wait();
            panic!("cyclic cut compilation did not terminate within two seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn law_round_trip_keeps_sticky_unknown_fields() {
    let mut project = fixture();
    let unknown = json!({"nested": [1, 2, 3]});
    project.stickies[0]
        .extra
        .insert("future_direct_sticky".into(), unknown.clone());

    let wire = serde_json::to_value(&project).expect("serialize project with sticky extra");
    assert_eq!(wire["stickies"][0]["future_direct_sticky"], unknown);
    let rewritten: Project = serde_json::from_value(wire).expect("deserialize sticky extra");
    assert_eq!(rewritten, project);
}
