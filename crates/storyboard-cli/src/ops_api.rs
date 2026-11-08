use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Map, Value, json};
use storyboard_core::{
    AgentAuthor, Author, ClipSource, Format, Op, OpError, Pin, Project, RenderMode,
    ValidationIssue, compile_active, compile_cut, render_cut_sheet, render_panel,
};
use storyboard_store::{CommitError, StoreError, commit, create, history, open, undo};

use crate::args::{CutSheetArgs, PanelImageArgs};

#[derive(Debug)]
pub enum ApiError {
    MissingBaseVersion,
    Authorship { message: String },
    Usage { message: String },
    Validation { message: String, valid: Vec<String> },
    Op(OpError),
    Commit(CommitError),
    Store(StoreError),
    Render(storyboard_core::RenderError),
    Compile(storyboard_core::CompileError),
}
impl From<OpError> for ApiError {
    fn from(value: OpError) -> Self {
        Self::Op(value)
    }
}
impl From<CommitError> for ApiError {
    fn from(value: CommitError) -> Self {
        Self::Commit(value)
    }
}
impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<storyboard_core::RenderError> for ApiError {
    fn from(value: storyboard_core::RenderError) -> Self {
        Self::Render(value)
    }
}
impl From<storyboard_core::CompileError> for ApiError {
    fn from(value: storyboard_core::CompileError) -> Self {
        Self::Compile(value)
    }
}

pub fn validation(message: impl Into<String>) -> ApiError {
    ApiError::Validation {
        message: message.into(),
        valid: Vec::new(),
    }
}
pub fn options(
    message: impl Into<String>,
    valid: impl IntoIterator<Item = impl Into<String>>,
) -> ApiError {
    ApiError::Validation {
        message: message.into(),
        valid: valid.into_iter().map(Into::into).collect(),
    }
}
pub fn author(value: Option<String>) -> Result<Author, ApiError> {
    let value = value
        .or_else(|| std::env::var("NB_AUTHOR").ok())
        .ok_or_else(|| ApiError::Validation {
            message: "a write needs --author or NB_AUTHOR".into(),
            valid: vec!["human".into(), "agent:NAME".into()],
        })?;
    if value == "human" {
        return Ok(Author::Human);
    }
    let Some(name) = value.strip_prefix("agent:") else {
        return Err(options(
            "author must be human or agent:NAME",
            ["human", "agent:NAME"],
        ));
    };
    if name.is_empty() {
        return Err(validation("agent author name must not be empty"));
    }
    Ok(Author::Agent {
        agent: AgentAuthor {
            name: name.into(),
            extra: Default::default(),
        },
        extra: Default::default(),
    })
}
pub fn create_project(
    dir: &Path,
    title: &str,
    fps: u32,
    width: u32,
    height: u32,
) -> Result<Value, ApiError> {
    refuse_project_ancestor(dir)?;
    let loaded = create(
        dir,
        title,
        fps,
        Format {
            width,
            height,
            extra: Default::default(),
        },
    )?;
    Ok(json!({"version": loaded.version, "title": loaded.project.title}))
}
pub fn commit_ops(
    dir: &Path,
    base_version: &str,
    ops: Vec<Op>,
    author: Author,
    note: Option<String>,
) -> Result<Value, ApiError> {
    for op in &ops {
        op.check_public()?;
        if op
            .sticky_authors()
            .into_iter()
            .any(|sticky_author| sticky_author != &author)
        {
            return Err(ApiError::Authorship {
                message: "a write may only create stickies under its own author".into(),
            });
        }
    }
    let before = project_ids(&open(dir)?.project);
    let record = commit(dir, base_version, ops, author, note)?;
    let after = project_ids(&open(dir)?.project);
    let ids = created_ids(&before, &after);
    Ok(
        json!({ "version": record.version, "summary": record.summary, "rebased": record.rebased, "ids": ids }),
    )
}
pub fn full_write_result(dir: &Path, mut result: Value, full: bool) -> Result<Value, ApiError> {
    if full {
        let loaded = open(dir)?;
        let object = result
            .as_object_mut()
            .ok_or_else(|| validation("write result must be an object"))?;
        object.insert("project".into(), json!(loaded.project));
    }
    Ok(result)
}
pub fn undo_ops(
    dir: &Path,
    base_version: &str,
    author: Author,
    steps: usize,
) -> Result<Value, ApiError> {
    let record = undo(dir, base_version, author, steps)?;
    Ok(json!({ "version": record.version, "summary": record.summary, "rebased": record.rebased }))
}

pub fn parse_pin(value: &str) -> Result<Pin, ApiError> {
    Pin::parse(value).map_err(|message| {
        options(
            message,
            [
                "none",
                "scene:S",
                "shot:H",
                "take:H/T",
                "panel:H/T/P",
                "layer:H/T/P/L",
                "clip:C/K",
            ],
        )
    })
}

