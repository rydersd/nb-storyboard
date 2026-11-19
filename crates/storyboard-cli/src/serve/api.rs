//! JSON routes: the workspace index, a project's state and history, and writes through ops_api.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};
use storyboard_core::{Author, Op};
use storyboard_store::{
    Loaded, Viewport, check_refs, import_source_bytes, import_source_reader, index, open,
    open_workspace, read_viewport, scan_sources, validate_viewport, write_viewport,
};

use super::http::encode;
use super::{State, body_project, project_dir, project_name};
use crate::ops_api::{self, ApiError};
use crate::{movie_export, movie_import, sequence_import};

const MAX_LAYOUT_BYTES: usize = 16 * 1024;
const PANEL_IDS: &[&str] = &[
    "graph",
    "inspector",
    "stickies",
    "history",
    "sources",
    "switcher",
    "player",
    "cut-editor",
    "sample.dialogue-word-count",
];
static NEXT_LAYOUT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkbenchLayout {
    left: Vec<String>,
    right: Vec<String>,
    bottom: Vec<String>,
    #[serde(default)]
    center: Vec<String>,
    hidden: Vec<String>,
    sizes: HashMap<String, u16>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutWrite {
    layout: WorkbenchLayout,
}

pub(super) fn workspace_index(state: &State) -> Result<Value, ApiError> {
    let workspace = open_workspace(&state.root)?;
    let entries = index(&workspace)?;
    let projects: Vec<Value> = entries
        .into_iter()
        .map(|entry| {
            let name = project_name(&entry.path);
            let mut value = serde_json::to_value(&entry).unwrap_or(Value::Null);
            if let Value::Object(map) = &mut value {
                map.remove("path");
                map.insert("project".into(), Value::String(name.clone()));
                let thumb = map.remove("thumbnail").filter(|value| !value.is_null());
                map.insert(
                    "thumbnail".into(),
                    match thumb {
                        Some(_) => {
                            Value::String(format!("/api/thumb.png?project={}", encode(&name)))
                        }
                        None => Value::Null,
                    },
                );
            }
            value
        })
        .collect();
    Ok(json!({
        "root": state.root.file_name().map(|name| name.to_string_lossy().to_string()),
        "root_path": state.root,
        "cli_path": std::env::current_exe().ok(),
        "projects": projects,
    }))
}

pub(super) fn create_project(state: &State, body: Value) -> Result<Value, ApiError> {
    let folder = body
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("project folder is required"))?;
    if folder.len() > 64
        || folder.is_empty()
        || !folder
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(ops_api::validation(
            "project folder must be 1 to 64 ASCII letters, numbers, hyphens or underscores",
        ));
    }
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("title is required"))?;
    if title.trim().is_empty() || title.chars().count() > 20_000 {
        return Err(ops_api::validation("title must be 1 to 20000 characters"));
    }
    let number = |key: &str, default: u32| -> Result<u32, ApiError> {
        match body.get(key) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| ops_api::validation(format!("{key} must be a nonnegative integer"))),
        }
    };
    let fps = number("fps", 24)?;
    let width = number("width", 1440)?;
    let height = number("height", 900)?;
    if ![24, 25, 30, 60].contains(&fps) {
        return Err(ops_api::validation("fps must be 24, 25, 30 or 60"));
    }
    if !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
        return Err(ops_api::validation(
            "width and height must each be 1 to 8192",
        ));
    }
    let dir = state.root.join(folder);
    fs::create_dir(&dir).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            ops_api::validation(format!(
                "project folder {folder:?} already exists; choose another name"
            ))
        } else {
            ApiError::Store(storyboard_store::StoreError::Io(error))
        }
    })?;
    let result = ops_api::create_project(&dir, title, fps, width, height)?;
    Ok(json!({"project": folder, "version": result["version"], "title": title}))
}

pub(super) fn project_state(
    state: &State,
    query: &HashMap<String, String>,
) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    let loaded = open(&dir)?;
    let cut = query
        .get("cut")
        .map(String::as_str)
        .filter(|cut| !cut.is_empty());
    project_state_from_loaded(&dir, &loaded, cut)
}

