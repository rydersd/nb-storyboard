//! The small, validated edits accepted by a storyboard project.

use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    fmt,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    Author, Clip, ClipSource, Cut, ExtraFields, Layer, Panel, Pin, Position, Project, Scene, Shot,
    Sticky, Take, ValidationIssue, footprint::sorted, validate,
};

pub type Patch = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Op {
    Batch {
        ops: Vec<Op>,
    },
    AddScene {
        id: Option<String>,
        title: String,
        index: Option<usize>,
    },
    RemoveScene {
        scene: String,
    },
    MoveScene {
        scene: String,
        index: usize,
    },
    UpdateScene {
        scene: String,
        patch: Patch,
    },
    AddShot {
        scene: String,
        id: Option<String>,
        title: String,
        index: Option<usize>,
    },
    RemoveShot {
        shot: String,
    },
    MoveShot {
        shot: String,
        index: usize,
    },
    UpdateShot {
        shot: String,
        patch: Patch,
    },
    AddTake {
        shot: String,
        id: Option<String>,
        name: String,
        index: Option<usize>,
    },
    RemoveTake {
        shot: String,
        take: String,
    },
    MoveTake {
        shot: String,
        take: String,
        index: usize,
    },
    UpdateTake {
        shot: String,
        take: String,
        patch: Patch,
    },
    SelectTake {
        shot: String,
        take: String,
    },
    DuplicateTake {
        shot: String,
        take: String,
        id: Option<String>,
        index: Option<usize>,
    },
    AddPanel {
        shot: String,
        take: String,
        id: Option<String>,
        time: Option<f64>,
        index: Option<usize>,
    },
    RemovePanel {
        shot: String,
        take: String,
        panel: String,
    },
    MovePanel {
        shot: String,
        take: String,
        panel: String,
        index: usize,
    },
    UpdatePanel {
        shot: String,
        take: String,
        panel: String,
        patch: Patch,
    },
    AddLayer {
        shot: String,
        take: String,
        panel: String,
        id: Option<String>,
        layer_type: String,
        fields: Patch,
        index: Option<usize>,
    },
    RemoveLayer {
        shot: String,
        take: String,
        panel: String,
        layer: String,
    },
    MoveLayer {
        shot: String,
        take: String,
        panel: String,
        layer: String,
        index: usize,
    },
    UpdateLayer {
        shot: String,
        take: String,
        panel: String,
        layer: String,
        patch: Patch,
    },
    AddSticky {
        id: Option<String>,
        text: String,
        kind: String,
        author: Author,
        pin: Pin,
        created: String,
    },
    RemoveSticky {
        sticky: String,
    },
    MoveSticky {
        sticky: String,
        index: usize,
    },
    UpdateSticky {
        sticky: String,
        patch: Patch,
    },
    AddClip {
        cut: String,
        id: Option<String>,
        source: ClipSource,
        index: Option<usize>,
    },
    RemoveClip {
        cut: String,
        clip: String,
    },
    ReorderClip {
        cut: String,
        clip: String,
        index: usize,
    },
    UpdateClip {
        cut: String,
        clip: String,
        patch: Patch,
    },
    AddCut {
        id: Option<String>,
        name: String,
        index: Option<usize>,
    },
    UpdateCut {
        cut: String,
        patch: Patch,
    },
    RemoveCut {
        cut: String,
    },
    SetActiveCut {
        cut: Option<String>,
    },
    MoveShotBetweenScenes {
        shot: String,
        scene: String,
        index: usize,
    },
    AddRef {
        target: RefTarget,
        reference: String,
        index: Option<usize>,
    },
    RemoveRef {
        target: RefTarget,
        reference: String,
    },
    ResolveSticky {
        sticky: String,
    },
    ReplySticky {
        sticky: String,
        id: Option<String>,
        text: String,
        kind: String,
        author: Author,
        created: String,
    },
    MoveNode {
        node: String,
        position: Option<Position>,
    },
    /// A compact, path-scoped inverse. It is deliberately not a project snapshot.
    #[cfg_attr(feature = "schema", schemars(skip))]
    RestoreChanges {
        changes: Vec<RestoreChange>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RestoreChange {
    pub action: RestoreAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RestoreAction {
    Delete {
        target: RestoreTarget,
    },
    Insert {
        parent: RestoreParent,
        index: usize,
        value: Value,
    },
    Move {
        target: RestoreTarget,
        parent: RestoreParent,
        index: usize,
    },
    SetField {
        target: RestoreTarget,
        field: String,
        value: Option<Value>,
    },
    InsertRef {
        target: RefTarget,
        index: usize,
        reference: String,
    },
    DeleteRef {
        target: RefTarget,
        index: usize,
        reference: String,
    },
    MoveRef {
        target: RefTarget,
        from: usize,
        index: usize,
        reference: String,
    },
    SetLayout {
        node: String,
        position: Option<Position>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RestoreTarget {
    Project,
    Scene {
        scene: String,
    },
    Shot {
        shot: String,
    },
    Take {
        shot: String,
        take: String,
    },
    Panel {
        shot: String,
        take: String,
        panel: String,
    },
    Layer {
        shot: String,
        take: String,
        panel: String,
        layer: String,
    },
    Sticky {
        sticky: String,
    },
    Cut {
        cut: String,
    },
    Clip {
        cut: String,
        clip: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RestoreParent {
    Scenes,
    Shots {
        scene: String,
    },
    Takes {
        shot: String,
    },
    Panels {
        shot: String,
        take: String,
    },
    Layers {
        shot: String,
        take: String,
        panel: String,
    },
    Stickies,
    Cuts,
    Clips {
        cut: String,
    },
}

impl Op {
    pub fn is_internal(&self) -> bool {
        match self {
            Self::Batch { ops } => ops.iter().any(Self::is_internal),
            Self::RestoreChanges { .. } => true,
            _ => false,
        }
    }

    pub fn check_public(&self) -> Result<(), OpError> {
        if self.is_internal() {
            Err(OpError::at("op", "internal operations cannot be submitted"))
        } else {
            Ok(())
        }
    }

    /// The authors a write proposes to store. Task 6 uses this before committing a write.
    pub fn sticky_authors(&self) -> Vec<&Author> {
        match self {
            Self::AddSticky { author, .. } | Self::ReplySticky { author, .. } => vec![author],
            Self::Batch { ops } => ops.iter().flat_map(Self::sticky_authors).collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RefTarget {
    Shot { shot: String },
    Take { shot: String, take: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub inverse: Vec<Op>,
    pub footprints: Vec<String>,
    pub unpinned_stickies: Vec<String>,
    pub created_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpError {
    pub field: String,
    pub message: String,
    pub valid: Vec<String>,
    pub issues: Vec<OpIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpIssue {
    pub field: String,
    pub message: String,
    pub valid: Vec<String>,
}

impl OpError {
    fn at(field: impl Into<String>, message: impl Into<String>) -> Self {
        let field = field.into();
        let message = message.into();
        Self {
            field: field.clone(),
            message: message.clone(),
            valid: Vec::new(),
            issues: vec![OpIssue {
                field,
                message,
                valid: Vec::new(),
            }],
        }
    }
    fn options(
        field: impl Into<String>,
        message: impl Into<String>,
        valid: impl IntoIterator<Item = String>,
    ) -> Self {
        let field = field.into();
        let mut valid: Vec<_> = valid.into_iter().collect();
        valid.sort();
        valid.dedup();
        let message = message.into();
        Self {
            field: field.clone(),
            message: message.clone(),
            valid: valid.clone(),
            issues: vec![OpIssue {
                field,
                message,
                valid,
            }],
        }
    }
    fn validation(issues: &[ValidationIssue]) -> Self {
        let issues: Vec<_> = issues
            .iter()
            .map(|issue| OpIssue {
                field: issue.field.clone(),
                message: issue.message.clone(),
                valid: issue.valid.clone(),
            })
            .collect();
        let first = issues.first().expect("validation errors are not empty");
        Self {
            field: first.field.clone(),
            message: first.message.clone(),
            valid: first.valid.clone(),
            issues,
        }
    }
}

impl fmt::Display for OpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl Error for OpError {}

/// Applies one operation atomically. Any invalid result is discarded before the caller sees it.
pub fn apply(project: &mut Project, op: &Op) -> Result<Applied, OpError> {
    let before = project.clone();
    let mut next = project.clone();
    let mut effect = Effect::default();
    execute(&mut next, op, &mut effect)?;
    if next == before {
        return Err(OpError::at("op", "operation made no change"));
    }
    let issues = validate(&next);
    if !issues.is_empty() {
        return Err(OpError::validation(&issues));
    }
    *project = next;
    Ok(Applied {
        inverse: inverse_ops(&before, project, op)?,
        footprints: sorted(effect.footprints),
        unpinned_stickies: sorted(effect.unpinned),
        created_ids: sorted(effect.created),
    })
}

fn inverse_ops(before: &Project, after: &Project, op: &Op) -> Result<Vec<Op>, OpError> {
    let Op::Batch { ops } = op else {
        return Ok(vec![Op::RestoreChanges {
            changes: project_changes(before, after)?,
        }]);
    };

    let mut cursor = before.clone();
    let mut groups = Vec::with_capacity(ops.len());
    for child in ops {
        let child_before = cursor.clone();
        execute(&mut cursor, child, &mut Effect::default())?;
        groups.push(inverse_ops(&child_before, &cursor, child)?);
    }
    debug_assert_eq!(&cursor, after);
    Ok(groups.into_iter().rev().flatten().collect())
}

#[derive(Default)]
struct Effect {
    footprints: Vec<String>,
    unpinned: Vec<String>,
    created: Vec<String>,
}
impl Effect {
    fn touch(&mut self, value: impl Into<String>) {
        self.footprints.push(canonical_footprint(&value.into()));
    }
    fn unpinned(&mut self, values: Vec<String>) {
        for sticky in values {
            self.touch(format!("sticky/{sticky}"));
            self.unpinned.push(sticky);
        }
    }
    fn created(&mut self, id: impl Into<String>) {
        self.created.push(id.into());
    }
}

fn canonical_footprint(value: &str) -> String {
    let parts: Vec<_> = value.split('/').collect();
    match parts.as_slice() {
        ["take", shot, take, tail @ ..] => {
            let tail = match tail {
                ["panels", rest @ ..] => std::iter::once("panel")
                    .chain(rest.iter().copied())
                    .collect::<Vec<_>>(),
                _ => tail.to_vec(),
            };
            std::iter::once("shot")
                .chain([*shot, "take", *take])
                .chain(tail)
                .collect::<Vec<_>>()
                .join("/")
        }
        ["panel", shot, take, panel, tail @ ..] => {
            let tail = match tail {
                ["layers", rest @ ..] => std::iter::once("layer")
                    .chain(rest.iter().copied())
                    .collect::<Vec<_>>(),
                _ => tail.to_vec(),
            };
            std::iter::once("shot")
                .chain([*shot, "take", *take, "panel", *panel])
                .chain(tail)
                .collect::<Vec<_>>()
                .join("/")
        }
        ["layer", shot, take, panel, layer, tail @ ..] => std::iter::once("shot")
            .chain([*shot, "take", *take, "panel", *panel, "layer", *layer])
            .chain(tail.iter().copied())
            .collect::<Vec<_>>()
            .join("/"),
        ["clip", cut, clip, tail @ ..] => std::iter::once("cut")
            .chain([*cut, "clip", *clip])
            .chain(tail.iter().copied())
            .collect::<Vec<_>>()
            .join("/"),
        ["shot", shot, "takes", rest @ ..] => std::iter::once("shot")
            .chain([*shot, "take"])
            .chain(rest.iter().copied())
            .collect::<Vec<_>>()
            .join("/"),
        _ => value.to_owned(),
    }
}

fn node_exists(project: &Project, node: &str) -> bool {
    let Some((kind, id)) = node.split_once(':') else {
        return false;
    };
    match kind {
        "shot" => project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .any(|shot| shot.id == id),
        "cut" => project.cuts.iter().any(|cut| cut.id == id),
        "sticky" => project.stickies.iter().any(|sticky| sticky.id == id),
        _ => false,
    }
}

fn node_footprint(node: &str) -> String {
    let (kind, id) = node.split_once(':').expect("validated typed node");
    format!("{kind}/{id}")
}

fn valid_nodes(project: &Project) -> Vec<String> {
    project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .map(|shot| format!("shot:{}", shot.id))
        .chain(project.cuts.iter().map(|cut| format!("cut:{}", cut.id)))
        .chain(
            project
                .stickies
                .iter()
                .map(|sticky| format!("sticky:{}", sticky.id)),
        )
        .collect()
}

fn pin_footprint(pin: &Pin) -> String {
    match pin {
        Pin::None => "sticky".to_owned(),
        Pin::Scene(scene) => format!("scene/{scene}"),
        Pin::Shot(shot) => format!("shot/{shot}"),
        Pin::Take { shot, take } => format!("shot/{shot}/take/{take}"),
        Pin::Panel { shot, take, panel } => format!("shot/{shot}/take/{take}/panel/{panel}"),
        Pin::Layer {
            shot,
            take,
            panel,
            layer,
        } => {
            format!("shot/{shot}/take/{take}/panel/{panel}/layer/{layer}")
        }
        Pin::Clip { cut, clip } => format!("cut/{cut}/clip/{clip}"),
    }
}

fn touch_clip_source(effect: &mut Effect, source: &ClipSource) {
    match source {
        ClipSource::Shot(source) => {
            effect.touch(format!("shot/{}", source.shot));
            if let Some(take) = &source.take {
                effect.touch(format!("shot/{}/take/{take}", source.shot));
            }
        }
        ClipSource::Cut(source) => {
            effect.touch(format!("cut/{}", source.cut));
            effect.touch("cuts/graph");
        }
    }
}

fn execute(project: &mut Project, op: &Op, effect: &mut Effect) -> Result<(), OpError> {
    match op {
        Op::Batch { ops } => {
            for op in ops {
                execute(project, op, effect)?;
            }
        }
        Op::RestoreChanges { changes } => restore_changes(project, changes)?,
        Op::AddScene { id, title, index } => {
            let id = next_id(project, "scene", id.as_deref())?;
            let destination = index.unwrap_or(project.scenes.len());
            insert(
                &mut project.scenes,
                destination,
                Scene {
                    id: id.clone(),
                    title: title.clone(),
                    shots: Vec::new(),
                    extra: ExtraFields::new(),
                },
                "index",
            )?;
            effect.touch(format!("scene/{id}"));
            effect.touch("scene/order");
            effect.touch("ids/scene");
            effect.created(id);
        }
        Op::RemoveScene { scene } => {
            let index = scene_index(project, scene)?;
            let contained: HashSet<_> = project.scenes[index]
                .shots
                .iter()
                .map(|shot| shot.id.clone())
                .collect();
            refuse_shots_in_cuts(project, &contained)?;
            let removed = project.scenes.remove(index);
            effect.touch(format!("scene/{}", removed.id));
            effect.touch("scene/order");
            effect.touch("ids/scene");
            for shot in &removed.shots {
                effect.touch(format!("shot/{}", shot.id));
                effect.touch("ids/shot");
                let key = format!("shot:{}", shot.id);
                project.layout.positions.remove(&key);
                effect.touch(format!("layout/{key}"));
                for take in &shot.takes {
                    effect.touch("ids/take");
                    for panel in &take.panels {
                        effect.touch("ids/panel");
                        if !panel.layers.is_empty() {
                            effect.touch("ids/layer");
                        }
                    }
                }
            }
            effect.unpinned(unpin(project, |pin| pin_scene_or_shot(pin, &removed)));
        }
        Op::MoveScene { scene, index } => {
            move_item(
                &mut project.scenes,
                scene,
                *index,
                |scene| &scene.id,
                "index",
            )?;
            effect.touch("scene/order");
        }
        Op::UpdateScene { scene, patch } => {
            patch_value(scene_mut(project, scene)?, patch, &["title"])?;
            for key in patch.keys() {
                effect.touch(format!("scene/{scene}/field/{key}"));
            }
        }
        Op::AddShot {
            scene,
            id,
            title,
            index,
        } => {
            let shot_id = next_id(project, "shot", id.as_deref())?;
            let take_id = next_id(project, "take", None)?;
            let panel_id = next_id(project, "panel", None)?;
            let shot = Shot {
                id: shot_id.clone(),
                title: title.clone(),
                action: String::new(),
                dialogue: String::new(),
                speaker: String::new(),
                camera: String::new(),
                notes: String::new(),
                duration: 2.0,
                status: "draft".into(),
                selected_take: take_id.clone(),
                takes: vec![Take {
                    id: take_id.clone(),
                    name: "Take 1".into(),
                    panels: vec![blank_panel(panel_id.clone())],
                    refs: Vec::new(),
                    extra: ExtraFields::new(),
                }],
                refs: Vec::new(),
                extra: ExtraFields::new(),
            };
            let scene = scene_mut(project, scene)?;
            let destination = index.unwrap_or(scene.shots.len());
            insert(&mut scene.shots, destination, shot, "index")?;
            effect.touch(format!("shot/{shot_id}"));
            effect.touch(format!("scene/{}/order", scene.id));
            effect.touch("ids/shot");
            effect.touch("ids/take");
            effect.touch("ids/panel");
            effect.created(shot_id);
            effect.created(take_id);
            effect.created(panel_id);
        }
        Op::RemoveShot { shot } => {
            refuse_shots_in_cuts(project, &HashSet::from([shot.clone()]))?;
            let (scene, index) = shot_location(project, shot)?;
            let removed = project.scenes[scene].shots.remove(index);
            effect.touch(format!("shot/{}", removed.id));
            effect.touch(format!("scene/{}/order", project.scenes[scene].id));
            effect.touch("ids/shot");
            for take in &removed.takes {
                effect.touch("ids/take");
                for panel in &take.panels {
                    effect.touch("ids/panel");
                    if !panel.layers.is_empty() {
                        effect.touch("ids/layer");
                    }
                }
            }
            let layout = format!("shot:{}", removed.id);
            effect.touch(format!("layout/{layout}"));
            project.layout.positions.remove(&layout);
            effect.unpinned(unpin(project, |pin| pin_shot(pin, &removed)));
        }
        Op::MoveShot { shot, index } => {
            let (scene, _) = shot_location(project, shot)?;
            let scene_id = project.scenes[scene].id.clone();
            move_item(
                &mut project.scenes[scene].shots,
                shot,
                *index,
                |shot| &shot.id,
                "index",
            )?;
            effect.touch(format!("scene/{scene_id}/order"));
        }
        Op::UpdateShot { shot, patch } => {
            patch_value(
                shot_mut(project, shot)?,
                patch,
                &[
                    "title", "action", "dialogue", "speaker", "camera", "notes", "duration",
                    "status",
                ],
            )?;
            for key in patch.keys() {
                if key == "duration" {
                    effect.touch(format!("shot/{shot}/timing"));
                } else {
                    effect.touch(format!("shot/{shot}/field/{key}"));
                }
            }
        }
        Op::AddTake {
            shot,
            id,
            name,
            index,
        } => {
            let id = next_id(project, "take", id.as_deref())?;
            let shot = shot_mut(project, shot)?;
            let destination = index.unwrap_or(shot.takes.len());
            insert(
                &mut shot.takes,
                destination,
                Take {
                    id: id.clone(),
                    name: name.clone(),
                    panels: Vec::new(),
                    refs: Vec::new(),
                    extra: ExtraFields::new(),
                },
                "index",
            )?;
            effect.touch(format!("shot/{}/take/{id}", shot.id));
            effect.touch(format!("shot/{}/order", shot.id));
            effect.touch(format!("shot/{}", shot.id));
            effect.touch("ids/take");
            effect.created(id);
        }
        Op::RemoveTake { shot, take } => {
            if project.cuts.iter().any(|cut| cut.clips.iter().any(|clip| matches!(&clip.source, ClipSource::Shot(source) if source.shot == *shot && source.take.as_deref() == Some(take)))) { return Err(OpError::at(format!("shot.{shot}.take.{take}"), "cannot remove a take explicitly used by a clip")); }
            let (removed, reselected) = {
                let candidate = shot_mut(project, shot)?;
                if candidate.takes.len() == 1 {
                    return Err(OpError::at(
                        format!("shot.{shot}.takes"),
                        "cannot remove a shot's last take",
                    ));
                }
                let index = by_id(&candidate.takes, take, |take| &take.id, "take")?;
                let removed = candidate.takes.remove(index);
                let reselected = candidate.selected_take == removed.id;
                if reselected {
                    candidate.selected_take = candidate.takes[0].id.clone();
                }
                (removed, reselected)
            };
            if reselected {
                effect.touch(format!("shot/{shot}/field/selected_take"));
            }
            effect.touch(format!("shot/{shot}/take/{}", removed.id));
            effect.touch(format!("shot/{shot}/order"));
            effect.touch("ids/take");
            for panel in &removed.panels {
                effect.touch("ids/panel");
                if !panel.layers.is_empty() {
                    effect.touch("ids/layer");
                }
            }
            effect.unpinned(unpin(project, |pin| pin_take(pin, shot, &removed)));
        }
        Op::MoveTake { shot, take, index } => {
            let shot = shot_mut(project, shot)?;
            move_item(&mut shot.takes, take, *index, |take| &take.id, "index")?;
            effect.touch(format!("shot/{}/order", shot.id));
        }
        Op::UpdateTake { shot, take, patch } => {
            patch_value(take_mut(project, shot, take)?, patch, &["name"])?;
            for key in patch.keys() {
                effect.touch(format!("shot/{shot}/take/{take}/field/{key}"));
            }
        }
        Op::SelectTake { shot, take } => {
            let shot_obj = shot_mut(project, shot)?;
            if !shot_obj.takes.iter().any(|item| item.id == *take) {
                return Err(options_for(
                    "take",
                    "unknown take",
                    shot_obj.takes.iter().map(|take| take.id.clone()),
                ));
            }
            shot_obj.selected_take = take.clone();
            effect.touch(format!("shot/{shot}/take/{take}"));
            effect.touch(format!("shot/{shot}/field/selected_take"));
        }
        Op::DuplicateTake {
            shot,
            take,
            id,
            index,
        } => {
            let id = next_id(project, "take", id.as_deref())?;
            effect.touch(format!("shot/{shot}/take/{take}"));
            let mut duplicate = {
                let shot_obj = shot_mut(project, shot)?;
                shot_obj.takes[by_id(&shot_obj.takes, take, |take| &take.id, "take")?].clone()
            };
            duplicate.id = id.clone();
            let nested_ids = freshen_take(project, &mut duplicate);
            let shot_obj = shot_mut(project, shot)?;
            let destination = index.unwrap_or(shot_obj.takes.len());
            insert(&mut shot_obj.takes, destination, duplicate, "index")?;
            effect.touch(format!("shot/{shot}/take/{id}"));
            effect.touch(format!("shot/{shot}/order"));
            effect.touch("ids/take");
            effect.created(id);
            for (kind, nested_id) in nested_ids {
                effect.touch(format!("ids/{kind}"));
                effect.created(nested_id);
            }
        }
        Op::AddPanel {
            shot,
            take,
            id,
            time,
            index,
        } => {
            let id = next_id(project, "panel", id.as_deref())?;
            let shot_duration = shot_mut(project, shot)?.duration;
            let fps = project.fps;
            let take = take_mut(project, shot, take)?;
            let time = match time {
                Some(time) => *time,
                None if take.panels.is_empty() => 0.0,
                None => {
                    let candidate = take.panels.last().expect("non-empty").time + 1.0 / fps as f64;
                    if candidate >= shot_duration {
                        return Err(OpError::at(
                            format!("shot.{shot}.duration"),
                            format!(
                                "no panel time remains within the shot duration {shot_duration}"
                            ),
                        ));
                    }
                    candidate
                }
            };
            let destination = index.unwrap_or(take.panels.len());
            insert(
                &mut take.panels,
                destination,
                Panel {
                    id: id.clone(),
                    time,
                    title: String::new(),
                    direction: String::new(),
                    background: String::new(),
                    layers: Vec::new(),
                    strokes: Vec::new(),
                    extra: ExtraFields::new(),
                },
                "index",
            )?;
            effect.touch(format!("shot/{shot}/take/{}/panel/{id}", take.id));
            effect.touch(format!("shot/{shot}/take/{}/order", take.id));
            effect.touch(format!("shot/{shot}/timing"));
            effect.touch(format!("shot/{shot}/take/{}", take.id));
            effect.touch("ids/panel");
            effect.created(id);
        }
        Op::RemovePanel { shot, take, panel } => {
            let removed = {
                let take_obj = take_mut(project, shot, take)?;
                take_obj
                    .panels
                    .remove(by_id(&take_obj.panels, panel, |panel| &panel.id, "panel")?)
            };
            effect.touch(format!("shot/{shot}/take/{take}/panel/{}", removed.id));
            effect.touch(format!("shot/{shot}/take/{take}/order"));
            effect.touch(format!("shot/{shot}/timing"));
            effect.touch("ids/panel");
            if !removed.layers.is_empty() {
                effect.touch("ids/layer");
            }
            effect.unpinned(unpin(project, |pin| pin_panel(pin, shot, take, &removed)));
        }
        Op::MovePanel {
            shot,
            take,
            panel,
            index,
        } => {
            let take_obj = take_mut(project, shot, take)?;
            move_item(
                &mut take_obj.panels,
                panel,
                *index,
                |panel| &panel.id,
                "index",
            )?;
            effect.touch(format!("shot/{shot}/take/{take}/order"));
        }
        Op::UpdatePanel {
            shot,
            take,
            panel,
            patch,
        } => {
            patch_value(
                panel_mut(project, shot, take, panel)?,
                patch,
                &["time", "title", "direction", "background"],
            )?;
            for key in patch.keys() {
                if key == "time" {
                    effect.touch(format!("shot/{shot}/take/{take}/panel/{panel}"));
                    effect.touch(format!("shot/{shot}/timing"));
                } else {
                    effect.touch(format!("shot/{shot}/take/{take}/panel/{panel}/field/{key}"));
                }
            }
        }
        Op::AddLayer {
            shot,
            take,
            panel,
            id,
            layer_type,
            fields,
            index,
        } => {
            let valid_types = ["image", "text", "solid", "box"];
            if !valid_types.contains(&layer_type.as_str()) {
                return Err(OpError::options(
                    "layer_type",
                    "invalid layer type",
                    valid_types.into_iter().map(str::to_owned),
                ));
            }
            if layer_type == "image"
                && fields
                    .get("src")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err(OpError::at("fields.src", "image layers require src"));
            }
            if layer_type == "text" && fields.get("text").and_then(Value::as_str).is_none() {
                return Err(OpError::at("fields.text", "text layers require text"));
            }
            let id = next_id(project, "layer", id.as_deref())?;
            let (width, height) = (project.format.width as f64, project.format.height as f64);
            let panel = panel_mut(project, shot, take, panel)?;
            let destination = index.unwrap_or(panel.layers.len());
            let mut layer = Layer {
                id: id.clone(),
                layer_type: layer_type.clone(),
                x: 0.0,
                y: 0.0,
                width,
                height,
                opacity: 1.0,
                hidden: false,
                label: String::new(),
                src: None,
                fit: None,
                crop: None,
                text: None,
                font_size: None,
                weight: None,
                align: None,
                color: (layer_type == "text").then(|| "#000000".to_owned()),
                fill: (layer_type == "solid").then(|| "#000000".to_owned()),
                extra: ExtraFields::new(),
            };
            patch_value(
                &mut layer,
                fields,
                &[
                    "x",
                    "y",
                    "width",
                    "height",
                    "opacity",
                    "hidden",
                    "label",
                    "src",
                    "fit",
                    "crop",
                    "text",
                    "font_size",
                    "weight",
                    "align",
                    "color",
                    "fill",
                ],
            )?;
            insert(&mut panel.layers, destination, layer, "index")?;
            effect.touch(format!(
                "shot/{shot}/take/{take}/panel/{}/layer/{id}",
                panel.id
            ));
            effect.touch(format!("shot/{shot}/take/{take}/panel/{}/order", panel.id));
            effect.touch(format!("shot/{shot}/take/{take}/panel/{}", panel.id));
            effect.touch("ids/layer");
            effect.created(id);
        }
        Op::RemoveLayer {
            shot,
            take,
            panel,
            layer,
        } => {
            let removed = {
                let panel_obj = panel_mut(project, shot, take, panel)?;
                panel_obj.layers.remove(by_id(
                    &panel_obj.layers,
                    layer,
                    |layer| &layer.id,
                    "layer",
                )?)
            };
            effect.touch(format!(
                "shot/{shot}/take/{take}/panel/{panel}/layer/{}",
                removed.id
            ));
            effect.touch(format!("shot/{shot}/take/{take}/panel/{panel}/order"));
            effect.touch("ids/layer");
            effect.unpinned(unpin(project, |pin| {
                pin_layer(pin, shot, take, panel, &removed)
            }));
        }
        Op::MoveLayer {
            shot,
            take,
            panel,
            layer,
            index,
        } => {
            let panel_obj = panel_mut(project, shot, take, panel)?;
            move_item(
                &mut panel_obj.layers,
                layer,
                *index,
                |layer| &layer.id,
                "index",
            )?;
            effect.touch(format!("shot/{shot}/take/{take}/panel/{panel}/order"));
        }
        Op::UpdateLayer {
            shot,
            take,
            panel,
            layer,
            patch,
        } => {
            patch_value(
                layer_mut(project, shot, take, panel, layer)?,
                patch,
                &[
                    "type",
                    "x",
                    "y",
                    "width",
                    "height",
                    "opacity",
                    "hidden",
                    "label",
                    "src",
                    "fit",
                    "crop",
                    "text",
                    "font_size",
                    "weight",
                    "align",
                    "color",
                    "fill",
                ],
            )?;
            for key in patch.keys() {
                effect.touch(format!(
                    "shot/{shot}/take/{take}/panel/{panel}/layer/{layer}/field/{key}"
                ));
            }
        }
        Op::AddSticky {
            id,
            text,
            kind,
            author,
            pin,
            created,
        } => {
            let id = next_id(project, "sticky", id.as_deref())?;
            if !matches!(pin, Pin::None) {
                effect.touch(pin_footprint(pin));
            }
            project.stickies.push(Sticky {
                id: id.clone(),
                text: text.clone(),
                kind: kind.clone(),
                author: author.clone(),
                pin: pin.clone(),
                reply_to: None,
                resolved: false,
                created: created.clone(),
                extra: ExtraFields::new(),
            });
            effect.touch(format!("sticky/{id}"));
            effect.touch("sticky/order");
            effect.touch("ids/sticky");
            effect.created(id);
        }
        Op::RemoveSticky { sticky } => {
            if project
                .stickies
                .iter()
                .any(|item| item.reply_to.as_deref() == Some(sticky))
            {
                return Err(OpError::at(
                    format!("sticky.{sticky}"),
                    "cannot remove a sticky with replies",
                ));
            }
            let index = by_id(&project.stickies, sticky, |sticky| &sticky.id, "sticky")?;
            project.stickies.remove(index);
            let layout = format!("sticky:{sticky}");
            project.layout.positions.remove(&layout);
            effect.touch(format!("sticky/{sticky}"));
            effect.touch("sticky/order");
            effect.touch(format!("layout/{layout}"));
            effect.touch("ids/sticky");
        }
        Op::MoveSticky { sticky, index } => {
            move_item(
                &mut project.stickies,
                sticky,
                *index,
                |sticky| &sticky.id,
                "index",
            )?;
            effect.touch("sticky/order");
        }
        Op::UpdateSticky { sticky, patch } => {
            patch_value(
                sticky_mut(project, sticky)?,
                patch,
                &["text", "kind", "pin"],
            )?;
            for key in patch.keys() {
                effect.touch(format!("sticky/{sticky}/field/{key}"));
            }
            if patch.contains_key("pin") {
                let pin = &sticky_mut(project, sticky)?.pin;
                if !matches!(pin, Pin::None) {
                    effect.touch(pin_footprint(pin));
                }
            }
        }
        Op::AddClip {
            cut,
            id,
            source,
            index,
        } => {
            let id = next_id(project, "clip", id.as_deref())?;
            let cut = cut_mut(project, cut)?;
            let destination = index.unwrap_or(cut.clips.len());
            insert(
                &mut cut.clips,
                destination,
                Clip {
                    id: id.clone(),
                    source: source.clone(),
                    in_frame: 0,
                    out_frame: None,
                    slot: None,
                    extra: ExtraFields::new(),
                },
                "index",
            )?;
            effect.touch(format!("clip/{}/{id}", cut.id));
            effect.touch(format!("cut/{}/order", cut.id));
            effect.touch(format!("cut/{}", cut.id));
            touch_clip_source(effect, source);
            effect.touch("ids/clip");
            effect.created(id);
        }
        Op::RemoveClip { cut, clip } => {
            {
                let cut_obj = cut_mut(project, cut)?;
                let index = by_id(&cut_obj.clips, clip, |clip| &clip.id, "clip")?;
                cut_obj.clips.remove(index);
            }
            effect.touch(format!("clip/{cut}/{clip}"));
            effect.touch(format!("cut/{cut}/order"));
            effect.touch("ids/clip");
            effect.unpinned(unpin(project, |pin| matches!(pin, Pin::Clip { cut: pin_cut, clip: pin_clip } if pin_cut == cut && pin_clip == clip)));
        }
        Op::ReorderClip { cut, clip, index } => {
            let cut = cut_mut(project, cut)?;
            move_item(&mut cut.clips, clip, *index, |clip| &clip.id, "index")?;
            effect.touch(format!("cut/{}/order", cut.id));
        }
        Op::UpdateClip { cut, clip, patch } => {
            let clip = clip_mut(project, cut, clip)?;
            patch_value(clip, patch, &["source", "in", "out", "slot"])?;
            for key in patch.keys() {
                if ["in", "out", "slot"].contains(&key.as_str()) {
                    effect.touch(format!("cut/{cut}/clip/{}/trim", clip.id));
                } else {
                    effect.touch(format!("clip/{cut}/{}/field/{key}", clip.id));
                }
            }
            if patch.contains_key("source") {
                touch_clip_source(effect, &clip.source);
            }
        }
        Op::AddCut { id, name, index } => {
            let id = next_id(project, "cut", id.as_deref())?;
            let destination = index.unwrap_or(project.cuts.len());
            insert(
                &mut project.cuts,
                destination,
                Cut {
                    id: id.clone(),
                    name: name.clone(),
                    clips: Vec::new(),
                    extra: ExtraFields::new(),
                },
                "index",
            )?;
            effect.touch(format!("cut/{id}"));
            effect.touch("cut/order");
            effect.touch("ids/cut");
            effect.created(id);
        }
        Op::UpdateCut { cut, patch } => {
            patch_value(cut_mut(project, cut)?, patch, &["name"])?;
            for key in patch.keys() {
                effect.touch(format!("cut/{cut}/field/{key}"));
            }
        }
        Op::RemoveCut { cut } => {
            if project.cuts.iter().any(|candidate| candidate.id != *cut && candidate.clips.iter().any(|clip| matches!(&clip.source, ClipSource::Cut(source) if source.cut == *cut))) { return Err(OpError::at(format!("cut.{cut}"), "cannot remove a cut used by another cut")); }
            let index = by_id(&project.cuts, cut, |cut| &cut.id, "cut")?;
            let removed = project.cuts.remove(index);
            if project.active_cut.as_deref() == Some(cut) {
                project.active_cut = None;
                effect.touch("active_cut");
            }
            let layout = format!("cut:{cut}");
            project.layout.positions.remove(&layout);
            effect.touch(format!("cut/{cut}"));
            effect.touch("cut/order");
            effect.touch(format!("layout/{layout}"));
            effect.touch("ids/cut");
            if !removed.clips.is_empty() {
                effect.touch("ids/clip");
            }
            effect.unpinned(unpin(
                project,
                |pin| matches!(pin, Pin::Clip { cut: pin_cut, .. } if pin_cut == cut),
            ));
        }
        Op::SetActiveCut { cut } => {
            if let Some(cut) = cut
                && !project.cuts.iter().any(|candidate| candidate.id == *cut)
            {
                return Err(options_for(
                    "cut",
                    "unknown cut",
                    project.cuts.iter().map(|cut| cut.id.clone()),
                ));
            }
            project.active_cut = cut.clone();
            effect.touch("active_cut");
            if let Some(cut) = cut {
                effect.touch(format!("cut/{cut}"));
            }
        }
        Op::MoveShotBetweenScenes { shot, scene, index } => {
            let (from, at) = shot_location(project, shot)?;
            let moving = project.scenes[from].shots.remove(at);
            let from_id = project.scenes[from].id.clone();
            let destination = scene_mut(project, scene)?;
            insert(&mut destination.shots, *index, moving, "index")?;
            effect.touch(format!("shot/{shot}"));
            effect.touch(format!("scene/{from_id}/order"));
            effect.touch(format!("scene/{}/order", destination.id));
        }
        Op::AddRef {
            target,
            reference,
            index,
        } => {
            let refs = refs_mut(project, target)?;
            insert(
                refs,
                index.unwrap_or(refs.len()),
                reference.clone(),
                "index",
            )?;
            effect.touch(ref_footprint(target));
        }
        Op::RemoveRef { target, reference } => {
            let refs = refs_mut(project, target)?;
            let index = refs
                .iter()
                .position(|value| value == reference)
                .ok_or_else(|| {
                    OpError::options("reference", "unknown ref", refs.iter().cloned())
                })?;
            refs.remove(index);
            effect.touch(ref_footprint(target));
        }
        Op::ResolveSticky { sticky } => {
            let ids = thread_ids(project, sticky)?;
            if ids
                .iter()
                .all(|id| sticky_mut(project, id).is_ok_and(|item| item.resolved))
            {
                return Err(OpError::at(
                    format!("sticky.{sticky}.resolved"),
                    "sticky thread is already resolved",
                ));
            }
            for id in ids {
                sticky_mut(project, &id)?.resolved = true;
                effect.touch(format!("sticky/{id}"));
                effect.touch(format!("sticky/{id}/field/resolved"));
            }
        }
        Op::ReplySticky {
            sticky,
            id,
            text,
            kind,
            author,
            created,
        } => {
            let _ = sticky_mut(project, sticky)?;
            effect.touch(format!("sticky/{sticky}"));
            let id = next_id(project, "sticky", id.as_deref())?;
            project.stickies.push(Sticky {
                id: id.clone(),
                text: text.clone(),
                kind: kind.clone(),
                author: author.clone(),
                pin: Pin::None,
                reply_to: Some(sticky.clone()),
                resolved: false,
                created: created.clone(),
                extra: ExtraFields::new(),
            });
            effect.touch(format!("sticky/{id}"));
            effect.touch("sticky/order");
            effect.touch("ids/sticky");
            effect.created(id);
        }
        Op::MoveNode { node, position } => {
            if !node_exists(project, node) {
                return Err(OpError::options(
                    "node",
                    format!("unknown node {node}"),
                    valid_nodes(project),
                ));
            }
            if let Some(position) = position {
                let mut position = position.clone();
                if let Some(previous) = project.layout.positions.get(node) {
                    let mut extra = previous.extra.clone();
                    extra.extend(position.extra);
                    position.extra = extra;
                }
                project.layout.positions.insert(node.clone(), position);
            } else {
                project.layout.positions.remove(node);
            }
            effect.touch(format!("layout/{node}"));
            effect.touch(node_footprint(node));
        }
    }
    Ok(())
}

fn blank_panel(id: String) -> Panel {
    Panel {
        id,
        time: 0.0,
        title: String::new(),
        direction: String::new(),
        background: String::new(),
        layers: Vec::new(),
        strokes: Vec::new(),
        extra: ExtraFields::new(),
    }
}

fn project_changes(before: &Project, after: &Project) -> Result<Vec<RestoreChange>, OpError> {
    let mut changes = Vec::new();
    diff_fields(
        before,
        after,
        RestoreTarget::Project,
        &["scenes", "cuts", "stickies", "layout"],
        &mut changes,
    )?;
    diff_scenes(before, after, &mut changes)?;
    diff_cuts(before, after, &mut changes)?;
    diff_stickies(before, after, &mut changes)?;
    let nodes: BTreeSet<_> = before
        .layout
        .positions
        .keys()
        .chain(after.layout.positions.keys())
        .cloned()
        .collect();
    for node in nodes {
        if before.layout.positions.get(&node) != after.layout.positions.get(&node) {
            changes.push(RestoreChange {
                action: RestoreAction::SetLayout {
                    node: node.clone(),
                    position: before.layout.positions.get(&node).cloned(),
                },
            });
        }
    }
    Ok(changes)
}

fn diff_fields<T: Serialize>(
    before: &T,
    after: &T,
    target: RestoreTarget,
    skip: &[&str],
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    let before =
        serde_json::to_value(before).map_err(|error| OpError::at("inverse", error.to_string()))?;
    let after =
        serde_json::to_value(after).map_err(|error| OpError::at("inverse", error.to_string()))?;
    let before = before
        .as_object()
        .ok_or_else(|| OpError::at("inverse", "inverse target is not an object"))?;
    let after = after
        .as_object()
        .ok_or_else(|| OpError::at("inverse", "inverse target is not an object"))?;
    let keys: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
    for field in keys {
        if field == "id" || skip.contains(&field.as_str()) {
            continue;
        }
        if before.get(&field) != after.get(&field) {
            changes.push(RestoreChange {
                action: RestoreAction::SetField {
                    target: target.clone(),
                    field: field.clone(),
                    value: before.get(&field).cloned(),
                },
            });
        }
    }
    Ok(())
}

fn diff_structure<T: Serialize>(
    before: &[T],
    after: &[T],
    parent: RestoreParent,
    id: impl Fn(&T) -> &str,
    target: impl Fn(&str) -> RestoreTarget,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    for item in after {
        if !before.iter().any(|candidate| id(candidate) == id(item)) {
            changes.push(RestoreChange {
                action: RestoreAction::Delete {
                    target: target(id(item)),
                },
            });
        }
    }
    for (index, item) in before.iter().enumerate() {
        if !after.iter().any(|candidate| id(candidate) == id(item)) {
            changes.push(RestoreChange {
                action: RestoreAction::Insert {
                    parent: parent.clone(),
                    index,
                    value: serde_json::to_value(item)
                        .map_err(|error| OpError::at("inverse", error.to_string()))?,
                },
            });
        }
    }
    let before_ids: Vec<_> = before.iter().map(&id).collect();
    let after_ids: Vec<_> = after.iter().map(&id).collect();
    if before_ids != after_ids {
        for (index, item) in before.iter().enumerate() {
            if after.iter().any(|candidate| id(candidate) == id(item)) {
                changes.push(RestoreChange {
                    action: RestoreAction::Move {
                        target: target(id(item)),
                        parent: parent.clone(),
                        index,
                    },
                });
            }
        }
    }
    Ok(())
}

fn diff_scenes(
    before: &Project,
    after: &Project,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    diff_structure(
        &before.scenes,
        &after.scenes,
        RestoreParent::Scenes,
        |scene| &scene.id,
        |scene| RestoreTarget::Scene {
            scene: scene.into(),
        },
        changes,
    )?;
    for scene in &before.scenes {
        let Some(next) = after
            .scenes
            .iter()
            .find(|candidate| candidate.id == scene.id)
        else {
            continue;
        };
        let target = RestoreTarget::Scene {
            scene: scene.id.clone(),
        };
        diff_fields(scene, next, target, &["shots"], changes)?;
    }
    diff_shots(before, after, changes)?;
    Ok(())
}

fn diff_shots(
    before: &Project,
    after: &Project,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    for scene in &after.scenes {
        if !before
            .scenes
            .iter()
            .any(|candidate| candidate.id == scene.id)
        {
            continue;
        }
        for shot in &scene.shots {
            if shot_entry(before, &shot.id).is_none() {
                changes.push(RestoreChange {
                    action: RestoreAction::Delete {
                        target: RestoreTarget::Shot {
                            shot: shot.id.clone(),
                        },
                    },
                });
            }
        }
    }

    for scene in &before.scenes {
        if !after
            .scenes
            .iter()
            .any(|candidate| candidate.id == scene.id)
        {
            continue;
        }
        for (index, shot) in scene.shots.iter().enumerate() {
            match shot_entry(after, &shot.id) {
                None => changes.push(RestoreChange {
                    action: RestoreAction::Insert {
                        parent: RestoreParent::Shots {
                            scene: scene.id.clone(),
                        },
                        index,
                        value: serde_json::to_value(shot)
                            .map_err(|error| OpError::at("inverse", error.to_string()))?,
                    },
                }),
                Some((next_scene, _, _)) if next_scene != scene.id => {
                    changes.push(RestoreChange {
                        action: RestoreAction::Move {
                            target: RestoreTarget::Shot {
                                shot: shot.id.clone(),
                            },
                            parent: RestoreParent::Shots {
                                scene: scene.id.clone(),
                            },
                            index,
                        },
                    });
                }
                Some(_) => {}
            }
        }
    }

    for scene in &before.scenes {
        let Some(next_scene) = after
            .scenes
            .iter()
            .find(|candidate| candidate.id == scene.id)
        else {
            continue;
        };
        let before_stable: Vec<_> = scene
            .shots
            .iter()
            .filter(|shot| shot_entry(after, &shot.id).is_some_and(|(next, _, _)| next == scene.id))
            .map(|shot| shot.id.as_str())
            .collect();
        let after_stable: Vec<_> = next_scene
            .shots
            .iter()
            .filter(|shot| {
                shot_entry(before, &shot.id).is_some_and(|(previous, _, _)| previous == scene.id)
            })
            .map(|shot| shot.id.as_str())
            .collect();
        if before_stable != after_stable {
            for (index, shot) in scene.shots.iter().enumerate() {
                if after_stable.contains(&shot.id.as_str()) {
                    changes.push(RestoreChange {
                        action: RestoreAction::Move {
                            target: RestoreTarget::Shot {
                                shot: shot.id.clone(),
                            },
                            parent: RestoreParent::Shots {
                                scene: scene.id.clone(),
                            },
                            index,
                        },
                    });
                }
            }
        }
    }

    for scene in &before.scenes {
        for shot in &scene.shots {
            let Some((_, _, next)) = shot_entry(after, &shot.id) else {
                continue;
            };
            diff_shot(shot, next, changes)?;
        }
    }
    Ok(())
}

fn shot_entry<'a>(project: &'a Project, wanted: &str) -> Option<(&'a str, usize, &'a Shot)> {
    project.scenes.iter().find_map(|scene| {
        scene
            .shots
            .iter()
            .enumerate()
            .find(|(_, shot)| shot.id == wanted)
            .map(|(index, shot)| (scene.id.as_str(), index, shot))
    })
}

fn diff_shot(shot: &Shot, next: &Shot, changes: &mut Vec<RestoreChange>) -> Result<(), OpError> {
    diff_fields(
        shot,
        next,
        RestoreTarget::Shot {
            shot: shot.id.clone(),
        },
        &["takes", "refs"],
        changes,
    )?;
    diff_refs(
        &shot.refs,
        &next.refs,
        RefTarget::Shot {
            shot: shot.id.clone(),
        },
        changes,
    );
    diff_takes(shot, next, changes)?;
    Ok(())
}

fn diff_takes(
    before: &Shot,
    after: &Shot,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    diff_structure(
        &before.takes,
        &after.takes,
        RestoreParent::Takes {
            shot: before.id.clone(),
        },
        |take| &take.id,
        |take| RestoreTarget::Take {
            shot: before.id.clone(),
            take: take.into(),
        },
        changes,
    )?;
    for take in &before.takes {
        let Some(next) = after.takes.iter().find(|candidate| candidate.id == take.id) else {
            continue;
        };
        diff_fields(
            take,
            next,
            RestoreTarget::Take {
                shot: before.id.clone(),
                take: take.id.clone(),
            },
            &["panels", "refs"],
            changes,
        )?;
        diff_refs(
            &take.refs,
            &next.refs,
            RefTarget::Take {
                shot: before.id.clone(),
                take: take.id.clone(),
            },
            changes,
        );
        diff_panels(&before.id, take, next, changes)?;
    }
    Ok(())
}

fn diff_panels(
    shot: &str,
    before: &Take,
    after: &Take,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    diff_structure(
        &before.panels,
        &after.panels,
        RestoreParent::Panels {
            shot: shot.into(),
            take: before.id.clone(),
        },
        |panel| &panel.id,
        |panel| RestoreTarget::Panel {
            shot: shot.into(),
            take: before.id.clone(),
            panel: panel.into(),
        },
        changes,
    )?;
    for panel in &before.panels {
        let Some(next) = after
            .panels
            .iter()
            .find(|candidate| candidate.id == panel.id)
        else {
            continue;
        };
        diff_fields(
            panel,
            next,
            RestoreTarget::Panel {
                shot: shot.into(),
                take: before.id.clone(),
                panel: panel.id.clone(),
            },
            &["layers"],
            changes,
        )?;
        diff_layers(shot, &before.id, panel, next, changes)?;
    }
    Ok(())
}

fn diff_layers(
    shot: &str,
    take: &str,
    before: &Panel,
    after: &Panel,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    diff_structure(
        &before.layers,
        &after.layers,
        RestoreParent::Layers {
            shot: shot.into(),
            take: take.into(),
            panel: before.id.clone(),
        },
        |layer| &layer.id,
        |layer| RestoreTarget::Layer {
            shot: shot.into(),
            take: take.into(),
            panel: before.id.clone(),
            layer: layer.into(),
        },
        changes,
    )?;
    for layer in &before.layers {
        let Some(next) = after
            .layers
            .iter()
            .find(|candidate| candidate.id == layer.id)
        else {
            continue;
        };
        diff_fields(
            layer,
            next,
            RestoreTarget::Layer {
                shot: shot.into(),
                take: take.into(),
                panel: before.id.clone(),
                layer: layer.id.clone(),
            },
            &[],
            changes,
        )?;
    }
    Ok(())
}

fn diff_cuts(
    before: &Project,
    after: &Project,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    diff_structure(
        &before.cuts,
        &after.cuts,
        RestoreParent::Cuts,
        |cut| &cut.id,
        |cut| RestoreTarget::Cut { cut: cut.into() },
        changes,
    )?;
    for cut in &before.cuts {
        let Some(next) = after.cuts.iter().find(|candidate| candidate.id == cut.id) else {
            continue;
        };
        diff_fields(
            cut,
            next,
            RestoreTarget::Cut {
                cut: cut.id.clone(),
            },
            &["clips"],
            changes,
        )?;
        diff_structure(
            &cut.clips,
            &next.clips,
            RestoreParent::Clips {
                cut: cut.id.clone(),
            },
            |clip| &clip.id,
            |clip| RestoreTarget::Clip {
                cut: cut.id.clone(),
                clip: clip.into(),
            },
            changes,
        )?;
        for clip in &cut.clips {
            let Some(next_clip) = next.clips.iter().find(|candidate| candidate.id == clip.id)
            else {
                continue;
            };
            diff_fields(
                clip,
                next_clip,
                RestoreTarget::Clip {
                    cut: cut.id.clone(),
                    clip: clip.id.clone(),
                },
                &[],
                changes,
            )?;
        }
    }
    Ok(())
}

fn diff_stickies(
    before: &Project,
    after: &Project,
    changes: &mut Vec<RestoreChange>,
) -> Result<(), OpError> {
    diff_structure(
        &before.stickies,
        &after.stickies,
        RestoreParent::Stickies,
        |sticky| &sticky.id,
        |sticky| RestoreTarget::Sticky {
            sticky: sticky.into(),
        },
        changes,
    )?;
    for sticky in &before.stickies {
        let Some(next) = after
            .stickies
            .iter()
            .find(|candidate| candidate.id == sticky.id)
        else {
            continue;
        };
        diff_fields(
            sticky,
            next,
            RestoreTarget::Sticky {
                sticky: sticky.id.clone(),
            },
            &[],
            changes,
        )?;
    }
    Ok(())
}

fn diff_refs(
    before: &[String],
    after: &[String],
    target: RefTarget,
    changes: &mut Vec<RestoreChange>,
) {
    let mut current = after.to_vec();
    for (index, wanted) in before.iter().enumerate() {
        if current.get(index) == Some(wanted) {
            continue;
        }
        if let Some(offset) = current[index.min(current.len())..]
            .iter()
            .position(|reference| reference == wanted)
        {
            let from = index.min(current.len()) + offset;
            let reference = current.remove(from);
            current.insert(index, reference.clone());
            changes.push(RestoreChange {
                action: RestoreAction::MoveRef {
                    target: target.clone(),
                    from,
                    index,
                    reference,
                },
            });
        } else {
            current.insert(index, wanted.clone());
            changes.push(RestoreChange {
                action: RestoreAction::InsertRef {
                    target: target.clone(),
                    index,
                    reference: wanted.clone(),
                },
            });
        }
    }
    while current.len() > before.len() {
        let index = before.len();
        let reference = current.remove(index);
        changes.push(RestoreChange {
            action: RestoreAction::DeleteRef {
                target: target.clone(),
                index,
                reference,
            },
        });
    }
}

fn restore_changes(project: &mut Project, changes: &[RestoreChange]) -> Result<(), OpError> {
    for priority in 0..5 {
        for change in changes
            .iter()
            .filter(|change| restore_priority(&change.action) == priority)
        {
            restore_action(project, &change.action)?;
        }
    }
    Ok(())
}

fn restore_priority(action: &RestoreAction) -> usize {
    match action {
        RestoreAction::Delete { .. } => 0,
        RestoreAction::Insert { .. } => 1,
        RestoreAction::Move { .. } => 2,
        RestoreAction::InsertRef { .. }
        | RestoreAction::DeleteRef { .. }
        | RestoreAction::MoveRef { .. } => 3,
        RestoreAction::SetField { .. } | RestoreAction::SetLayout { .. } => 4,
    }
}

fn restore_action(project: &mut Project, action: &RestoreAction) -> Result<(), OpError> {
    match action {
        RestoreAction::Delete { target } => restore_delete(project, target),
        RestoreAction::Insert {
            parent,
            index,
            value,
        } => restore_insert(project, parent, *index, value),
        RestoreAction::Move {
            target,
            parent,
            index,
        } => restore_move(project, target, parent, *index),
        RestoreAction::SetField {
            target,
            field,
            value,
        } => restore_field(project, target, field, value.clone()),
        RestoreAction::InsertRef {
            target,
            index,
            reference,
        } => insert(
            refs_mut(project, target)?,
            *index,
            reference.clone(),
            "inverse.ref.index",
        ),
        RestoreAction::DeleteRef {
            target,
            index,
            reference,
        } => {
            let refs = refs_mut(project, target)?;
            if refs.get(*index) != Some(reference) {
                return Err(OpError::at(
                    "inverse.ref",
                    "inverse reference no longer exists at its stored index",
                ));
            }
            refs.remove(*index);
            Ok(())
        }
        RestoreAction::MoveRef {
            target,
            from,
            index,
            reference,
        } => {
            let refs = refs_mut(project, target)?;
            if refs.get(*from) != Some(reference) {
                return Err(OpError::at(
                    "inverse.ref",
                    "inverse reference no longer exists at its stored index",
                ));
            }
            let reference = refs.remove(*from);
            insert(refs, *index, reference, "inverse.ref.index")
        }
        RestoreAction::SetLayout { node, position } => {
            match position {
                Some(position) => {
                    project
                        .layout
                        .positions
                        .insert(node.clone(), position.clone());
                }
                None => {
                    project.layout.positions.remove(node);
                }
            }
            Ok(())
        }
    }
}

fn restore_delete(project: &mut Project, target: &RestoreTarget) -> Result<(), OpError> {
    match target {
        RestoreTarget::Project => Err(OpError::at("inverse", "cannot delete project root")),
        RestoreTarget::Scene { scene } => remove_by_id(&mut project.scenes, scene, |item| &item.id),
        RestoreTarget::Shot { shot } => {
            let (scene, index) = shot_location(project, shot)?;
            project.scenes[scene].shots.remove(index);
            Ok(())
        }
        RestoreTarget::Take { shot, take } => {
            let takes = &mut shot_mut(project, shot)?.takes;
            let index = by_id(takes, take, |item| &item.id, "inverse.take")?;
            takes.remove(index);
            Ok(())
        }
        RestoreTarget::Panel { shot, take, panel } => {
            let panels = &mut take_mut(project, shot, take)?.panels;
            let index = by_id(panels, panel, |item| &item.id, "inverse.panel")?;
            panels.remove(index);
            Ok(())
        }
        RestoreTarget::Layer {
            shot,
            take,
            panel,
            layer,
        } => {
            let layers = &mut panel_mut(project, shot, take, panel)?.layers;
            let index = by_id(layers, layer, |item| &item.id, "inverse.layer")?;
            layers.remove(index);
            Ok(())
        }
        RestoreTarget::Sticky { sticky } => {
            remove_by_id(&mut project.stickies, sticky, |item| &item.id)
        }
        RestoreTarget::Cut { cut } => remove_by_id(&mut project.cuts, cut, |item| &item.id),
        RestoreTarget::Clip { cut, clip } => {
            let clips = &mut cut_mut(project, cut)?.clips;
            let index = by_id(clips, clip, |item| &item.id, "inverse.clip")?;
            clips.remove(index);
            Ok(())
        }
    }
}

fn remove_by_id<T>(
    items: &mut Vec<T>,
    wanted: &str,
    id: impl Fn(&T) -> &String,
) -> Result<(), OpError> {
    let index = by_id(items, wanted, id, "inverse.target")?;
    items.remove(index);
    Ok(())
}

fn restore_insert(
    project: &mut Project,
    parent: &RestoreParent,
    index: usize,
    value: &Value,
) -> Result<(), OpError> {
    macro_rules! decode {
        ($kind:ty) => {
            serde_json::from_value::<$kind>(value.clone())
                .map_err(|error| OpError::at("inverse.value", error.to_string()))?
        };
    }
    match parent {
        RestoreParent::Scenes => {
            insert(&mut project.scenes, index, decode!(Scene), "inverse.index")
        }
        RestoreParent::Shots { scene } => insert(
            &mut scene_mut(project, scene)?.shots,
            index,
            decode!(Shot),
            "inverse.index",
        ),
        RestoreParent::Takes { shot } => insert(
            &mut shot_mut(project, shot)?.takes,
            index,
            decode!(Take),
            "inverse.index",
        ),
        RestoreParent::Panels { shot, take } => insert(
            &mut take_mut(project, shot, take)?.panels,
            index,
            decode!(Panel),
            "inverse.index",
        ),
        RestoreParent::Layers { shot, take, panel } => insert(
            &mut panel_mut(project, shot, take, panel)?.layers,
            index,
            decode!(Layer),
            "inverse.index",
        ),
        RestoreParent::Stickies => insert(
            &mut project.stickies,
            index,
            decode!(Sticky),
            "inverse.index",
        ),
        RestoreParent::Cuts => insert(&mut project.cuts, index, decode!(Cut), "inverse.index"),
        RestoreParent::Clips { cut } => insert(
            &mut cut_mut(project, cut)?.clips,
            index,
            decode!(Clip),
            "inverse.index",
        ),
    }
}

fn restore_move(
    project: &mut Project,
    target: &RestoreTarget,
    parent: &RestoreParent,
    index: usize,
) -> Result<(), OpError> {
    match (target, parent) {
        (RestoreTarget::Scene { scene }, RestoreParent::Scenes) => move_item(
            &mut project.scenes,
            scene,
            index,
            |item| &item.id,
            "inverse.index",
        ),
        (RestoreTarget::Shot { shot }, RestoreParent::Shots { scene }) => {
            let (old_scene, old_index) = shot_location(project, shot)?;
            let item = project.scenes[old_scene].shots.remove(old_index);
            insert(
                &mut scene_mut(project, scene)?.shots,
                index,
                item,
                "inverse.index",
            )
        }
        (RestoreTarget::Take { shot, take }, RestoreParent::Takes { shot: parent_shot })
            if shot == parent_shot =>
        {
            move_item(
                &mut shot_mut(project, shot)?.takes,
                take,
                index,
                |item| &item.id,
                "inverse.index",
            )
        }
        (
            RestoreTarget::Panel { shot, take, panel },
            RestoreParent::Panels {
                shot: parent_shot,
                take: parent_take,
            },
        ) if shot == parent_shot && take == parent_take => move_item(
            &mut take_mut(project, shot, take)?.panels,
            panel,
            index,
            |item| &item.id,
            "inverse.index",
        ),
        (
            RestoreTarget::Layer {
                shot,
                take,
                panel,
                layer,
            },
            RestoreParent::Layers {
                shot: parent_shot,
                take: parent_take,
                panel: parent_panel,
            },
        ) if shot == parent_shot && take == parent_take && panel == parent_panel => move_item(
            &mut panel_mut(project, shot, take, panel)?.layers,
            layer,
            index,
            |item| &item.id,
            "inverse.index",
        ),
        (RestoreTarget::Sticky { sticky }, RestoreParent::Stickies) => move_item(
            &mut project.stickies,
            sticky,
            index,
            |item| &item.id,
            "inverse.index",
        ),
        (RestoreTarget::Cut { cut }, RestoreParent::Cuts) => move_item(
            &mut project.cuts,
            cut,
            index,
            |item| &item.id,
            "inverse.index",
        ),
        (RestoreTarget::Clip { cut, clip }, RestoreParent::Clips { cut: parent_cut })
            if cut == parent_cut =>
        {
            move_item(
                &mut cut_mut(project, cut)?.clips,
                clip,
                index,
                |item| &item.id,
                "inverse.index",
            )
        }
        _ => Err(OpError::at(
            "inverse.target",
            "inverse target and parent disagree",
        )),
    }
}

fn restore_field(
    project: &mut Project,
    target: &RestoreTarget,
    field: &str,
    value: Option<Value>,
) -> Result<(), OpError> {
    match target {
        RestoreTarget::Project => set_field(project, field, value),
        RestoreTarget::Scene { scene } => set_field(scene_mut(project, scene)?, field, value),
        RestoreTarget::Shot { shot } => set_field(shot_mut(project, shot)?, field, value),
        RestoreTarget::Take { shot, take } => {
            set_field(take_mut(project, shot, take)?, field, value)
        }
        RestoreTarget::Panel { shot, take, panel } => {
            set_field(panel_mut(project, shot, take, panel)?, field, value)
        }
        RestoreTarget::Layer {
            shot,
            take,
            panel,
            layer,
        } => set_field(layer_mut(project, shot, take, panel, layer)?, field, value),
        RestoreTarget::Sticky { sticky } => set_field(sticky_mut(project, sticky)?, field, value),
        RestoreTarget::Cut { cut } => set_field(cut_mut(project, cut)?, field, value),
        RestoreTarget::Clip { cut, clip } => set_field(clip_mut(project, cut, clip)?, field, value),
    }
}

fn set_field<T>(target: &mut T, field: &str, value: Option<Value>) -> Result<(), OpError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let mut document = serde_json::to_value(&*target)
        .map_err(|error| OpError::at("inverse.value", error.to_string()))?;
    let object = document
        .as_object_mut()
        .ok_or_else(|| OpError::at("inverse.target", "inverse target is not an object"))?;
    match value {
        Some(value) => {
            object.insert(field.into(), value);
        }
        None => {
            object.remove(field);
        }
    }
    *target = serde_json::from_value(document)
        .map_err(|error| OpError::at("inverse.value", error.to_string()))?;
    Ok(())
}

fn insert<T>(items: &mut Vec<T>, index: usize, item: T, field: &str) -> Result<(), OpError> {
    if index > items.len() {
        Err(OpError::options(
            field,
            "index is outside the list",
            (0..=items.len()).map(|index| index.to_string()),
        ))
    } else {
        items.insert(index, item);
        Ok(())
    }
}
fn by_id<T>(
    items: &[T],
    wanted: &str,
    id: impl Fn(&T) -> &String,
    field: &str,
) -> Result<usize, OpError> {
    items
        .iter()
        .position(|item| id(item) == wanted)
        .ok_or_else(|| options_for(field, "unknown id", items.iter().map(id).cloned()))
}
fn move_item<T>(
    items: &mut Vec<T>,
    wanted: &str,
    index: usize,
    id: impl Fn(&T) -> &String,
    field: &str,
) -> Result<(), OpError> {
    let old = by_id(items, wanted, id, field)?;
    let item = items.remove(old);
    if index > items.len() {
        let valid = (0..=items.len())
            .map(|index| index.to_string())
            .collect::<Vec<_>>();
        items.insert(old, item);
        return Err(OpError::options(field, "index is outside the list", valid));
    }
    items.insert(index, item);
    Ok(())
}
fn options_for(field: &str, message: &str, values: impl IntoIterator<Item = String>) -> OpError {
    OpError::options(field, message, values)
}

fn scene_index(project: &Project, scene: &str) -> Result<usize, OpError> {
    by_id(&project.scenes, scene, |scene| &scene.id, "scene")
}
fn scene_mut<'a>(project: &'a mut Project, scene: &str) -> Result<&'a mut Scene, OpError> {
    let index = scene_index(project, scene)?;
    Ok(&mut project.scenes[index])
}
fn shot_location(project: &Project, shot: &str) -> Result<(usize, usize), OpError> {
    project
        .scenes
        .iter()
        .enumerate()
        .find_map(|(scene, value)| {
            value
                .shots
                .iter()
                .position(|item| item.id == shot)
                .map(|index| (scene, index))
        })
        .ok_or_else(|| {
            options_for(
                "shot",
                "unknown shot",
                project
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.shots.iter())
                    .map(|shot| shot.id.clone()),
            )
        })
}
fn shot_mut<'a>(project: &'a mut Project, shot: &str) -> Result<&'a mut Shot, OpError> {
    let (scene, shot) = shot_location(project, shot)?;
    Ok(&mut project.scenes[scene].shots[shot])
}
fn take_mut<'a>(project: &'a mut Project, shot: &str, take: &str) -> Result<&'a mut Take, OpError> {
    let shot = shot_mut(project, shot)?;
    let index = by_id(&shot.takes, take, |take| &take.id, "take")?;
    Ok(&mut shot.takes[index])
}
fn panel_mut<'a>(
    project: &'a mut Project,
    shot: &str,
    take: &str,
    panel: &str,
) -> Result<&'a mut Panel, OpError> {
    let take = take_mut(project, shot, take)?;
    let index = by_id(&take.panels, panel, |panel| &panel.id, "panel")?;
    Ok(&mut take.panels[index])
}
fn layer_mut<'a>(
    project: &'a mut Project,
    shot: &str,
    take: &str,
    panel: &str,
    layer: &str,
) -> Result<&'a mut Layer, OpError> {
    let panel = panel_mut(project, shot, take, panel)?;
    let index = by_id(&panel.layers, layer, |layer| &layer.id, "layer")?;
    Ok(&mut panel.layers[index])
}
fn sticky_mut<'a>(project: &'a mut Project, sticky: &str) -> Result<&'a mut Sticky, OpError> {
    let index = by_id(&project.stickies, sticky, |sticky| &sticky.id, "sticky")?;
    Ok(&mut project.stickies[index])
}
fn cut_mut<'a>(project: &'a mut Project, cut: &str) -> Result<&'a mut Cut, OpError> {
    let index = by_id(&project.cuts, cut, |cut| &cut.id, "cut")?;
    Ok(&mut project.cuts[index])
}
fn clip_mut<'a>(project: &'a mut Project, cut: &str, clip: &str) -> Result<&'a mut Clip, OpError> {
    let cut = cut_mut(project, cut)?;
    let index = by_id(&cut.clips, clip, |clip| &clip.id, "clip")?;
    Ok(&mut cut.clips[index])
}

