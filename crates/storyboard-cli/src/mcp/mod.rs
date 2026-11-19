//! The stdio MCP surface is intentionally only a transport over `ops_api`.

use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::CommandFactory;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};
use storyboard_core::{Author, ClipSource, CutSource, Op, ShotSource};
use storyboard_store::import_source;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    args::*,
    commands, movie_export, movie_import,
    ops_api::{self, ApiError},
};

const AGREEMENT: &str = "Read with show, notes, history, and schema before changing a project. Writes use the base_version returned by a read or write; send small edits and then read the result. Stickies are the shared note channel: use note, reply, and resolve; an agent can only create stickies under its own client identity.";
const TOOLS: [(&str, &str); 45] = [
    ("new", "Create an empty storyboard project."),
    ("index", "List projects in this workspace."),
    ("add_scene", "Add a scene to a project."),
    ("add_shot", "Add a shot with its first take and panel."),
    (
        "import_shot",
        "Import numbered image files as one timed shot.",
    ),
    ("remove_scene", "Remove a scene."),
    ("remove_shot", "Remove a shot."),
    ("add_panel", "Add a panel to a take."),
    ("remove_panel", "Remove a panel from a take."),
    ("add_text", "Add a text layer to a panel."),
    ("add_image", "Add an image layer to a panel."),
    ("remove_layer", "Remove a layer from a panel."),
    ("import", "Copy a file into sources."),
    ("sources", "List scanned sources."),
    ("add_take", "Add a take to a shot."),
    ("add_cut", "Add an alternative cut."),
    ("update_cut", "Rename a cut."),
    ("remove_cut", "Remove a cut."),
    ("add_clip", "Add a shot or nested-cut clip."),
    ("update_clip", "Update a clip take."),
    ("remove_clip", "Remove a clip."),
    ("reorder_clip", "Reorder a clip within a cut."),
    ("set_active_cut", "Choose the active cut or story order."),
    ("set_shot", "Update shot story fields."),
    (
        "replace_movie",
        "Probe and attach a generated MP4 to a shot through an undoable edit.",
    ),
    ("move_shot_to_scene", "Move a shot into a scene."),
    ("reorder_shot", "Reorder a shot within its scene."),
    ("add_ref", "Attach a source reference to a shot or take."),
    (
        "remove_ref",
        "Detach a source reference from a shot or take.",
    ),
    ("select_take", "Choose a shot take."),
    ("trim", "Set a clip trim or slot."),
    ("undo", "Undo this author's recent commits."),
    ("redo", "Redo this author's recently undone commits."),
    ("history", "Read project history."),
    ("show", "Read the active sequence."),
    (
        "describe",
        "Describe a panel, its layers, timing, and notes.",
    ),
    ("panel_image", "Render one panel as an image."),
    ("cut_sheet", "Render a sequence contact sheet."),
    (
        "export_movie",
        "Render a cut as an H.264/AAC MP4 from source video or storyboard panels.",
    ),
    ("note", "Add a sticky note."),
    ("reply", "Reply to a sticky note."),
    ("resolve", "Resolve a sticky note."),
    ("notes", "List sticky notes."),
    ("schema", "Read public operation and layer schemas."),
    ("apply", "Apply public operations when no named tool fits."),
];

#[allow(dead_code)]
#[derive(JsonSchema, Serialize)]
struct ApplyArgs {
    /// Project folder.
    project: PathBuf,
    /// Version read before this edit.
    base_version: String,
    /// Public operations to apply in order.
    ops: Vec<Op>,
    /// Optional history note for this write.
    note: Option<String>,
    /// Return the full project after the write.
    full: Option<bool>,
}