fn project_state_from_loaded(
    dir: &Path,
    loaded: &Loaded,
    cut: Option<&str>,
) -> Result<Value, ApiError> {
    let sequence = ops_api::show_loaded(loaded, cut)?;
    Ok(json!({
        "project": project_name(dir),
        "version": loaded.version,
        "document": loaded.project,
        "sequence": {
            "cut": sequence.get("cut"),
            "frames": sequence.get("frames"),
            "items": sequence.get("shots"),
            "warnings": sequence.get("warnings"),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use storyboard_core::Format;
    use storyboard_store::{commit, create};

    #[test]
    fn project_state_compiles_the_same_saved_snapshot_as_its_document() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("movie");
        let before = create(
            &dir,
            "Movie",
            24,
            Format {
                width: 1440,
                height: 900,
                extra: Default::default(),
            },
        )
        .unwrap();
        commit(
            &dir,
            &before.version,
            vec![
                Op::AddScene {
                    id: Some("scene-1".into()),
                    title: "Scene".into(),
                    index: None,
                },
                Op::AddShot {
                    scene: "scene-1".into(),
                    id: Some("shot-1".into()),
                    title: "Shot".into(),
                    index: None,
                },
            ],
            Author::Human,
            None,
        )
        .unwrap();

        let old_response = project_state_from_loaded(&dir, &before, None).unwrap();
        let new_response = project_state_from_loaded(&dir, &open(&dir).unwrap(), None).unwrap();
        assert_eq!(old_response["version"], before.version);
        assert_eq!(old_response["document"], json!(before.project));
        assert_eq!(old_response["sequence"]["frames"], 0);
        assert_eq!(
            old_response["sequence"]["items"].as_array().unwrap().len(),
            0
        );
        assert_ne!(new_response["version"], old_response["version"]);
        assert_eq!(
            new_response["document"]["scenes"].as_array().unwrap().len(),
            1
        );
        assert_eq!(new_response["sequence"]["frames"], 48);
    }
}

pub(super) fn export_movie(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let expected = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("base_version is required"))?;
    let loaded = open(&dir)?;
    if loaded.version != expected {
        return Err(ops_api::validation(
            "project changed before export; refresh and try again",
        ));
    }
    let cut = body
        .get("cut")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let mode = movie_export::ImageMode::parse(
        body.get("mode")
            .and_then(Value::as_str)
            .unwrap_or("original"),
    )?;
    movie_export::export_loaded(&loaded, cut, mode)
}

pub(super) fn history(state: &State, query: &HashMap<String, String>) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    let limit = query
        .get("limit")
        .and_then(|limit| limit.parse().ok())
        .unwrap_or(50usize)
        .min(500);
    ops_api::history_result(&dir, limit, false)
}

pub(super) fn sources(state: &State, query: &HashMap<String, String>) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    let loaded = open(&dir)?;
    let mut sources = serde_json::to_value(scan_sources(&dir)?).map_err(|error| {
        ops_api::validation(format!("could not serialize sources scan: {error}"))
    })?;
    if let Value::Array(entries) = &mut sources {
        for entry in entries {
            if let Value::Object(entry) = entry {
                let used_by = entry.remove("uses").unwrap_or_else(|| json!([]));
                entry.insert("used_by".into(), used_by);
            }
        }
    }
    Ok(json!({
        "sources": sources,
        "missing_refs": check_refs(&dir, &loaded.project)?,
    }))
}

pub(super) fn commit(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base_version = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let ops_value = body
        .get("ops")
        .cloned()
        .ok_or_else(|| ops_api::validation("ops is required"))?;
    let ops: Vec<Op> = serde_json::from_value(ops_value)
        .map_err(|error| ops_api::validation(format!("ops: {error}")))?;
    let note = body.get("note").and_then(Value::as_str).map(str::to_string);
    let result = ops_api::commit_ops(&dir, base_version, ops, Author::Human, note)?;
    with_document(&dir, result)
}