pub fn add_panel_op(
    dir: &Path,
    shot: Option<String>,
    take: Option<String>,
    at: Option<&str>,
    id: Option<String>,
    time: Option<f64>,
    index: Option<usize>,
) -> Result<Op, ApiError> {
    let (shot, take) = match (at, shot, take) {
        (Some(at), None, None) => {
            let loaded = open(dir)?;
            let (shot, take, _) = target(&loaded.project, None, None, None, Some(at))?;
            (shot, take)
        }
        (Some(_), _, _) => {
            return Err(validation("--at cannot be combined with --shot or --take"));
        }
        (None, Some(shot), Some(take)) => (shot, take),
        (None, _, _) => return Err(validation("provide --shot and --take, or provide --at")),
    };
    Ok(Op::AddPanel {
        shot,
        take,
        id,
        time,
        index,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn add_text_op(
    dir: &Path,
    shot: Option<String>,
    take: Option<String>,
    panel: Option<String>,
    at: Option<&str>,
    text: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    font_size: f64,
    color: String,
) -> Result<Op, ApiError> {
    add_layer_op(
        dir,
        shot,
        take,
        panel,
        at,
        "text",
        fields([
            ("text", json!(text)),
            ("x", json!(x)),
            ("y", json!(y)),
            ("width", json!(width)),
            ("height", json!(height)),
            ("font_size", json!(font_size)),
            ("color", json!(color)),
        ]),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_image_op(
    dir: &Path,
    shot: Option<String>,
    take: Option<String>,
    panel: Option<String>,
    at: Option<&str>,
    src: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fit: String,
) -> Result<Op, ApiError> {
    add_layer_op(
        dir,
        shot,
        take,
        panel,
        at,
        "image",
        fields([
            ("src", json!(normalize_source_path(src)?)),
            ("x", json!(x)),
            ("y", json!(y)),
            ("width", json!(width)),
            ("height", json!(height)),
            ("fit", json!(fit)),
        ]),
    )
}

fn add_layer_op(
    dir: &Path,
    shot: Option<String>,
    take: Option<String>,
    panel: Option<String>,
    at: Option<&str>,
    layer_type: &str,
    fields: Map<String, Value>,
) -> Result<Op, ApiError> {
    let loaded = open(dir)?;
    let (shot, take, panel) = target(&loaded.project, shot, take, panel, at)?;
    Ok(Op::AddLayer {
        shot,
        take,
        panel,
        id: None,
        layer_type: layer_type.into(),
        fields,
        index: None,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn set_shot_op(
    shot: String,
    title: Option<String>,
    action: Option<String>,
    dialogue: Option<String>,
    speaker: Option<String>,
    camera: Option<String>,
    notes: Option<String>,
    duration: Option<f64>,
) -> Result<Op, ApiError> {
    let mut patch = Map::new();
    for (field, value) in [
        ("title", title.map(Value::String)),
        ("action", action.map(Value::String)),
        ("dialogue", dialogue.map(Value::String)),
        ("speaker", speaker.map(Value::String)),
        ("camera", camera.map(Value::String)),
        ("notes", notes.map(Value::String)),
        ("duration", duration.map(|value| json!(value))),
    ] {
        if let Some(value) = value {
            patch.insert(field.into(), value);
        }
    }
    if patch.is_empty() {
        return Err(validation("set-shot requires at least one field"));
    }
    Ok(Op::UpdateShot { shot, patch })
}

pub fn trim_op(
    cut: String,
    clip: String,
    r#in: Option<u64>,
    out: Option<u64>,
    slot: Option<u64>,
) -> Result<Op, ApiError> {
    let mut patch = Map::new();
    for (field, value) in [("in", r#in), ("out", out), ("slot", slot)] {
        if let Some(value) = value {
            patch.insert(field.into(), json!(value));
        }
    }
    if patch.is_empty() {
        return Err(validation("trim requires in, out, or slot"));
    }
    Ok(Op::UpdateClip { cut, clip, patch })
}

pub fn update_clip_take_op(
    dir: &Path,
    cut: String,
    clip: String,
    take: Option<String>,
) -> Result<Op, ApiError> {
    let take = take.ok_or_else(|| validation("update-clip requires --take"))?;
    let loaded = open(dir)?;
    let shot = loaded
        .project
        .cuts
        .iter()
        .find(|candidate| candidate.id == cut)
        .and_then(|candidate| candidate.clips.iter().find(|item| item.id == clip))
        .and_then(|item| match &item.source {
            ClipSource::Shot(source) => Some(source.shot.clone()),
            ClipSource::Cut(_) => None,
        })
        .ok_or_else(|| validation("--take can only update a shot clip"))?;
    Ok(Op::UpdateClip {
        cut,
        clip,
        patch: fields([("source", json!({"shot": {"shot": shot, "take": take}}))]),
    })
}

pub fn normalize_source_path(src: &str) -> Result<String, ApiError> {
    let stripped = src.strip_prefix("sources/").unwrap_or(src);
    if stripped.is_empty() {
        return Err(validation("src must name a file below sources/"));
    }
    Ok(format!("sources/{stripped}"))
}

pub fn history_result(dir: &Path, limit: usize, full: bool) -> Result<Value, ApiError> {
    let records = history(dir, limit)?;
    if full {
        Ok(json!({"history": records}))
    } else {
        Ok(
            json!({"history": records.iter().map(storyboard_store::CommitSummary::from).collect::<Vec<_>>()}),
        )
    }
}

pub fn notes_result(
    dir: &Path,
    kind: Option<String>,
    author_value: Option<String>,
    pin_value: Option<String>,
    resolved: bool,
    unresolved: bool,
) -> Result<Value, ApiError> {
    if resolved && unresolved {
        return Err(validation("resolved conflicts with unresolved"));
    }
    if let Some(kind) = kind.as_deref()
        && !["note", "todo", "question"].contains(&kind)
    {
        return Err(options(
            "kind must be note, todo, or question",
            ["note", "todo", "question"],
        ));
    }
    let loaded = open(dir)?;
    let author = author_value.map(|value| author(Some(value))).transpose()?;
    let pin = pin_value.map(|value| parse_pin(&value)).transpose()?;
    let notes: Vec<_> = loaded
        .project
        .stickies
        .into_iter()
        .filter(|sticky| sticky.reply_to.is_none())
        .filter(|sticky| kind.as_deref().is_none_or(|kind| sticky.kind == kind))
        .filter(|sticky| {
            author
                .as_ref()
                .is_none_or(|author| sticky.author == *author)
        })
        .filter(|sticky| pin.as_ref().is_none_or(|pin| sticky.pin == *pin))
        .filter(|sticky| !resolved || sticky.resolved)
        .filter(|sticky| !unresolved || !sticky.resolved)
        .collect();
    Ok(json!({"version": loaded.version, "notes": notes}))
}

fn fields<const N: usize>(values: [(&str, Value); N]) -> Map<String, Value> {
    values
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}

pub fn target(
    project: &Project,
    shot: Option<String>,
    take: Option<String>,
    panel: Option<String>,
    at: Option<&str>,
) -> Result<(String, String, String), ApiError> {
    if let Some(at) = at {
        if shot.is_some() || take.is_some() || panel.is_some() {
            return Err(validation(
                "--at cannot be combined with --shot, --take, or --panel",
            ));
        }
        let (position, panel_selector) = at
            .split_once('/')
            .map_or((at, None), |(position, panel)| (position, Some(panel)));
        let position: usize = position
            .parse()
            .map_err(|_| validation("--at starts at 1 and may use POSITION/PANEL"))?;
        let sequence = compile_active(project)?;
        let range = format!("1..={}", sequence.items.len());
        let item = position
            .checked_sub(1)
            .and_then(|index| sequence.items.get(index))
            .ok_or_else(|| {
                options(
                    "--at is beyond the active sequence",
                    [range.as_str(), "POSITION/PANEL"],
                )
            })?;
        let take_object = project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .find(|candidate| candidate.id == item.shot)
            .and_then(|shot| {
                shot.takes
                    .iter()
                    .find(|candidate| candidate.id == item.take)
            })
            .ok_or_else(|| validation("selected sequence item has no take"))?;
        let panel = match panel_selector {
            None => take_object.panels.first(),
            Some(selector) => selector
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| take_object.panels.get(index))
                .or_else(|| {
                    take_object
                        .panels
                        .iter()
                        .find(|candidate| candidate.id == selector)
                }),
        }
        .ok_or_else(|| {
            options(
                "selected sequence item has no matching panel",
                take_object.panels.iter().map(|panel| panel.id.as_str()),
            )
        })?;
        return Ok((item.shot.clone(), item.take.clone(), panel.id.clone()));
    }
    match (shot, take, panel) {
        (Some(shot), Some(take), Some(panel)) => {
            let resolved_panel = project
                .scenes
                .iter()
                .flat_map(|scene| &scene.shots)
                .find(|candidate| candidate.id == shot)
                .and_then(|shot| shot.takes.iter().find(|candidate| candidate.id == take))
                .and_then(|take| {
                    take.panels
                        .iter()
                        .find(|candidate| candidate.id == panel)
                        .map(|panel| panel.id.clone())
                        .or_else(|| {
                            panel
                                .strip_prefix("panel-")
                                .and_then(|index| index.parse::<usize>().ok())
                                .and_then(|index| index.checked_sub(1))
                                .and_then(|index| take.panels.get(index))
                                .map(|panel| panel.id.clone())
                        })
                })
                .unwrap_or(panel);
            Ok((shot, take, resolved_panel))
        }
        _ => Err(validation(
            "provide --shot, --take, and --panel, or provide --at",
        )),
    }
}

pub fn show(dir: &Path, cut: Option<&str>) -> Result<Value, ApiError> {
    let loaded = open(dir)?;
    if let Some(cut) = cut
        && !loaded
            .project
            .cuts
            .iter()
            .any(|candidate| candidate.id == cut)
    {
        let valid: Vec<_> = loaded
            .project
            .cuts
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect();
        return Err(options(
            "unknown cut",
            if valid.is_empty() {
                vec!["no cuts defined"]
            } else {
                valid
            },
        ));
    }
    let sequence = match cut {
        Some(cut) => compile_cut(&loaded.project, cut)?,
        None => compile_active(&loaded.project)?,
    };
    let shots: Vec<_> = sequence.items.iter().enumerate().map(|(index, item)| {
        let shot = loaded.project.scenes.iter().flat_map(|scene| &scene.shots).find(|shot| shot.id == item.shot);
        json!({"position": index + 1, "start": item.start, "end": item.end, "shot": item.shot, "take": item.take, "title": shot.map(|shot| shot.title.clone()).unwrap_or_default(), "duration": shot.map(|shot| shot.duration).unwrap_or_default(), "offset": item.offset, "clip": item.clip})
    }).collect();
    let scenes: Vec<_> = loaded.project.scenes.iter().map(|scene| json!({"id": scene.id, "title": scene.title, "shots": scene.shots.iter().map(|shot| json!({"id": shot.id, "title": shot.title, "duration": shot.duration, "action": shot.action, "dialogue": shot.dialogue, "notes": shot.notes, "selected_take": shot.selected_take, "takes": shot.takes.iter().map(|take| json!({"id": take.id, "name": take.name, "panels": take.panels.len()})).collect::<Vec<_>>() })).collect::<Vec<_>>() })).collect();
    let warnings: Vec<_> = sequence
        .warnings
        .iter()
        .map(|warning| json!({"field": warning.field, "message": warning.message}))
        .collect();
    Ok(
        json!({"version": loaded.version, "cut": cut, "frames": sequence.frames, "scenes": scenes, "shots": shots, "stickies": loaded.project.stickies, "positions": loaded.project.layout.positions, "warnings": warnings}),
    )
}

pub fn panel_image(args: PanelImageArgs) -> Result<Value, ApiError> {
    panel_image_with_root(args, None)
}

pub fn panel_image_in_workspace(args: PanelImageArgs, workspace: &Path) -> Result<Value, ApiError> {
    panel_image_with_root(args, Some(workspace))
}

fn panel_image_with_root(
    args: PanelImageArgs,
    workspace: Option<&Path>,
) -> Result<Value, ApiError> {
    let loaded = open(&args.dir)?;
    let (shot, take, panel) = target(
        &loaded.project,
        args.shot,
        args.take,
        args.panel,
        args.at.as_deref(),
    )?;
    let mode = match args.mode.as_str() {
        "stage" => RenderMode::Stage,
        "board" => RenderMode::Board,
        _ => return Err(options("mode must be stage or board", ["stage", "board"])),
    };
    let rendered = render_panel(
        &loaded.project,
        &args.dir,
        &shot,
        &take,
        &panel,
        args.width,
        mode,
    )?;
    let path = args.out.unwrap_or_else(|| {
        args.dir.join(".cache").join(format!(
            "panel-{}-{}-{}.png",
            encode_id(&shot),
            encode_id(&take),
            encode_id(&panel)
        ))
    });
    let path = write_render_png(&args.dir, &path, workspace, &rendered.png)?;
    Ok(
        json!({"path": path, "width": rendered.width, "height": rendered.height, "warnings": rendered.warnings}),
    )
}

pub fn cut_sheet(args: CutSheetArgs) -> Result<Value, ApiError> {
    cut_sheet_with_root(args, None)
}

pub fn cut_sheet_in_workspace(args: CutSheetArgs, workspace: &Path) -> Result<Value, ApiError> {
    cut_sheet_with_root(args, Some(workspace))
}

fn cut_sheet_with_root(args: CutSheetArgs, workspace: Option<&Path>) -> Result<Value, ApiError> {
    let loaded = open(&args.dir)?;
    if let Some(cut) = args.cut.as_deref()
        && !loaded
            .project
            .cuts
            .iter()
            .any(|candidate| candidate.id == cut)
    {
        let valid: Vec<_> = loaded
            .project
            .cuts
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect();
        return Err(options(
            "unknown cut",
            if valid.is_empty() {
                vec!["no cuts defined"]
            } else {
                valid
            },
        ));
    }
    let sequence = match args.cut.as_deref() {
        Some(cut) => compile_cut(&loaded.project, cut)?,
        None => compile_active(&loaded.project)?,
    };
    let rendered = render_cut_sheet(&loaded.project, &args.dir, &sequence, args.width)?;
    let path = write_render_png(&args.dir, &args.out, workspace, &rendered.png)?;
    Ok(
        json!({"path": path, "width": rendered.width, "height": rendered.height, "warnings": rendered.warnings}),
    )
}

pub fn schema(op: Option<&str>) -> Result<Value, ApiError> {
    let schema = serde_json::to_value(schemars::schema_for!(Op))
        .map_err(|error| validation(error.to_string()))?;
    if let Some(op) = op {
        let op = op.replace('-', "_");
        let variants = schema
            .get("oneOf")
            .and_then(Value::as_array)
            .ok_or_else(|| validation("operation schema has no variants"))?;
        let shape = variants
            .iter()
            .find(|shape| {
                shape
                    .get("properties")
                    .and_then(|properties| properties.get(&op))
                    .is_some()
            })
            .cloned()
            .ok_or_else(|| options("unknown operation", op_names()))?;
        return Ok(json!({"op": shape}));
    }
    let common = serde_json::to_value(schemars::schema_for!(storyboard_core::Layer))
        .map_err(|error| validation(error.to_string()))?;
    let layer = |required: Vec<&str>, properties: Value| {
        json!({
            "allOf": [common.clone()],
            "required": required,
            "properties": properties,
        })
    };
    let layers = json!({
        "image": layer(vec!["src"], json!({"src": {"type": "string"}, "fit": {"enum": ["contain", "cover", "fill"]}, "crop": {"type": "object"}})),
        "text": layer(vec!["text", "font_size", "color"], json!({"text": {"type": "string"}, "font_size": {"type": "number"}, "weight": {"enum": ["regular", "bold"]}, "align": {"enum": ["left", "center", "right"]}, "color": {"type": "string"}})),
        "solid": layer(vec!["fill"], json!({"fill": {"type": "string"}})),
        "box": layer(Vec::new(), json!({})),
    });
    Ok(json!({"ops": schema, "layers": layers}))
}

pub fn pretty_show(value: &Value) -> String {
    let mut output = String::from("SCENES\n");
    for scene in value
        .get("scenes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "\n{}  {}\n",
            scene["id"].as_str().unwrap_or("scene"),
            scene["title"].as_str().unwrap_or("")
        ));
        for shot in scene
            .get("shots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            output.push_str(&format!(
                "  {}  {} ({:.2}s)\n",
                shot["id"].as_str().unwrap_or("shot"),
                shot["title"].as_str().unwrap_or(""),
                shot["duration"].as_f64().unwrap_or_default()
            ));
            if let Some(action) = shot["action"].as_str().filter(|value| !value.is_empty()) {
                output.push_str(&format!("    action: {action}\n"));
            }
            if let Some(dialogue) = shot["dialogue"].as_str().filter(|value| !value.is_empty()) {
                output.push_str(&format!("    dialogue: {dialogue}\n"));
            }
            if let Some(notes) = shot["notes"].as_str().filter(|value| !value.is_empty()) {
                output.push_str(&format!("    notes: {notes}\n"));
            }
            for take in shot
                .get("takes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                output.push_str(&format!(
                    "    take {}  {} ({} panels)\n",
                    take["id"].as_str().unwrap_or("take"),
                    take["name"].as_str().unwrap_or(""),
                    take["panels"].as_u64().unwrap_or_default()
                ));
            }
        }
    }
    let cut = value.get("cut").and_then(Value::as_str);
    output.push_str("\nSEQUENCE");
    if let Some(cut) = cut {
        output.push_str(&format!(" ({cut})"));
    }
    output.push('\n');
    for shot in value
        .get("shots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "  {}  {} / {}  frames {}..{}\n",
            shot["position"].as_u64().unwrap_or_default(),
            shot["shot"].as_str().unwrap_or("shot"),
            shot["take"].as_str().unwrap_or("take"),
            shot["start"].as_u64().unwrap_or_default(),
            shot["end"].as_u64().unwrap_or_default(),
        ));
    }
    output.push_str("STICKIES\n");
    for sticky in value
        .get("stickies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "  {}  {}{}\n",
            sticky["id"].as_str().unwrap_or("sticky"),
            sticky["text"].as_str().unwrap_or(""),
            if sticky["resolved"].as_bool().unwrap_or(false) {
                " (resolved)"
            } else {
                ""
            }
        ));
    }
    output
}

