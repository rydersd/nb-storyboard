#![allow(dead_code)]

use proptest::prelude::*;
use serde_json::{Value, json};
use storyboard_core::{Project, validate};

#[derive(Clone, Debug)]
struct LayerRecipe {
    token: u64,
    kind: u8,
}

#[derive(Clone, Debug)]
struct PanelRecipe {
    token: u64,
    layers: Vec<LayerRecipe>,
}

#[derive(Clone, Debug)]
struct TakeRecipe {
    token: u64,
    panels: Vec<PanelRecipe>,
}

#[derive(Clone, Debug)]
struct ShotRecipe {
    token: u64,
    takes: Vec<TakeRecipe>,
}

#[derive(Clone, Debug)]
struct SceneRecipe {
    token: u64,
    shots: Vec<ShotRecipe>,
}

#[derive(Clone, Debug)]
struct ClipRecipe {
    token: u64,
    source_selector: u64,
    take_selector: u64,
}

#[derive(Clone, Debug)]
struct CutRecipe {
    token: u64,
    clips: Vec<ClipRecipe>,
}

#[derive(Clone, Debug)]
struct ProjectRecipe {
    token: u64,
    label: String,
    scenes: Vec<SceneRecipe>,
    cuts: Vec<CutRecipe>,
}

#[derive(Clone, Debug)]
pub struct GeneratedProject {
    pub project: Project,
    pub token: u64,
}

pub fn project_strategy() -> BoxedStrategy<GeneratedProject> {
    let layer = (any::<u64>(), 0u8..4)
        .prop_map(|(token, kind)| LayerRecipe { token, kind })
        .boxed();
    let panel = (any::<u64>(), prop::collection::vec(layer, 0..5))
        .prop_map(|(token, layers)| PanelRecipe { token, layers })
        .boxed();
    let take = (any::<u64>(), prop::collection::vec(panel, 1..4))
        .prop_map(|(token, panels)| TakeRecipe { token, panels })
        .boxed();
    let shot = (any::<u64>(), prop::collection::vec(take, 1..4))
        .prop_map(|(token, takes)| ShotRecipe { token, takes })
        .boxed();
    let scene = (any::<u64>(), prop::collection::vec(shot, 1..5))
        .prop_map(|(token, shots)| SceneRecipe { token, shots })
        .boxed();
    let clip = (any::<u64>(), any::<u64>(), any::<u64>())
        .prop_map(|(token, source_selector, take_selector)| ClipRecipe {
            token,
            source_selector,
            take_selector,
        })
        .boxed();
    let cut = (any::<u64>(), prop::collection::vec(clip, 1..5))
        .prop_map(|(token, clips)| CutRecipe { token, clips })
        .boxed();

    (
        any::<u64>(),
        "[A-Za-z0-9]{1,12}",
        prop::collection::vec(scene, 1..6),
        prop::collection::vec(cut, 0..4),
    )
        .prop_map(|(token, label, scenes, cuts)| {
            build_project(ProjectRecipe {
                token,
                label,
                scenes,
                cuts,
            })
        })
        .boxed()
}

pub fn drawable_project_strategy() -> BoxedStrategy<GeneratedProject> {
    project_strategy()
        .prop_map(|mut generated| {
            retain_first_panel_context(&mut generated.project);
            generated.project.cuts.clear();
            generated.project.active_cut = None;
            generated.project.stickies.clear();
            generated.project.layout.positions.clear();
            let issues = validate(&generated.project);
            assert!(
                issues.is_empty(),
                "compact drawable project is invalid: {issues:?}"
            );
            generated
        })
        .boxed()
}

pub fn operation_project_strategy() -> BoxedStrategy<GeneratedProject> {
    project_strategy()
        .prop_map(|mut generated| {
            retain_first_panel_context(&mut generated.project);
            generated.project.cuts.clear();
            generated.project.active_cut = None;
            generated.project.stickies.truncate(2);
            generated.project.layout.positions.clear();
            let issues = validate(&generated.project);
            assert!(
                issues.is_empty(),
                "compact operation project is invalid: {issues:?}"
            );
            generated
        })
        .boxed()
}

fn retain_first_panel_context(project: &mut Project) {
    let mut scene = project.scenes.remove(0);
    let mut shot = scene.shots.remove(0);
    let mut take = shot.takes.remove(0);
    take.panels.truncate(1);
    shot.selected_take = take.id.clone();
    shot.takes = vec![take];
    scene.shots = vec![shot];
    project.scenes = vec![scene];
}

fn random_id(kind: &str, token: u64, path: &str) -> String {
    format!("{kind}-{token:016x}-{path}")
}