pub(super) fn undo(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base_version = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let steps = body
        .get("steps")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, 100) as usize;
    let result = ops_api::undo_ops(&dir, base_version, Author::Human, steps)?;
    with_document(&dir, result)
}

pub(super) fn redo(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base_version = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let steps = body
        .get("steps")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, 100) as usize;
    let result = ops_api::redo_ops(&dir, base_version, Author::Human, steps)?;
    with_document(&dir, result)
}

pub(super) fn viewport(state: &State, query: &HashMap<String, String>) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    Ok(json!({"viewport": read_viewport(&dir)?}))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewportWrite {
    project: String,
    viewport: Viewport,
}

pub(super) fn save_viewport(state: &State, body: Value) -> Result<Value, ApiError> {
    let request: ViewportWrite = serde_json::from_value(body)
        .map_err(|error| ops_api::validation(format!("viewport request: {error}")))?;
    let dir = body_project(state, &json!({"project": request.project}))?;
    let project = open(&dir)?.project;
    validate_viewport(&request.viewport, &project).map_err(ops_api::validation)?;
    write_viewport(&dir, &request.viewport)?;
    Ok(json!({"viewport": request.viewport}))
}

pub(super) fn layout(state: &State) -> Result<Value, ApiError> {
    Ok(json!({"layout": read_layout(&state.root)?}))
}

pub(super) fn save_layout(state: &State, body: Value) -> Result<Value, ApiError> {
    let request: LayoutWrite = serde_json::from_value(body)
        .map_err(|error| ops_api::validation(format!("layout request: {error}")))?;
    validate_layout(&request.layout).map_err(ops_api::validation)?;
    let layout = normalize_layout(request.layout);
    write_layout(&state.root, &layout)?;
    Ok(json!({"layout": layout}))
}

fn read_layout(root: &Path) -> Result<Option<WorkbenchLayout>, ApiError> {
    let cache = root.join(".cache");
    match fs::symlink_metadata(&cache) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => return Ok(None),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ApiError::Store(storyboard_store::StoreError::Io(error))),
    };
    let path = cache.join("workbench-layout.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Ok(None);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ApiError::Store(storyboard_store::StoreError::Io(error))),
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) if bytes.len() <= MAX_LAYOUT_BYTES => bytes,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ApiError::Store(storyboard_store::StoreError::Io(error))),
    };
    let layout: WorkbenchLayout = match serde_json::from_slice(&bytes) {
        Ok(layout) => layout,
        Err(_) => return Ok(None),
    };
    if validate_layout(&layout).is_err() {
        return Ok(None);
    }
    Ok(Some(normalize_layout(layout)))
}

fn validate_layout(layout: &WorkbenchLayout) -> Result<(), String> {
    let entries = [
        &layout.left,
        &layout.right,
        &layout.bottom,
        &layout.center,
        &layout.hidden,
    ];
    if entries.iter().map(|dock| dock.len()).sum::<usize>() > 64 {
        return Err("layout may contain at most 64 panel entries".into());
    }
    for id in entries.into_iter().flatten() {
        if id.is_empty() || id.len() > 160 || !id.is_ascii() {
            return Err("panel ids must be 1 to 160 ASCII characters".into());
        }
    }
    for (dock, size) in &layout.sizes {
        if !["left", "right", "bottom"].contains(&dock.as_str()) {
            return Err("layout sizes may name only left, right, or bottom".into());
        }
        if !(160..=800).contains(size) {
            return Err("layout dock sizes must be from 160 to 800 pixels".into());
        }
    }
    let bytes = serde_json::to_vec(layout).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_LAYOUT_BYTES {
        return Err(format!(
            "layout JSON must be at most {MAX_LAYOUT_BYTES} bytes"
        ));
    }
    Ok(())
}