fn patch_value<T>(target: &mut T, patch: &Patch, valid: &[&str]) -> Result<(), OpError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let unknown: Vec<_> = patch
        .keys()
        .filter(|key| !valid.contains(&key.as_str()))
        .collect();
    if !unknown.is_empty() {
        let valid: Vec<_> = valid.iter().map(|field| (*field).to_owned()).collect();
        let issues = unknown
            .iter()
            .map(|key| OpIssue {
                field: format!("patch.{key}"),
                message: format!("unknown patch field {key}"),
                valid: valid.clone(),
            })
            .collect::<Vec<_>>();
        let first = &issues[0];
        return Err(OpError {
            field: first.field.clone(),
            message: first.message.clone(),
            valid: first.valid.clone(),
            issues,
        });
    }
    let mut value =
        serde_json::to_value(&*target).map_err(|error| OpError::at("patch", error.to_string()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| OpError::at("patch", "patch target is not an object"))?;
    for (key, value) in patch {
        object.insert(key.clone(), value.clone());
    }
    *target = serde_json::from_value(value)
        .map_err(|error| OpError::at("patch", format!("invalid patch: {error}")))?;
    Ok(())
}

fn next_id(project: &Project, kind: &str, requested: Option<&str>) -> Result<String, OpError> {
    let used = kind_ids(project, kind);
    if let Some(requested) = requested {
        if used.contains(requested) {
            return Err(OpError::at(
                "id",
                format!("{kind} id {requested} is already live"),
            ));
        }
        return Ok(requested.to_owned());
    }
    for number in 1.. {
        let id = format!("{kind}-{number}");
        if !used.contains(&id) {
            return Ok(id);
        }
    }
    unreachable!("an unbounded id space has a vacancy")
}