fn op_names() -> Vec<&'static str> {
    vec![
        "add_scene",
        "remove_scene",
        "move_scene",
        "update_scene",
        "add_shot",
        "remove_shot",
        "move_shot",
        "update_shot",
        "add_take",
        "remove_take",
        "move_take",
        "update_take",
        "select_take",
        "duplicate_take",
        "add_panel",
        "remove_panel",
        "move_panel",
        "update_panel",
        "add_layer",
        "remove_layer",
        "move_layer",
        "update_layer",
        "add_sticky",
        "remove_sticky",
        "move_sticky",
        "update_sticky",
        "add_clip",
        "remove_clip",
        "reorder_clip",
        "update_clip",
        "add_cut",
        "update_cut",
        "remove_cut",
        "set_active_cut",
        "move_shot_between_scenes",
        "add_ref",
        "remove_ref",
        "resolve_sticky",
        "reply_sticky",
        "move_node",
    ]
}

struct CheckedRenderPath {
    target: PathBuf,
    parent: PathBuf,
    parent_identity: FileIdentity,
}

static RENDER_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_render_png(
    project: &Path,
    requested: &Path,
    workspace: Option<&Path>,
    png: &[u8],
) -> Result<PathBuf, ApiError> {
    let checked = checked_render_path(project, requested, workspace)?;
    let parent = open_directory(&checked.parent)?;
    let opened_parent_identity = file_identity(&parent, &checked.parent)?;
    if opened_parent_identity != checked.parent_identity {
        return Err(validation("render output parent changed during validation"));
    }

    let (mut pending, mut file) = create_render_temp(&checked.parent)?;
    file.write_all(png).map_err(StoreError::from)?;
    file.sync_all().map_err(StoreError::from)?;
    if path_identity(&checked.parent)? != opened_parent_identity {
        return Err(validation("render output parent changed before rename"));
    }
    if path_identity(pending.path())? != file_identity(&file, pending.path())? {
        return Err(validation("render temporary file changed before rename"));
    }
    fs::rename(pending.path(), &checked.target).map_err(StoreError::from)?;
    pending.persisted();
    Ok(checked.target)
}