fn normalize_layout(mut layout: WorkbenchLayout) -> WorkbenchLayout {
    let allowed: HashSet<_> = PANEL_IDS.iter().copied().collect();
    let mut seen = HashSet::new();
    for dock in [
        &mut layout.left,
        &mut layout.right,
        &mut layout.bottom,
        &mut layout.center,
        &mut layout.hidden,
    ] {
        dock.retain(|id| allowed.contains(id.as_str()) && seen.insert(id.clone()));
    }
    layout
        .sizes
        .retain(|dock, _| ["left", "right", "bottom"].contains(&dock.as_str()));
    layout
}

fn write_layout(root: &Path, layout: &WorkbenchLayout) -> Result<(), ApiError> {
    let mut bytes = serde_json::to_vec_pretty(layout)
        .map_err(|error| ops_api::validation(format!("could not serialize layout: {error}")))?;
    bytes.push(b'\n');
    let cache = root.join(".cache");
    match fs::symlink_metadata(&cache) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(ops_api::validation(
                "workspace cache is not a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&cache)
            .map_err(|error| ApiError::Store(storyboard_store::StoreError::Io(error)))?,
        Err(error) => return Err(ApiError::Store(storyboard_store::StoreError::Io(error))),
    }
    match fs::symlink_metadata(&cache) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(ops_api::validation(
                "workspace cache is not a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) => return Err(ApiError::Store(storyboard_store::StoreError::Io(error))),
    }
    let path = cache.join("workbench-layout.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(ops_api::validation("workspace layout cache is a symlink"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(ApiError::Store(storyboard_store::StoreError::Io(error))),
    }
    let temporary = cache.join(format!(
        "workbench-layout.json.tmp-{}-{}",
        std::process::id(),
        NEXT_LAYOUT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        fs::File::open(&cache)?.sync_all()?;
        Ok::<(), io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| ApiError::Store(storyboard_store::StoreError::Io(error)))
}

pub(super) fn upload_source(
    state: &State,
    query: &HashMap<String, String>,
    bytes: &[u8],
) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    let folder = query
        .get("folder")
        .ok_or_else(|| ops_api::validation("folder is required"))?;
    if folder.is_empty()
        || folder.len() > 80
        || !folder
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(ops_api::validation("folder must be a short hexadecimal id"));
    }
    let name = query
        .get("name")
        .ok_or_else(|| ops_api::validation("name is required"))?;
    if !sequence_import::image_name(name) || bytes.is_empty() {
        return Err(ops_api::validation(
            "upload needs a named, nonempty image file",
        ));
    }
    let relative = Path::new("sources/imported-shots").join(folder).join(name);
    let path = import_source_bytes(&dir, bytes, &relative)?;
    Ok(json!({"path": path.to_string_lossy().replace('\\', "/")}))
}

fn movie_folder(folder: &str) -> Result<(), ApiError> {
    if folder.len() != 36
        || !folder
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(ops_api::validation("movie import folder must be a UUID"));
    }
    Ok(())
}

pub(super) fn upload_movie(
    state: &State,
    query: &HashMap<String, String>,
    reader: impl Read,
) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    let folder = query
        .get("folder")
        .ok_or_else(|| ops_api::validation("folder is required"))?;
    movie_folder(folder)?;
    let source = format!("sources/imported-movies/{folder}/original.mp4");
    let mut limited = reader.take(super::MAX_MOVIE_UPLOAD_BYTES + 1);
    let (path, size) = import_source_reader(&dir, &mut limited, Path::new(&source))?;
    if size == 0 || size > super::MAX_MOVIE_UPLOAD_BYTES {
        let _ = fs::remove_file(dir.join(path));
        return Err(ops_api::validation(if size == 0 {
            "select a nonempty MP4 movie".to_string()
        } else {
            "movie upload is larger than 1 GB".to_string()
        }));
    }
    match movie_import::probe(&dir.join(&path)) {
        Ok(media) => Ok(json!({"path": source, "media": media})),
        Err(error) => {
            let _ = fs::remove_file(dir.join(path));
            Err(error)
        }
    }
}