fn freshen_take(project: &Project, take: &mut Take) -> Vec<(&'static str, String)> {
    let mut panels = kind_ids(project, "panel");
    let mut layers = kind_ids(project, "layer");
    let mut created = Vec::new();
    for panel in &mut take.panels {
        panel.id = fresh_from(&mut panels, "panel");
        created.push(("panel", panel.id.clone()));
        for layer in &mut panel.layers {
            layer.id = fresh_from(&mut layers, "layer");
            created.push(("layer", layer.id.clone()));
        }
    }
    created
}

fn fresh_from(used: &mut HashSet<String>, kind: &str) -> String {
    for number in 1.. {
        let id = format!("{kind}-{number}");
        if used.insert(id.clone()) {
            return id;
        }
    }
    unreachable!("an unbounded id space has a vacancy")
}
fn kind_ids(project: &Project, kind: &str) -> HashSet<String> {
    match kind {
        "scene" => project.scenes.iter().map(|item| item.id.clone()).collect(),
        "shot" => project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .map(|item| item.id.clone())
            .collect(),
        "take" => project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .flat_map(|shot| &shot.takes)
            .map(|item| item.id.clone())
            .collect(),
        "panel" => project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .flat_map(|shot| &shot.takes)
            .flat_map(|take| &take.panels)
            .map(|item| item.id.clone())
            .collect(),
        "layer" => project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .flat_map(|shot| &shot.takes)
            .flat_map(|take| &take.panels)
            .flat_map(|panel| &panel.layers)
            .map(|item| item.id.clone())
            .collect(),
        "cut" => project.cuts.iter().map(|item| item.id.clone()).collect(),
        "clip" => project
            .cuts
            .iter()
            .flat_map(|cut| &cut.clips)
            .map(|item| item.id.clone())
            .collect(),
        "sticky" => project
            .stickies
            .iter()
            .map(|item| item.id.clone())
            .collect(),
        _ => HashSet::new(),
    }
}