fn checked_render_path(
    project: &Path,
    requested: &Path,
    workspace: Option<&Path>,
) -> Result<CheckedRenderPath, ApiError> {
    let current_dir = std::env::current_dir().map_err(StoreError::from)?;
    let relative_root = workspace.unwrap_or(&current_dir);
    let requested = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        relative_root.join(requested)
    };
    let project_input = if project.is_absolute() {
        project.to_path_buf()
    } else {
        current_dir.join(project)
    };
    let project = fs::canonicalize(&project_input).map_err(StoreError::from)?;
    let workspace = workspace
        .map(fs::canonicalize)
        .transpose()
        .map_err(StoreError::from)?;
    let parent = requested
        .parent()
        .ok_or_else(|| validation("render output has no parent directory"))?;
    let existing_parent = nearest_existing_ancestor(parent)?;
    let real_existing_parent = fs::canonicalize(existing_parent).map_err(StoreError::from)?;
    let cache = project.join(".cache");
    let cache_is_real = cache
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
    let real_cache = cache_is_real
        .then(|| fs::canonicalize(&cache))
        .transpose()
        .map_err(StoreError::from)?;
    if !cache_is_real && requested.starts_with(project_input.join(".cache")) {
        return Err(validation("render output parent must not be a symlink"));
    }
    validate_render_parent(
        &real_existing_parent,
        &project,
        real_cache.as_deref(),
        workspace.as_deref(),
    )?;
    if existing_output_aliases_protected_file(&project, &requested)? {
        return Err(validation(
            "render output must not overwrite project.json, history/, or sources/",
        ));
    }
    fs::create_dir_all(parent).map_err(StoreError::from)?;
    let real_parent = fs::canonicalize(parent).map_err(StoreError::from)?;
    validate_render_parent(
        &real_parent,
        &project,
        real_cache.as_deref(),
        workspace.as_deref(),
    )?;
    if existing_output_aliases_protected_file(&project, &requested)? {
        return Err(validation(
            "render output must not overwrite project.json, history/, or sources/",
        ));
    }
    let file_name = requested
        .components()
        .next_back()
        .and_then(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .ok_or_else(|| validation("render output must end with a file name"))?;
    let target = real_parent.join(file_name);
    Ok(CheckedRenderPath {
        target,
        parent_identity: path_identity(&real_parent)?,
        parent: real_parent,
    })
}

