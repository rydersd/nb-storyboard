//! Panel images and thumbnails, with a bounded, content-keyed memory cache.

use std::{
    collections::{HashMap, VecDeque},
    path::Path,
};

use serde_json::json;
use sha2::{Digest, Sha256};
use storyboard_core::RenderMode;
use storyboard_store::{Workspace, index, open};

use super::http::{Bytes, error_bytes, header};
use super::{RENDER_CACHE_ENTRIES, State, project_dir};
use crate::ops_api::{self, ApiError};

pub(super) fn panel_png(state: &State, query: &HashMap<String, String>) -> Bytes {
    match render_png(state, query) {
        Ok((png, warnings)) => Bytes {
            status: 200,
            content_type: "image/png",
            body: png,
            extra: vec![
                header("Cache-Control", "no-store"),
                header("X-Render-Warnings", &warnings),
            ],
        },
        Err(error) => error_bytes(error),
    }
}

pub(super) fn render_png(
    state: &State,
    query: &HashMap<String, String>,
) -> Result<(Vec<u8>, String), ApiError> {
    let dir = project_dir(state, query)?;
    let loaded = open(&dir)?;
    let get = |key: &str| query.get(key).cloned().filter(|value| !value.is_empty());
    let (shot, take, panel) =
        panel_selection(&loaded.project, get("shot"), get("take"), get("panel"))?;
    let width: u32 = get("width")
        .and_then(|width| width.parse().ok())
        .unwrap_or(480)
        .clamp(16, 4096);
    let mode = match get("mode").as_deref() {
        Some("board") => RenderMode::Board,
        _ => RenderMode::Stage,
    };
    let key = render_key(
        &loaded.project,
        &dir,
        &shot,
        &take,
        &panel,
        width,
        matches!(mode, RenderMode::Board),
    );
    if let Some(hit) = state
        .renders
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
    {
        return Ok(hit);
    }
    let rendered =
        storyboard_core::render_panel(&loaded.project, &dir, &shot, &take, &panel, width, mode)?;
    let warnings = serde_json::to_string(&rendered.warnings).unwrap_or_else(|_| "[]".into());
    let warnings = warnings.replace(['\r', '\n'], " ");
    let entry = (rendered.png, warnings);
    state
        .renders
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .put(key, entry.clone());
    Ok(entry)
}

/// A shot alone means its selected take's first panel; a shot and take mean that take's first panel.
pub(super) fn panel_selection(
    project: &storyboard_core::Project,
    shot: Option<String>,
    take: Option<String>,
    panel: Option<String>,
) -> Result<(String, String, String), ApiError> {
    let shot_id = shot.ok_or_else(|| ops_api::validation("shot is required"))?;
    let found = project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .find(|candidate| candidate.id == shot_id)
        .ok_or_else(|| ops_api::validation(format!("unknown shot {shot_id}")))?;
    let take_id = take.unwrap_or_else(|| found.selected_take.clone());
    let panel_id = match panel {
        Some(panel) => panel,
        None => found
            .takes
            .iter()
            .find(|candidate| candidate.id == take_id)
            .and_then(|take| take.panels.first())
            .map(|panel| panel.id.clone())
            .ok_or_else(|| ops_api::validation(format!("take {take_id} has no panels")))?,
    };
    ops_api::target(project, Some(shot_id), Some(take_id), Some(panel_id), None)
}

/// Content key: the panel, its shot's caption fields, the format and its source files' modified
/// times, so a render is reused until something it draws changes.
pub(super) fn render_key(
    project: &storyboard_core::Project,
    dir: &Path,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
    width: u32,
    board: bool,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dir.to_string_lossy().as_bytes());
    hasher.update(format!("{shot_id}/{take_id}/{panel_id}/{width}/{board}").as_bytes());
    hasher.update(serde_json::to_vec(&project.format).unwrap_or_default());
    hasher.update(project.fps.to_le_bytes());
    for (number, shot) in project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .enumerate()
    {
        if board {
            hasher.update(shot.duration.to_le_bytes());
        }
        if shot.id != shot_id {
            continue;
        }
        hasher.update(number.to_le_bytes());
        if board {
            let caption = json!([
                shot.title,
                shot.action,
                shot.dialogue,
                shot.speaker,
                shot.duration
            ]);
            hasher.update(caption.to_string().as_bytes());
        }
        for take in shot.takes.iter().filter(|take| take.id == take_id) {
            if board {
                hasher.update(take.name.as_bytes());
            }
            for panel in take.panels.iter().filter(|panel| panel.id == panel_id) {
                hasher.update(serde_json::to_vec(panel).unwrap_or_default());
                if let Ok(paths) =
                    storyboard_core::panel_source_paths(project, shot_id, take_id, panel_id)
                {
                    for path in paths {
                        let modified = std::fs::metadata(dir.join(&path))
                            .and_then(|meta| meta.modified())
                            .ok()
                            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|time| time.as_nanos())
                            .unwrap_or(0);
                        hasher.update(format!("{path}@{modified}").as_bytes());
                    }
                }
            }
        }
        break;
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn thumbnail(state: &State, query: &HashMap<String, String>) -> Bytes {
    let result = (|| -> Result<Vec<u8>, ApiError> {
        let dir = project_dir(state, query)?;
        let workspace = Workspace {
            root: state.root.clone(),
            projects: vec![dir.clone()],
        };
        let entry = index(&workspace)?
            .into_iter()
            .next()
            .ok_or_else(|| ops_api::validation("project not in index"))?;
        let path = entry
            .thumbnail
            .ok_or_else(|| ops_api::validation("project has no thumbnail"))?;
        let path = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        std::fs::read(&path).map_err(|error| ops_api::validation(format!("thumbnail: {error}")))
    })();
    match result {
        Ok(body) => Bytes {
            status: 200,
            content_type: "image/png",
            body,
            extra: vec![header("Cache-Control", "no-cache")],
        },
        Err(error) => error_bytes(error),
    }
}

#[derive(Default)]
pub(super) struct RenderCache {
    entries: HashMap<String, (Vec<u8>, String)>,
    order: VecDeque<String>,
}

impl RenderCache {
    fn get(&mut self, key: &str) -> Option<(Vec<u8>, String)> {
        let hit = self.entries.get(key).cloned()?;
        if let Some(position) = self.order.iter().position(|entry| entry == key) {
            let key = self.order.remove(position).unwrap_or_default();
            self.order.push_back(key);
        }
        Some(hit)
    }

    fn put(&mut self, key: String, value: (Vec<u8>, String)) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > RENDER_CACHE_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_cache_is_bounded() {
        let mut cache = RenderCache::default();
        for number in 0..(RENDER_CACHE_ENTRIES + 10) {
            cache.put(number.to_string(), (vec![0; 4], String::new()));
        }
        assert_eq!(cache.entries.len(), RENDER_CACHE_ENTRIES);
        assert!(cache.get("0").is_none());
        assert!(cache.get(&(RENDER_CACHE_ENTRIES + 9).to_string()).is_some());
    }
}