fn build_project(recipe: ProjectRecipe) -> GeneratedProject {
    let future = json!({"token": recipe.token, "label": recipe.label});
    let mut scenes = Vec::new();
    let mut shot_ids = Vec::new();
    let mut take_ids = Vec::new();
    let mut panel_ids = Vec::new();
    let mut layer_ids = Vec::new();

    for (scene_index, scene_recipe) in recipe.scenes.iter().enumerate() {
        let scene_id = random_id("scene", scene_recipe.token, &scene_index.to_string());
        let mut shots = Vec::new();
        for (shot_index, shot_recipe) in scene_recipe.shots.iter().enumerate() {
            let shot_path = format!("{scene_index}-{shot_index}");
            let shot_id = random_id("shot", shot_recipe.token, &shot_path);
            let mut takes = Vec::new();
            for (take_index, take_recipe) in shot_recipe.takes.iter().enumerate() {
                let take_path = format!("{shot_path}-{take_index}");
                let take_id = random_id("take", take_recipe.token, &take_path);
                let mut panels = Vec::new();
                for (panel_index, panel_recipe) in take_recipe.panels.iter().enumerate() {
                    let panel_path = format!("{take_path}-{panel_index}");
                    let panel_id = random_id("panel", panel_recipe.token, &panel_path);
                    let mut layers = Vec::new();
                    for (layer_index, layer_recipe) in panel_recipe.layers.iter().enumerate() {
                        let layer_path = format!("{panel_path}-{layer_index}");
                        let layer_id = random_id("layer", layer_recipe.token, &layer_path);
                        let common = json!({
                            "id": layer_id,
                            "x": layer_index as f64,
                            "y": panel_index as f64,
                            "width": 32.0,
                            "height": 24.0,
                            "future_layer": future
                        });
                        let mut layer = common.as_object().expect("layer object").clone();
                        match layer_recipe.kind {
                            0 => {
                                layer.insert("type".into(), json!("image"));
                                layer.insert(
                                    "src".into(),
                                    json!(format!("sources/missing-{layer_index}.png")),
                                );
                                layer.insert("fit".into(), json!("contain"));
                            }
                            1 => {
                                layer.insert("type".into(), json!("text"));
                                layer
                                    .insert("text".into(), json!(format!("Text {}", recipe.label)));
                                layer.insert("font_size".into(), json!(18.0));
                                layer.insert("weight".into(), json!("bold"));
                                layer.insert("align".into(), json!("left"));
                                layer.insert("color".into(), json!("white"));
                            }
                            2 => {
                                layer.insert("type".into(), json!("solid"));
                                layer.insert("fill".into(), json!("#123456"));
                            }
                            _ => {
                                layer.insert("type".into(), json!("box"));
                            }
                        }
                        layers.push(Value::Object(layer));
                        layer_ids.push((
                            shot_id.clone(),
                            take_id.clone(),
                            panel_id.clone(),
                            layer_id,
                        ));
                    }
                    panels.push(json!({
                        "id": panel_id,
                        "time": panel_index as f64,
                        "title": format!("Panel {panel_index}"),
                        "background": "#000000",
                        "layers": layers,
                        "strokes": [{"color":"white","width":1.0,"points":[[0.0,0.0],[1.0,1.0]],"future_stroke":future}],
                        "future_panel": future
                    }));
                    panel_ids.push((shot_id.clone(), take_id.clone(), panel_id));
                }
                takes.push(json!({
                    "id": take_id,
                    "name": format!("Take {take_index}"),
                    "panels": panels,
                    "future_take": future
                }));
                take_ids.push((shot_id.clone(), take_id));
            }
            let selected_take = takes[0]["id"].as_str().expect("take id").to_owned();
            shots.push(json!({
                "id": shot_id,
                "title": format!("Shot {shot_index} {}", recipe.label),
                "action": format!("Action {}", recipe.label),
                "duration": 4.0,
                "selected_take": selected_take,
                "takes": takes,
                "future_shot": future
            }));
            shot_ids.push((scene_id.clone(), shot_id));
        }
        scenes.push(json!({
            "id": scene_id,
            "title": format!("Scene {scene_index} {}", recipe.label),
            "shots": shots,
            "future_scene": future
        }));
    }

    let mut cuts = Vec::new();
    let mut cut_ids = Vec::new();
    let mut clip_ids = Vec::new();
    for (cut_index, cut_recipe) in recipe.cuts.iter().enumerate() {
        let cut_id = random_id("cut", cut_recipe.token, &cut_index.to_string());
        let mut clips = Vec::new();
        for (clip_index, clip_recipe) in cut_recipe.clips.iter().enumerate() {
            let clip_id = random_id(
                "clip",
                clip_recipe.token,
                &format!("{cut_index}-{clip_index}"),
            );
            let source = if cut_index > 0 && clip_recipe.source_selector % 3 == 0 {
                let source_cut = &cut_ids[clip_recipe.source_selector as usize % cut_index];
                json!({"cut":{"cut":source_cut,"future_cut_source":future},"future_clip_source":future})
            } else {
                let (_, shot_id) = &shot_ids[clip_recipe.source_selector as usize % shot_ids.len()];
                let matching_takes: Vec<_> = take_ids
                    .iter()
                    .filter(|(shot, _)| shot == shot_id)
                    .collect();
                let take_id =
                    &matching_takes[clip_recipe.take_selector as usize % matching_takes.len()].1;
                json!({"shot":{"shot":shot_id,"take":take_id,"future_shot_source":future},"future_clip_source":future})
            };
            clips.push(json!({
                "id": clip_id,
                "source": source,
                "future_clip": future
            }));
            clip_ids.push((cut_id.clone(), clip_id));
        }
        cuts.push(json!({
            "id": cut_id,
            "name": format!("Cut {cut_index} {}", recipe.label),
            "clips": clips,
            "future_cut": future
        }));
        cut_ids.push(cut_id);
    }

    let mut pins = vec!["none".to_owned()];
    let (scene_id, shot_id) = &shot_ids[0];
    pins.push(format!("scene:{scene_id}"));
    pins.push(format!("shot:{shot_id}"));
    let (_, take_id) = &take_ids[0];
    pins.push(format!("take:{shot_id}/{take_id}"));
    let (_, _, panel_id) = &panel_ids[0];
    pins.push(format!("panel:{shot_id}/{take_id}/{panel_id}"));
    if let Some((layer_shot, layer_take, layer_panel, layer_id)) = layer_ids.first() {
        pins.push(format!(
            "layer:{layer_shot}/{layer_take}/{layer_panel}/{layer_id}"
        ));
    }
    if let Some((cut_id, clip_id)) = clip_ids.first() {
        pins.push(format!("clip:{cut_id}/{clip_id}"));
    }
    let mut stickies = Vec::new();
    for (index, pin) in pins.iter().enumerate() {
        let sticky_id = random_id("sticky", recipe.token, &format!("{index}-pin"));
        let reply_id = random_id("sticky", recipe.token, &format!("{index}-reply"));
        stickies.push(json!({
            "id": sticky_id,
            "text": format!("Pinned {index}"),
            "kind": "note",
            "author": "human",
            "pin": pin,
            "created": "2026-11-07T00:00:00Z",
            "future_sticky": future
        }));
        stickies.push(json!({
            "id": reply_id,
            "text": format!("Reply {index}"),
            "kind": "question",
            "author": {"agent":{"name":format!("agent-{:x}", recipe.token),"future_agent":future},"future_author":future},
            "pin": "none",
            "reply_to": sticky_id,
            "created": "2026-11-07T00:00:01Z",
            "future_sticky": future
        }));
    }

    let active_cut = cut_ids.last().cloned();
    let document = json!({
        "schema": "nb-storyboard/v1",
        "title": format!("Generated {}", recipe.label),
        "fps": 24,
        "format": {"width":320,"height":180,"future_format":future},
        "scenes": scenes,
        "cuts": cuts,
        "active_cut": active_cut,
        "stickies": stickies,
        "layout": {"positions":{},"future_layout":future},
        "future_project": future
    });
    let project: Project = serde_json::from_value(document).expect("generated project decodes");
    let issues = validate(&project);
    assert!(
        issues.is_empty(),
        "generated project is invalid: {issues:?}"
    );
    GeneratedProject {
        project,
        token: recipe.token,
    }
}

pub fn all_scene_ids(project: &Project) -> Vec<String> {
    project
        .scenes
        .iter()
        .map(|scene| scene.id.clone())
        .collect()
}

pub fn all_shot_ids(project: &Project) -> Vec<String> {
    project
        .scenes
        .iter()
        .flat_map(|scene| scene.shots.iter().map(|shot| shot.id.clone()))
        .collect()
}

pub fn choose_existing(ids: &[String], selector: u64) -> String {
    ids[selector as usize % ids.len()].clone()
}

pub fn choose_target(ids: &[String], selector: u64, kind: &str) -> String {
    if selector.is_multiple_of(17) {
        format!("missing-{kind}-{selector:x}")
    } else {
        choose_existing(ids, selector / 17)
    }
}