pub(super) fn upload_audio(
    state: &State,
    query: &HashMap<String, String>,
    reader: impl Read,
) -> Result<Value, ApiError> {
    let dir = project_dir(state, query)?;
    let folder = query
        .get("folder")
        .ok_or_else(|| ops_api::validation("folder is required"))?;
    movie_folder(folder)?;
    let ext = query.get("ext").map(String::as_str).unwrap_or("");
    if !["mp3", "wav", "m4a"].contains(&ext) {
        return Err(ops_api::validation("audio must be MP3, WAV or M4A"));
    }
    let source = format!("sources/imported-audio/{folder}/original.{ext}");
    let mut limited = reader.take(super::MAX_AUDIO_UPLOAD_BYTES + 1);
    let (path, size) = import_source_reader(&dir, &mut limited, Path::new(&source))?;
    if size == 0 || size > super::MAX_AUDIO_UPLOAD_BYTES {
        let _ = fs::remove_file(dir.join(path));
        return Err(ops_api::validation(if size == 0 {
            "select a nonempty audio file".to_string()
        } else {
            "audio upload is larger than 128 MB".to_string()
        }));
    }
    match movie_import::probe_audio(&dir.join(&path)) {
        Ok(media) => Ok(json!({"path": source, "media": media})),
        Err(error) => {
            let _ = fs::remove_file(dir.join(path));
            Err(error)
        }
    }
}

pub(super) fn import_movie(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let folder = body
        .get("folder")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("folder is required"))?;
    let path = uploaded_movie_path(&dir, folder)?;
    let scene = body
        .get("scene")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Imported movie")
        .trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(ops_api::validation(
            "movie title must be 1 to 200 characters",
        ));
    }
    let interval = body
        .get("interval")
        .and_then(Value::as_f64)
        .ok_or_else(|| ops_api::validation("sample interval is required"))?;
    let cut_times: Option<Vec<f64>> = body
        .get("cut_times")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| ops_api::validation("cut_times must be an array of seconds"))?
                .iter()
                .map(|time| {
                    time.as_f64().ok_or_else(|| {
                        ops_api::validation("each cut time must be a number of seconds")
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let media = movie_import::probe(&path)?;
    let duration = media["duration"]
        .as_f64()
        .expect("validated probe duration");
    let loaded = open(&dir)?;
    if loaded.version != base {
        return Err(ops_api::validation(
            "project changed since movie upload; refresh and retry",
        ));
    }
    let ops = movie_import::import_ops(
        &dir,
        &loaded.project,
        folder,
        scene,
        title,
        duration,
        movie_import::ImportTiming {
            interval,
            cut_times: cut_times.as_deref(),
        },
    )?;
    let result = ops_api::commit_ops(
        &dir,
        base,
        ops,
        Author::Human,
        Some(format!("Imported movie {title}")),
    )?;
    with_document(&dir, result)
}

pub(super) fn split_movie_shot(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let cut = body
        .get("cut")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("cut is required"))?;
    let clip = body
        .get("clip")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("clip is required"))?;
    let frame = body
        .get("frame")
        .and_then(Value::as_u64)
        .ok_or_else(|| ops_api::validation("split frame is required"))?;
    let loaded = open(&dir)?;
    if loaded.version != base {
        return Err(ops_api::validation(
            "project changed before the split; refresh and retry",
        ));
    }
    let ops = movie_import::split_shot_ops(&dir, &loaded.project, cut, clip, frame)?;
    let result = ops_api::commit_ops(
        &dir,
        base,
        ops,
        Author::Human,
        Some("Split imported movie shot".into()),
    )?;
    with_document(&dir, result)
}

pub(super) fn move_movie_boundary(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let cut = body
        .get("cut")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("cut is required"))?;
    let left_clip = body
        .get("left_clip")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("left clip is required"))?;
    let frame = body
        .get("frame")
        .and_then(Value::as_u64)
        .ok_or_else(|| ops_api::validation("boundary frame is required"))?;
    let loaded = open(&dir)?;
    if loaded.version != base {
        return Err(ops_api::validation(
            "project changed before the boundary edit; refresh and retry",
        ));
    }
    let ops = movie_import::move_boundary_ops(&dir, &loaded.project, cut, left_clip, frame)?;
    let result = ops_api::commit_ops(
        &dir,
        base,
        ops,
        Author::Human,
        Some("Move imported movie boundary".into()),
    )?;
    with_document(&dir, result)
}