fn input_schema(name: &str) -> Value {
    match name {
        "new" => schema_for_args::<NewArgs>(name, false),
        "index" => schema_for_args::<IndexArgs>(name, false),
        "add_scene" => schema_for_args::<AddSceneArgs>(name, true),
        "add_shot" => schema_for_args::<AddShotArgs>(name, true),
        "import_shot" => schema_for_args::<ImportShotArgs>(name, true),
        "remove_scene" => schema_for_args::<RemoveSceneArgs>(name, true),
        "remove_shot" => schema_for_args::<RemoveShotArgs>(name, true),
        "add_panel" => schema_for_args::<AddPanelArgs>(name, true),
        "remove_panel" => schema_for_args::<RemovePanelArgs>(name, true),
        "add_text" => schema_for_args::<AddTextArgs>(name, true),
        "add_image" => schema_for_args::<AddImageArgs>(name, true),
        "remove_layer" => schema_for_args::<RemoveLayerArgs>(name, true),
        "import" => schema_for_args::<ImportArgs>(name, false),
        "sources" => schema_for_args::<SourcesArgs>(name, false),
        "add_take" => schema_for_args::<AddTakeArgs>(name, true),
        "add_cut" => schema_for_args::<AddCutArgs>(name, true),
        "update_cut" => schema_for_args::<UpdateCutArgs>(name, true),
        "remove_cut" => schema_for_args::<RemoveCutArgs>(name, true),
        "add_clip" => schema_for_args::<AddClipArgs>(name, true),
        "update_clip" => schema_for_args::<UpdateClipArgs>(name, true),
        "remove_clip" => schema_for_args::<RemoveClipArgs>(name, true),
        "reorder_clip" => schema_for_args::<ReorderClipArgs>(name, true),
        "set_active_cut" => schema_for_args::<SetActiveCutArgs>(name, true),
        "set_shot" => schema_for_args::<SetShotArgs>(name, true),
        "replace_movie" => schema_for_args::<ReplaceMovieArgs>(name, true),
        "move_shot_to_scene" => schema_for_args::<MoveShotToSceneArgs>(name, true),
        "reorder_shot" => schema_for_args::<ReorderShotArgs>(name, true),
        "add_ref" | "remove_ref" => schema_for_args::<RefArgs>(name, true),
        "select_take" => schema_for_args::<SelectTakeArgs>(name, true),
        "trim" => schema_for_args::<TrimArgs>(name, true),
        "undo" => schema_for_args::<UndoArgs>(name, true),
        "redo" => schema_for_args::<RedoArgs>(name, true),
        "history" => schema_for_args::<HistoryArgs>(name, false),
        "show" => schema_for_args::<ShowArgs>(name, false),
        "describe" => schema_for_args::<DescribeArgs>(name, false),
        "panel_image" => schema_for_args::<PanelImageArgs>(name, false),
        "cut_sheet" => schema_for_args::<CutSheetArgs>(name, false),
        "export_movie" => schema_for_args::<ExportMovieArgs>(name, false),
        "note" => schema_for_args::<NoteArgs>(name, true),
        "reply" => schema_for_args::<ReplyArgs>(name, true),
        "resolve" => schema_for_args::<ResolveArgs>(name, true),
        "notes" => schema_for_args::<NotesArgs>(name, false),
        "schema" => schema_for_args::<SchemaArgs>(name, false),
        "apply" => {
            let mut schema = serde_json::to_value(schemars::schema_for!(ApplyArgs))
                .expect("ApplyArgs schema must serialize");
            schema
                .as_object_mut()
                .expect("apply schema is an object")
                .insert("additionalProperties".into(), Value::Bool(false));
            schema
        }
        _ => unreachable!("tool list and schema dispatch must match"),
    }
}

fn schema_for_args<T: JsonSchema>(name: &str, write: bool) -> Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(T)).expect("argument schema must serialize");
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("argument schema must be an object");
    if let Some(project) = properties.remove("dir") {
        properties.insert("project".into(), project);
    }
    if write {
        properties.remove("author");
    }
    if properties.contains_key("at") {
        properties.insert(
            "at".into(),
            json!({
                "description": "Sequence position, optionally followed by /PANEL.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer", "minimum": 1}
                ]
            }),
        );
    }

    let cli_name = name.replace('_', "-");
    let cli = Cli::command();
    let command = cli
        .find_subcommand(&cli_name)
        .expect("each MCP tool must mirror a CLI command");
    let mut required = Vec::new();
    for argument in command.get_arguments() {
        let id = argument.get_id().as_str();
        if (id == "author" && write) || id == "pretty" {
            continue;
        }
        let property_name = if id == "dir" { "project" } else { id };
        if let Some(property) = properties
            .get_mut(property_name)
            .and_then(Value::as_object_mut)
            && let Some(help) = argument.get_help()
        {
            property.insert("description".into(), Value::String(help.to_string()));
        }
        if argument.is_required_set() {
            required.push(Value::String(property_name.to_owned()));
        }
    }
    if write {
        properties.insert(
            "full".into(),
            json!({"type":"boolean","description":"Return the full project after the write"}),
        );
    }
    let object = schema.as_object_mut().expect("schema object");
    object.insert("required".into(), Value::Array(required));
    object.insert("additionalProperties".into(), Value::Bool(false));
    schema
}