fn refuse_shots_in_cuts(project: &Project, shots: &HashSet<String>) -> Result<(), OpError> {
    let names: BTreeSet<_> = project.cuts.iter().filter(|cut| cut.clips.iter().any(|clip| matches!(&clip.source, ClipSource::Shot(source) if shots.contains(&source.shot)))).map(|cut| if cut.name.is_empty() { cut.id.clone() } else { cut.name.clone() }).collect();
    if names.is_empty() {
        Ok(())
    } else {
        Err(OpError::options(
            format!("shot.{}", shots.iter().next().cloned().unwrap_or_default()),
            format!(
                "cannot remove shots used in cuts: {}",
                names.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
            project
                .cuts
                .iter()
                .filter(|cut| cut.clips.iter().any(|clip| matches!(&clip.source, ClipSource::Shot(source) if shots.contains(&source.shot))))
                .map(|cut| cut.id.clone()),
        ))
    }
}
fn unpin(project: &mut Project, predicate: impl Fn(&Pin) -> bool) -> Vec<String> {
    project
        .stickies
        .iter_mut()
        .filter_map(|sticky| {
            if predicate(&sticky.pin) {
                sticky.pin = Pin::None;
                Some(sticky.id.clone())
            } else {
                None
            }
        })
        .collect()
}
fn pin_scene_or_shot(pin: &Pin, scene: &Scene) -> bool {
    match pin {
        Pin::Scene(id) => id == &scene.id,
        Pin::Shot(id) => scene.shots.iter().any(|shot| &shot.id == id),
        Pin::Take { shot, .. } | Pin::Panel { shot, .. } | Pin::Layer { shot, .. } => {
            scene.shots.iter().any(|item| &item.id == shot)
        }
        _ => false,
    }
}
fn pin_shot(pin: &Pin, shot: &Shot) -> bool {
    matches!(pin, Pin::Shot(id) if id == &shot.id)
        || matches!(pin, Pin::Take { shot: id, .. } | Pin::Panel { shot: id, .. } | Pin::Layer { shot: id, .. } if id == &shot.id)
}
fn pin_take(pin: &Pin, shot: &str, take: &Take) -> bool {
    matches!(pin, Pin::Take { shot: pin_shot, take: pin_take } if pin_shot == shot && pin_take == &take.id)
        || matches!(pin, Pin::Panel { shot: pin_shot, take: pin_take, .. } | Pin::Layer { shot: pin_shot, take: pin_take, .. } if pin_shot == shot && pin_take == &take.id)
}
fn pin_panel(pin: &Pin, shot: &str, take: &str, panel: &Panel) -> bool {
    matches!(pin, Pin::Panel { shot: pin_shot, take: pin_take, panel: pin_panel } if pin_shot == shot && pin_take == take && pin_panel == &panel.id)
        || matches!(pin, Pin::Layer { shot: pin_shot, take: pin_take, panel: pin_panel, .. } if pin_shot == shot && pin_take == take && pin_panel == &panel.id)
}
fn pin_layer(pin: &Pin, shot: &str, take: &str, panel: &str, layer: &Layer) -> bool {
    matches!(pin, Pin::Layer { shot: pin_shot, take: pin_take, panel: pin_panel, layer: pin_layer } if pin_shot == shot && pin_take == take && pin_panel == panel && pin_layer == &layer.id)
}

fn refs_mut<'a>(
    project: &'a mut Project,
    target: &RefTarget,
) -> Result<&'a mut Vec<String>, OpError> {
    match target {
        RefTarget::Shot { shot } => Ok(&mut shot_mut(project, shot)?.refs),
        RefTarget::Take { shot, take } => Ok(&mut take_mut(project, shot, take)?.refs),
    }
}
fn ref_footprint(target: &RefTarget) -> String {
    match target {
        RefTarget::Shot { shot } => format!("shot/{shot}/refs"),
        RefTarget::Take { shot, take } => format!("take/{shot}/{take}/refs"),
    }
}
fn thread_ids(project: &Project, sticky: &str) -> Result<Vec<String>, OpError> {
    let root = root_id(project, sticky)?;
    Ok(project
        .stickies
        .iter()
        .filter(|candidate| root_id(project, &candidate.id).ok().as_deref() == Some(root.as_str()))
        .map(|sticky| sticky.id.clone())
        .collect())
}
fn root_id(project: &Project, sticky: &str) -> Result<String, OpError> {
    let mut id = sticky.to_owned();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(id.clone()) {
            return Err(OpError::at("sticky.reply_to", "sticky reply cycle"));
        }
        let current = project
            .stickies
            .iter()
            .find(|candidate| candidate.id == id)
            .ok_or_else(|| OpError::at("sticky", "unknown sticky"))?;
        match &current.reply_to {
            Some(parent) => id = parent.clone(),
            None => return Ok(id),
        }
    }
}