fn nearest_existing_ancestor(path: &Path) -> Result<&Path, ApiError> {
    path.ancestors()
        .find(|candidate| candidate.symlink_metadata().is_ok())
        .ok_or_else(|| validation("render output has no existing parent"))
}

fn refuse_project_ancestor(dir: &Path) -> Result<(), ApiError> {
    let current_dir = std::env::current_dir().map_err(StoreError::from)?;
    let requested = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        current_dir.join(dir)
    };
    let existing = nearest_existing_ancestor(&requested)?;
    let real_existing = fs::canonicalize(existing).map_err(StoreError::from)?;
    if let Some(project) = real_existing.ancestors().find(|ancestor| {
        ancestor
            .join("project.json")
            .metadata()
            .is_ok_and(|metadata| metadata.is_file())
    }) {
        return Err(validation(format!(
            "new project must not be inside existing project {}",
            project.display()
        )));
    }
    Ok(())
}

struct PendingRender {
    path: Option<PathBuf>,
}

impl PendingRender {
    fn path(&self) -> &Path {
        self.path.as_deref().expect("pending render has a path")
    }

    fn persisted(&mut self) {
        self.path = None;
    }
}

impl Drop for PendingRender {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn create_render_temp(parent: &Path) -> Result<(PendingRender, File), ApiError> {
    loop {
        let nonce = RENDER_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".nb-storyboard-render-{}-{nonce}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                return Ok((PendingRender { path: Some(path) }, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(StoreError::from(error).into()),
        }
    }
}