pub fn run(workspace: &Path) -> Result<(), ApiError> {
    let workspace = workspace
        .canonicalize()
        .map_err(|e| ApiError::Store(e.into()))?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut author = String::from("agent");
    for line in stdin.lock().lines() {
        let request: Value = match line {
            Ok(line) => match serde_json::from_str(&line) {
                Ok(request) => request,
                Err(_) => {
                    serde_json::to_writer(
                        &mut stdout,
                        &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
                    )
                    .map_err(|e| ops_api::validation(e.to_string()))?;
                    writeln!(stdout).map_err(|e| ApiError::Store(e.into()))?;
                    stdout.flush().map_err(|e| ApiError::Store(e.into()))?;
                    continue;
                }
            },
            Err(error) => return Err(ApiError::Store(error.into())),
        };
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        if !matches!(
            method,
            "initialize" | "notifications/initialized" | "ping" | "tools/list" | "tools/call"
        ) {
            if let Some(id) = id {
                let response = json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "error":{"code":-32601,"message":"Method not found"}
                });
                serde_json::to_writer(&mut stdout, &response)
                    .map_err(|e| ops_api::validation(e.to_string()))?;
                writeln!(stdout).map_err(|e| ApiError::Store(e.into()))?;
                stdout.flush().map_err(|e| ApiError::Store(e.into()))?;
            }
            continue;
        }
        let result = match method {
            "initialize" => {
                author = params
                    .pointer("/clientInfo/name")
                    .and_then(Value::as_str)
                    .map(str::to_lowercase)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "agent".into());
                Ok(
                    json!({"protocolVersion":"2025-06-18", "capabilities":{"tools":{"listChanged":false}}, "serverInfo":{"name":"storyboard","version":env!("CARGO_PKG_VERSION")}, "instructions":AGREEMENT}),
                )
            }
            "notifications/initialized" | "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": TOOLS.into_iter().filter(|(name, _)| !single_project_mode() || *name != "new").map(|(name, description)| {
                    json!({"name":name,"description":description,"inputSchema":input_schema(name)})
                }).collect::<Vec<_>>()
            })),
            "tools/call" => call(&workspace, &author, params),
            _ => unreachable!("known MCP methods were checked above"),
        };
        if let Some(id) = id {
            let response = match result {
                Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                Err(error) => json!({"jsonrpc":"2.0","id":id,"result":error_result(error)}),
            };
            serde_json::to_writer(&mut stdout, &response)
                .map_err(|e| ops_api::validation(e.to_string()))?;
            writeln!(stdout).map_err(|e| ApiError::Store(e.into()))?;
            stdout.flush().map_err(|e| ApiError::Store(e.into()))?;
        }
    }
    Ok(())
}

fn call(workspace: &Path, author: &str, params: Value) -> Result<Value, ApiError> {
    let name = required_value(&params, "name")?;
    if !TOOLS.iter().any(|(candidate, _)| *candidate == name) {
        return Err(ops_api::validation(format!("unknown tool {name}")));
    }
    let input = params
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or_else(|| ops_api::validation("tool arguments must be an object"))?;
    validate_tool_arguments(name, input)?;
    match dispatch(workspace, author, name, input)? {
        Output::Text(value) => Ok(text(value, false)),
        Output::Image(value, bytes) => Ok(
            json!({"content":[{"type":"image","data":STANDARD.encode(bytes),"mimeType":"image/png"},{"type":"text","text":value.to_string()}]}),
        ),
    }
}

fn validate_tool_arguments(name: &str, input: &Map<String, Value>) -> Result<(), ApiError> {
    let schema = input_schema(name);
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| ops_api::validation("tool schema has no properties"))?;
    let unknown = input
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(ops_api::validation(format!(
            "unknown argument{}: {}",
            if unknown.len() == 1 { "" } else { "s" },
            unknown.join(", ")
        )))
    }
}

fn error_result(error: ApiError) -> Value {
    json!({"content":[{"type":"text","text":ops_api::map_error(error).to_string()}],"isError":true})
}
fn text(value: Value, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"isError":is_error})
}
enum Output {
    Text(Value),
    Image(Value, Vec<u8>),
}