pub(super) fn suggest_cuts(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let folder = body
        .get("folder")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("folder is required"))?;
    let path = uploaded_movie_path(&dir, folder)?;
    let threshold = body
        .get("threshold")
        .and_then(Value::as_f64)
        .unwrap_or(10.0);
    let fps = open(&dir)?.project.fps;
    let media = movie_import::probe(&path)?;
    let duration = media["duration"].as_f64().expect("validated duration");
    let marks = movie_import::suggest_cuts(&path, fps, duration, threshold)?;
    Ok(json!({"cut_times": marks, "threshold": threshold}))
}

fn uploaded_movie_path(dir: &Path, folder: &str) -> Result<std::path::PathBuf, ApiError> {
    movie_folder(folder)?;
    let source = format!("sources/imported-movies/{folder}/original.mp4");
    let path = dir.join(&source);
    for relative in [
        "sources".to_owned(),
        "sources/imported-movies".to_owned(),
        format!("sources/imported-movies/{folder}"),
    ] {
        if !dir
            .join(relative)
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        {
            return Err(ops_api::validation(
                "movie source folder must be a real project directory",
            ));
        }
    }
    if !path
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
    {
        return Err(ops_api::validation(
            "upload the movie into this project first",
        ));
    }
    Ok(path)
}

pub(super) fn import_shot(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let base = body
        .get("base_version")
        .and_then(Value::as_str)
        .ok_or(ApiError::MissingBaseVersion)?;
    let scene = body
        .get("scene")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("scene is required"))?;
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("title is required"))?;
    let duration = body
        .get("duration")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_f64()
                .ok_or_else(|| ops_api::validation("duration must be a number"))
        })
        .transpose()?;
    let index = body
        .get("index")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| ops_api::validation("index must be a nonnegative integer"))
        })
        .transpose()?;
    let mut files: Vec<(String, String)> = body
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| ops_api::validation("files array is required"))?
        .iter()
        .map(|file| {
            let name = file
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| ops_api::validation("file name is required"))?;
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| ops_api::validation("file path is required"))?;
            Ok((name.to_owned(), path.to_owned()))
        })
        .collect::<Result<_, ApiError>>()?;
    sequence_import::sort_images(&mut files)?;
    for (name, source) in &files {
        let path = Path::new(source);
        let parts: Vec<_> = path.components().collect();
        if parts.len() != 4
            || parts[0].as_os_str() != "sources"
            || parts[1].as_os_str() != "imported-shots"
            || !parts[2]
                .as_os_str()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
            || parts[3].as_os_str() != name.as_str()
            || !dir
                .join("sources/imported-shots")
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            || !dir
                .join("sources/imported-shots")
                .join(parts[2].as_os_str())
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            || !dir
                .join(path)
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            return Err(ops_api::validation(format!(
                "{name}: upload an image into this project first"
            )));
        }
    }
    let loaded = open(&dir)?;
    let ops = sequence_import::import_ops(&loaded.project, scene, title, duration, index, &files)?;
    let result = ops_api::commit_ops(&dir, base, ops, Author::Human, None)?;
    with_document(&dir, result)
}

/// Write results carry the new document so the page never needs a second round trip.
pub(super) fn with_document(dir: &Path, mut result: Value) -> Result<Value, ApiError> {
    let loaded = open(dir)?;
    if let Value::Object(map) = &mut result {
        map.insert("document".into(), json!(loaded.project));
        map.insert("version".into(), Value::String(loaded.version));
    }
    Ok(result)
}