#[cfg(not(windows))]
fn open_directory(path: &Path) -> Result<File, ApiError> {
    File::open(path).map_err(|error| StoreError::from(error).into())
}

#[cfg(windows)]
fn open_directory(path: &Path) -> Result<File, ApiError> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| StoreError::from(error).into())
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
fn identity(metadata: &fs::Metadata, _path: &Path) -> Result<FileIdentity, ApiError> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    volume: Option<u32>,
    index: Option<u64>,
}

#[cfg(windows)]
fn identity(metadata: &fs::Metadata, _path: &Path) -> Result<FileIdentity, ApiError> {
    use std::os::windows::fs::MetadataExt;

    Ok(FileIdentity {
        volume: metadata.volume_serial_number(),
        index: metadata.file_index(),
    })
}

#[cfg(not(any(unix, windows)))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity(PathBuf);

#[cfg(not(any(unix, windows)))]
fn identity(_metadata: &fs::Metadata, path: &Path) -> Result<FileIdentity, ApiError> {
    Ok(FileIdentity(
        fs::canonicalize(path).map_err(StoreError::from)?,
    ))
}

fn path_identity(path: &Path) -> Result<FileIdentity, ApiError> {
    let metadata = fs::metadata(path).map_err(StoreError::from)?;
    identity(&metadata, path)
}