fn dispatch(
    workspace: &Path,
    client: &str,
    name: &str,
    input: &Map<String, Value>,
) -> Result<Output, ApiError> {
    let result = match name {
        "new" => {
            let dir = project_destination(workspace, path(input, "project")?)?;
            ops_api::create_project(
                &dir,
                required(input, "title")?,
                number(input, "fps", 24)?,
                number(input, "width", 1440)?,
                number(input, "height", 900)?,
            )
        }
        "index" => commands::workspace_index(workspace),
        "add_scene" => write(
            workspace,
            input,
            client,
            Op::AddScene {
                id: opt(input, "id"),
                title: required(input, "title")?.into(),
                index: opt_num(input, "index")?,
            },
        ),
        "add_shot" => write(
            workspace,
            input,
            client,
            Op::AddShot {
                scene: required(input, "scene")?.into(),
                id: opt(input, "id"),
                title: required(input, "title")?.into(),
                index: opt_num(input, "index")?,
            },
        ),
        "import_shot" => {
            let images: Vec<PathBuf> = serde_json::from_value(
                input
                    .get("images")
                    .cloned()
                    .ok_or_else(|| ops_api::validation("images are required"))?,
            )
            .map_err(|_| ops_api::validation("images must be file paths"))?;
            commands::run(Command::ImportShot(ImportShotArgs {
                write: WriteArgs {
                    dir: project(workspace, input)?,
                    base_version: required(input, "base_version")?.into(),
                    author: Some(format!("agent:{client}")),
                    note: opt(input, "note"),
                },
                scene: required(input, "scene")?.into(),
                title: required(input, "title")?.into(),
                duration: opt_num(input, "duration")?,
                index: opt_num(input, "index")?,
                images,
            }))
        }
        "remove_scene" => write(
            workspace,
            input,
            client,
            ops_api::remove_scene_op(required(input, "scene")?.into()),
        ),
        "remove_shot" => write(
            workspace,
            input,
            client,
            ops_api::remove_shot_op(required(input, "shot")?.into()),
        ),
        "add_panel" => add_panel(workspace, input, client),
        "remove_panel" => remove_panel(workspace, input, client),
        "add_text" => add_layer(workspace, input, client, "text"),
        "add_image" => add_layer(workspace, input, client, "image"),
        "remove_layer" => remove_layer(workspace, input, client),
        "import" => {
            let dir = project(workspace, input)?;
            Ok(
                json!({"path":import_source(&dir,&path(input,"source")?,&PathBuf::from("sources").join(path(input,"destination")?))?}),
            )
        }
        "sources" => ops_api::sources_result(&project(workspace, input)?),
        "add_take" => write(
            workspace,
            input,
            client,
            Op::AddTake {
                shot: required(input, "shot")?.into(),
                id: opt(input, "id"),
                name: required(input, "name")?.into(),
                index: opt_num(input, "index")?,
            },
        ),
        "add_cut" => write(
            workspace,
            input,
            client,
            Op::AddCut {
                id: opt(input, "id"),
                name: required(input, "name")?.into(),
                index: opt_num(input, "index")?,
            },
        ),
        "update_cut" => write(
            workspace,
            input,
            client,
            Op::UpdateCut {
                cut: required(input, "cut")?.into(),
                patch: fields([("name", Value::String(required(input, "name")?.into()))]),
            },
        ),
        "remove_cut" => write(
            workspace,
            input,
            client,
            Op::RemoveCut {
                cut: required(input, "cut")?.into(),
            },
        ),
        "add_clip" => add_clip(workspace, input, client),
        "update_clip" => update_clip(workspace, input, client),
        "remove_clip" => write(
            workspace,
            input,
            client,
            Op::RemoveClip {
                cut: required(input, "cut")?.into(),
                clip: required(input, "clip")?.into(),
            },
        ),
        "reorder_clip" => write(
            workspace,
            input,
            client,
            ops_api::reorder_clip_op(
                required(input, "cut")?.into(),
                required(input, "clip")?.into(),
                req_num(input, "index")?,
            ),
        ),
        "set_active_cut" => write(
            workspace,
            input,
            client,
            ops_api::set_active_cut_op(opt(input, "cut"), boolean(input, "story", false)?)?,
        ),
        "set_shot" => set_shot(workspace, input, client),
        "replace_movie" => movie_import::replace_movie(
            &project(workspace, input)?,
            required(input, "base_version")?,
            required(input, "shot")?,
            &path(input, "source")?,
            agent(client)?,
            opt(input, "note"),
        ),
        "move_shot_to_scene" => write(
            workspace,
            input,
            client,
            ops_api::move_shot_to_scene_op(
                required(input, "shot")?.into(),
                required(input, "scene")?.into(),
                req_num(input, "index")?,
            ),
        ),
        "reorder_shot" => write(
            workspace,
            input,
            client,
            ops_api::reorder_shot_op(required(input, "shot")?.into(), req_num(input, "index")?),
        ),
        "add_ref" => ops_api::commit_add_ref(
            &project(workspace, input)?,
            required(input, "base_version")?,
            required(input, "shot")?.into(),
            opt(input, "take"),
            required(input, "reference")?,
            agent(client)?,
            opt(input, "note"),
        ),
        "remove_ref" => write(
            workspace,
            input,
            client,
            ops_api::ref_op(
                required(input, "shot")?.into(),
                opt(input, "take"),
                required(input, "reference")?,
                false,
            )?,
        ),
        "select_take" => write(
            workspace,
            input,
            client,
            Op::SelectTake {
                shot: required(input, "shot")?.into(),
                take: required(input, "take")?.into(),
            },
        ),
        "trim" => trim(workspace, input, client),
        "undo" => {
            let dir = project(workspace, input)?;
            ops_api::undo_ops(
                &dir,
                required(input, "base_version")?,
                agent(client)?,
                number(input, "steps", 1)?,
            )
        }
        "redo" => {
            let dir = project(workspace, input)?;
            ops_api::redo_ops(
                &dir,
                required(input, "base_version")?,
                agent(client)?,
                number(input, "steps", 1)?,
            )
        }
        "history" => ops_api::history_result(
            &project(workspace, input)?,
            number(input, "limit", 20)?,
            boolean(input, "full", false)?,
        ),
        "show" => ops_api::show(&project(workspace, input)?, opt_ref(input, "cut")),
        "describe" => ops_api::describe(
            &project(workspace, input)?,
            opt(input, "shot"),
            opt(input, "take"),
            opt(input, "panel"),
            at_value(input)?.as_deref(),
        ),
        "panel_image" => {
            return render_panel(workspace, input)
                .map(|(value, bytes)| Output::Image(value, bytes));
        }
        "cut_sheet" => ops_api::cut_sheet_in_workspace(
            CutSheetArgs {
                dir: project(workspace, input)?,
                cut: opt(input, "cut"),
                width: number(input, "width", 1200)?,
                out: path(input, "out")?,
            },
            workspace,
        ),
        "export_movie" => movie_export::export(
            &project(workspace, input)?,
            opt_ref(input, "cut"),
            movie_export::ImageMode::parse(opt_ref(input, "mode").unwrap_or("original"))?,
        ),
        "note" => write(
            workspace,
            input,
            client,
            Op::AddSticky {
                id: None,
                text: required(input, "text")?.into(),
                kind: opt(input, "kind").unwrap_or_else(|| "note".into()),
                author: agent(client)?,
                pin: ops_api::parse_pin(opt_ref(input, "pin").unwrap_or("none"))?,
                created: now()?,
            },
        ),
        "reply" => write(
            workspace,
            input,
            client,
            Op::ReplySticky {
                sticky: required(input, "sticky")?.into(),
                id: None,
                text: required(input, "text")?.into(),
                kind: opt(input, "kind").unwrap_or_else(|| "note".into()),
                author: agent(client)?,
                created: now()?,
            },
        ),
        "resolve" => write(
            workspace,
            input,
            client,
            Op::ResolveSticky {
                sticky: required(input, "sticky")?.into(),
            },
        ),
        "notes" => ops_api::notes_result(
            &project(workspace, input)?,
            opt(input, "kind"),
            opt(input, "author"),
            opt(input, "pin"),
            boolean(input, "resolved", false)?,
            boolean(input, "unresolved", false)?,
            boolean(input, "all", false)?,
        ),
        "schema" => ops_api::schema(opt_ref(input, "op")),
        "apply" => {
            let ops = serde_json::from_value(
                input
                    .get("ops")
                    .cloned()
                    .ok_or_else(|| ops_api::validation("missing ops"))?,
            )
            .map_err(|e| ops_api::validation(e.to_string()))?;
            ops_api::commit_ops(
                &project(workspace, input)?,
                required(input, "base_version")?,
                ops,
                agent(client)?,
                opt(input, "note"),
            )
        }
        _ => return Err(ops_api::validation(format!("unknown tool {name}"))),
    }?;
    let result = if is_write_tool(name) && boolean(input, "full", false)? {
        ops_api::full_write_result(&project(workspace, input)?, result, true)?
    } else {
        result
    };
    Ok(Output::Text(result))
}

fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "add_scene"
            | "add_shot"
            | "import_shot"
            | "remove_scene"
            | "remove_shot"
            | "add_panel"
            | "remove_panel"
            | "add_text"
            | "add_image"
            | "remove_layer"
            | "add_take"
            | "add_cut"
            | "update_cut"
            | "remove_cut"
            | "add_clip"
            | "update_clip"
            | "remove_clip"
            | "reorder_clip"
            | "set_active_cut"
            | "set_shot"
            | "replace_movie"
            | "move_shot_to_scene"
            | "reorder_shot"
            | "add_ref"
            | "remove_ref"
            | "select_take"
            | "trim"
            | "undo"
            | "redo"
            | "note"
            | "reply"
            | "resolve"
            | "apply"
    )
}

fn write(
    workspace: &Path,
    input: &Map<String, Value>,
    client: &str,
    op: Op,
) -> Result<Value, ApiError> {
    ops_api::commit_ops(
        &project(workspace, input)?,
        required(input, "base_version")?,
        vec![op],
        agent(client)?,
        opt(input, "note"),
    )
}
fn add_panel(
    workspace: &Path,
    input: &Map<String, Value>,
    client: &str,
) -> Result<Value, ApiError> {
    let dir = project(workspace, input)?;
    let op = ops_api::add_panel_op(
        &dir,
        opt(input, "shot"),
        opt(input, "take"),
        at_value(input)?.as_deref(),
        opt(input, "id"),
        opt_num(input, "time")?,
        opt_num(input, "index")?,
    )?;
    ops_api::commit_ops(
        &dir,
        required(input, "base_version")?,
        vec![op],
        agent(client)?,
        opt(input, "note"),
    )
}
fn remove_panel(
    workspace: &Path,
    input: &Map<String, Value>,
    client: &str,
) -> Result<Value, ApiError> {
    let dir = project(workspace, input)?;
    let op = ops_api::remove_panel_op(
        &dir,
        opt(input, "shot"),
        opt(input, "take"),
        opt(input, "panel"),
        at_value(input)?.as_deref(),
    )?;
    ops_api::commit_ops(
        &dir,
        required(input, "base_version")?,
        vec![op],
        agent(client)?,
        opt(input, "note"),
    )
}
fn add_layer(
    workspace: &Path,
    input: &Map<String, Value>,
    client: &str,
    kind: &str,
) -> Result<Value, ApiError> {
    let dir = project(workspace, input)?;
    let op = if kind == "text" {
        ops_api::add_text_op(
            &dir,
            opt(input, "shot"),
            opt(input, "take"),
            opt(input, "panel"),
            at_value(input)?.as_deref(),
            required(input, "text")?.into(),
            number(input, "x", 0.0)?,
            number(input, "y", 0.0)?,
            number(input, "width", 400.0)?,
            number(input, "height", 100.0)?,
            number(input, "font_size", 32.0)?,
            opt(input, "color").unwrap_or_else(|| "#000000".into()),
        )?
    } else {
        ops_api::add_image_op(
            &dir,
            opt(input, "shot"),
            opt(input, "take"),
            opt(input, "panel"),
            at_value(input)?.as_deref(),
            required(input, "src")?,
            number(input, "x", 0.0)?,
            number(input, "y", 0.0)?,
            number(input, "width", 400.0)?,
            number(input, "height", 300.0)?,
            opt(input, "fit").unwrap_or_else(|| "contain".into()),
        )?
    };
    ops_api::commit_ops(
        &dir,
        required(input, "base_version")?,
        vec![op],
        agent(client)?,
        opt(input, "note"),
    )
}
fn remove_layer(
    workspace: &Path,
    input: &Map<String, Value>,
    client: &str,
) -> Result<Value, ApiError> {
    let dir = project(workspace, input)?;
    let op = ops_api::remove_layer_op(
        &dir,
        opt(input, "shot"),
        opt(input, "take"),
        opt(input, "panel"),
        at_value(input)?.as_deref(),
        required(input, "layer")?.into(),
    )?;
    ops_api::commit_ops(
        &dir,
        required(input, "base_version")?,
        vec![op],
        agent(client)?,
        opt(input, "note"),
    )
}
fn add_clip(workspace: &Path, input: &Map<String, Value>, client: &str) -> Result<Value, ApiError> {
    let source = match (
        opt(input, "shot"),
        opt(input, "take"),
        opt(input, "nested_cut"),
    ) {
        (Some(shot), take, None) => ClipSource::Shot(ShotSource {
            shot,
            take,
            extra: Default::default(),
            source_extra: Default::default(),
        }),
        (None, None, Some(cut)) => ClipSource::Cut(CutSource {
            cut,
            extra: Default::default(),
            source_extra: Default::default(),
        }),
        _ => {
            return Err(ops_api::validation(
                "provide shot with optional take, or nested_cut",
            ));
        }
    };
    write(
        workspace,
        input,
        client,
        Op::AddClip {
            cut: required(input, "cut")?.into(),
            id: opt(input, "id"),
            source,
            index: opt_num(input, "index")?,
        },
    )
}
fn update_clip(
    workspace: &Path,
    input: &Map<String, Value>,
    client: &str,
) -> Result<Value, ApiError> {
    let dir = project(workspace, input)?;
    let op = ops_api::update_clip_take_op(
        &dir,
        required(input, "cut")?.into(),
        required(input, "clip")?.into(),
        opt(input, "take"),
    )?;
    ops_api::commit_ops(
        &dir,
        required(input, "base_version")?,
        vec![op],
        agent(client)?,
        opt(input, "note"),
    )
}
fn set_shot(workspace: &Path, input: &Map<String, Value>, client: &str) -> Result<Value, ApiError> {
    let op = ops_api::set_shot_op(
        required(input, "shot")?.into(),
        opt(input, "title"),
        opt(input, "action"),
        opt(input, "dialogue"),
        opt(input, "speaker"),
        opt(input, "camera"),
        opt(input, "notes"),
        opt_num(input, "duration")?,
    )?;
    write(workspace, input, client, op)
}
fn trim(workspace: &Path, input: &Map<String, Value>, client: &str) -> Result<Value, ApiError> {
    let op = ops_api::trim_op(
        required(input, "cut")?.into(),
        required(input, "clip")?.into(),
        opt_num(input, "in")?,
        opt_num(input, "out")?,
        opt_num(input, "slot")?,
    )?;
    write(workspace, input, client, op)
}
fn render_panel(
    workspace: &Path,
    input: &Map<String, Value>,
) -> Result<(Value, Vec<u8>), ApiError> {
    let value = ops_api::panel_image_in_workspace(
        PanelImageArgs {
            dir: project(workspace, input)?,
            shot: opt(input, "shot"),
            take: opt(input, "take"),
            panel: opt(input, "panel"),
            at: at_value(input)?,
            width: number(input, "width", 960)?,
            mode: opt(input, "mode").unwrap_or_else(|| "stage".into()),
            out: opt(input, "out").map(PathBuf::from),
        },
        workspace,
    )?;
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("render returned no path"))?;
    let bytes = fs::read(path).map_err(storyboard_store::StoreError::from)?;
    Ok((value, bytes))
}
fn project(workspace: &Path, input: &Map<String, Value>) -> Result<PathBuf, ApiError> {
    let requested = path(input, "project")?;
    let path = if single_project_mode() && !requested.is_absolute() {
        if requested == Path::new(".")
            || workspace
                .file_name()
                .is_some_and(|name| requested == Path::new(name))
        {
            workspace.to_path_buf()
        } else {
            return Err(ops_api::validation("Claude can edit only the open project"));
        }
    } else if requested.is_absolute() {
        requested
    } else {
        workspace.join(requested)
    };
    let path = path.canonicalize().map_err(|e| ApiError::Store(e.into()))?;
    if single_project_mode() && path != workspace {
        return Err(ops_api::validation("Claude can edit only the open project"));
    }
    if path.starts_with(workspace) {
        Ok(path)
    } else {
        Err(ops_api::validation(
            "project must be inside the MCP workspace",
        ))
    }
}
fn project_destination(workspace: &Path, requested: PathBuf) -> Result<PathBuf, ApiError> {
    if single_project_mode() {
        return Err(ops_api::validation(
            "Claude cannot create another project in this run",
        ));
    }
    if requested
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(ops_api::validation(
            "new project path must not contain parent segments",
        ));
    }
    let path = if requested.is_absolute() {
        requested
    } else {
        workspace.join(requested)
    };
    if !path.starts_with(workspace) {
        return Err(ops_api::validation(
            "project must be inside the MCP workspace",
        ));
    }
    for ancestor in path.ancestors() {
        if ancestor == workspace {
            break;
        }
        if fs::symlink_metadata(ancestor).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(ops_api::validation(
                "new project path must not use symlinked parents",
            ));
        }
    }
    let existing = nearest_existing_ancestor(&path)?;
    let canonical = existing
        .canonicalize()
        .map_err(|error| ApiError::Store(error.into()))?;
    if !canonical.starts_with(workspace) {
        return Err(ops_api::validation(
            "project must be inside the MCP workspace",
        ));
    }
    Ok(path)
}
fn single_project_mode() -> bool {
    std::env::var_os("NB_STORYBOARD_MCP_SINGLE_PROJECT").is_some_and(|value| value == "1")
}
fn nearest_existing_ancestor(path: &Path) -> Result<&Path, ApiError> {
    path.ancestors()
        .find(|ancestor| ancestor.exists())
        .ok_or_else(|| ops_api::validation("project path has no existing ancestor"))
}
fn agent(name: &str) -> Result<Author, ApiError> {
    ops_api::author(Some(format!("agent:{name}")))
}
fn now() -> Result<String, ApiError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| ops_api::validation(e.to_string()))
}
fn fields<const N: usize>(values: [(&str, Value); N]) -> Map<String, Value> {
    values.into_iter().map(|(k, v)| (k.into(), v)).collect()
}
fn required<'a>(input: &'a Map<String, Value>, name: &str) -> Result<&'a str, ApiError> {
    input
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation(format!("missing or invalid {name}")))
}
fn required_value<'a>(input: &'a Value, name: &str) -> Result<&'a str, ApiError> {
    input
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation(format!("missing or invalid {name}")))
}
fn opt(input: &Map<String, Value>, name: &str) -> Option<String> {
    input.get(name).and_then(Value::as_str).map(str::to_owned)
}
fn opt_ref<'a>(input: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    input.get(name).and_then(Value::as_str)
}
fn at_value(input: &Map<String, Value>) -> Result<Option<String>, ApiError> {
    input
        .get("at")
        .map(|value| match value {
            Value::String(value) => Ok(value.clone()),
            Value::Number(value) if value.as_u64().is_some() => Ok(value.to_string()),
            _ => Err(ops_api::validation(
                "at must be a string such as \"1\" or \"2/1\", or an integer such as 1",
            )),
        })
        .transpose()
}
fn path(input: &Map<String, Value>, name: &str) -> Result<PathBuf, ApiError> {
    Ok(PathBuf::from(required(input, name)?))
}
fn number<T: serde::de::DeserializeOwned>(
    input: &Map<String, Value>,
    name: &str,
    default: T,
) -> Result<T, ApiError> {
    input
        .get(name)
        .map(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| ops_api::validation(format!("invalid {name}: {e}")))
        })
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn req_num<T: serde::de::DeserializeOwned>(
    input: &Map<String, Value>,
    name: &str,
) -> Result<T, ApiError> {
    input
        .get(name)
        .cloned()
        .ok_or_else(|| ops_api::validation(format!("missing {name}")))
        .and_then(|v| {
            serde_json::from_value(v)
                .map_err(|e| ops_api::validation(format!("invalid {name}: {e}")))
        })
}
fn opt_num<T: serde::de::DeserializeOwned>(
    input: &Map<String, Value>,
    name: &str,
) -> Result<Option<T>, ApiError> {
    input
        .get(name)
        .map(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| ops_api::validation(format!("invalid {name}: {e}")))
        })
        .transpose()
}
fn boolean(input: &Map<String, Value>, name: &str, default: bool) -> Result<bool, ApiError> {
    number(input, name, default)
}