fn file_identity(file: &File, path: &Path) -> Result<FileIdentity, ApiError> {
    let metadata = file.metadata().map_err(StoreError::from)?;
    identity(&metadata, path)
}

fn validate_render_parent(
    parent: &Path,
    project: &Path,
    cache: Option<&Path>,
    workspace: Option<&Path>,
) -> Result<(), ApiError> {
    if let Some(workspace) = workspace
        && !has_ancestor_identity(parent, workspace)?
    {
        return Err(validation(
            "render output must stay inside the MCP workspace",
        ));
    }
    for ancestor in parent.ancestors() {
        let document = ancestor.join("project.json");
        if !document.metadata().is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        let named_project = same_filesystem_identity(ancestor, project)?;
        let in_named_cache = cache
            .map(|cache| has_ancestor_identity(parent, cache))
            .transpose()?
            .unwrap_or(false);
        if !named_project || !in_named_cache {
            return Err(validation(
                "render output inside a project must be under the named project's real .cache/ directory",
            ));
        }
    }
    Ok(())
}

fn has_ancestor_identity(path: &Path, expected: &Path) -> Result<bool, ApiError> {
    for ancestor in path.ancestors() {
        if same_filesystem_identity(ancestor, expected)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn same_filesystem_identity(left: &Path, right: &Path) -> Result<bool, ApiError> {
    use std::os::unix::fs::MetadataExt;

    let left = fs::metadata(left).map_err(StoreError::from)?;
    let right = fs::metadata(right).map_err(StoreError::from)?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(not(unix))]
fn same_filesystem_identity(left: &Path, right: &Path) -> Result<bool, ApiError> {
    Ok(fs::canonicalize(left).map_err(StoreError::from)?
        == fs::canonicalize(right).map_err(StoreError::from)?)
}

fn existing_output_aliases_protected_file(project: &Path, output: &Path) -> Result<bool, ApiError> {
    if output.symlink_metadata().is_err() {
        return Ok(false);
    }
    if same_filesystem_identity(output, &project.join("project.json"))? {
        return Ok(true);
    }
    for directory in [project.join("history"), project.join("sources")] {
        if aliases_file_under(output, &directory)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn aliases_file_under(output: &Path, directory: &Path) -> Result<bool, ApiError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(StoreError::from(error).into()),
    };
    for entry in entries {
        let entry = entry.map_err(StoreError::from)?;
        let path = entry.path();
        let metadata = path.symlink_metadata().map_err(StoreError::from)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if aliases_file_under(output, &path)? {
                return Ok(true);
            }
        } else if metadata.is_file() && same_filesystem_identity(output, &path)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn encode_id(id: &str) -> String {
    id.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn project_ids(project: &Project) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for scene in &project.scenes {
        ids.insert(scene.id.clone());
        for shot in &scene.shots {
            ids.insert(shot.id.clone());
            for take in &shot.takes {
                ids.insert(take.id.clone());
                for panel in &take.panels {
                    ids.insert(panel.id.clone());
                    for layer in &panel.layers {
                        ids.insert(layer.id.clone());
                    }
                }
            }
        }
    }
    for cut in &project.cuts {
        ids.insert(cut.id.clone());
        for clip in &cut.clips {
            ids.insert(clip.id.clone());
        }
    }
    for sticky in &project.stickies {
        ids.insert(sticky.id.clone());
    }
    ids
}
fn created_ids(before: &BTreeSet<String>, after: &BTreeSet<String>) -> Vec<String> {
    after.difference(before).cloned().collect()
}
pub fn map_error(error: ApiError) -> Value {
    let (kind, message, hint, valid, issues) = match error {
        ApiError::MissingBaseVersion => (
            "validation",
            "a write requires --base-version".into(),
            "run `storyboard show PROJECT` and copy its `version` value".into(),
            Vec::new(),
            Vec::new(),
        ),
        ApiError::Authorship { message } => (
            "authorship",
            message,
            "create stickies with the active author only".into(),
            Vec::new(),
            Vec::new(),
        ),
        ApiError::Usage { message } => (
            "usage",
            message,
            "run `storyboard <verb> --help` for usage".into(),
            Vec::new(),
            Vec::new(),
        ),
        ApiError::Validation { message, valid } => (
            "validation",
            message,
            "correct the command and try again".into(),
            valid,
            Vec::new(),
        ),
        ApiError::Op(error) => (
            "validation",
            error.message,
            "correct the named field and try again".into(),
            error.valid,
            issue_values(error.issues),
        ),
        ApiError::Store(StoreError::Invalid(issues)) => (
            "validation",
            "project validation failed".into(),
            "correct every reported field".into(),
            Vec::new(),
            validation_values(issues),
        ),
        ApiError::Store(error) => (
            "store",
            error.to_string(),
            "check the project folder and retry".into(),
            Vec::new(),
            Vec::new(),
        ),
        ApiError::Commit(CommitError::EmptyCommit) => (
            "empty_commit",
            "empty commit".into(),
            "supply at least one operation".into(),
            Vec::new(),
            Vec::new(),
        ),
        ApiError::Commit(CommitError::Op(error)) => (
            "validation",
            error.message,
            "correct the named field and try again".into(),
            error.valid,
            issue_values(error.issues),
        ),
        ApiError::Commit(CommitError::Store(StoreError::Invalid(issues))) => (
            "validation",
            "project validation failed".into(),
            "correct every reported field".into(),
            Vec::new(),
            validation_values(issues),
        ),
        ApiError::Commit(CommitError::Conflict {
            current_version,
            reason,
            intervening,
        }) => {
            let no_undo = reason == "not enough commits by this author to undo";
            (
                if no_undo { "not_found" } else { "conflict" },
                reason,
                if no_undo {
                    "use storyboard history to find a commit by this author".into()
                } else {
                    "read the current version and retry the edit".into()
                },
                Vec::new(),
                vec![json!({"current_version": current_version, "intervening": intervening})],
            )
        }
        ApiError::Commit(error) => (
            "commit",
            error.to_string(),
            "check the project and retry".into(),
            Vec::new(),
            Vec::new(),
        ),
        ApiError::Render(error) => ("render", error.message, error.hint, error.valid, Vec::new()),
        ApiError::Compile(error) => (
            "compile",
            error.to_string(),
            "correct the sequence references and try again".into(),
            Vec::new(),
            Vec::new(),
        ),
    };
    let mut body = serde_json::Map::new();
    body.insert("kind".into(), Value::String(kind.into()));
    body.insert("message".into(), Value::String(message));
    body.insert("hint".into(), Value::String(hint));
    body.insert("valid".into(), json!(valid));
    if !issues.is_empty() {
        body.insert("issues".into(), Value::Array(issues));
    }
    json!({"error": Value::Object(body)})
}
fn validation_values(issues: Vec<ValidationIssue>) -> Vec<Value> {
    issues
        .into_iter()
        .map(|issue| json!({"field": issue.field, "message": issue.message, "valid": issue.valid}))
        .collect()
}
fn issue_values(issues: Vec<storyboard_core::OpIssue>) -> Vec<Value> {
    issues
        .into_iter()
        .map(|issue| json!({"field": issue.field, "message": issue.message, "valid": issue.valid}))
        .collect()
}
