//! `storyboard serve`: the local workbench. A small synchronous HTTP server bound to 127.0.0.1 that
//! serves the embedded page, reads projects, renders panels, and lands every change through the same
//! `ops_api` commit path the CLI and MCP use. Live updates reach the page over server-sent events
//! fed by the store's watcher, so an agent's write through the CLI or MCP shows without a reload.

mod api;
mod assets;
mod claude_run;
mod events;
mod http;
mod media;
mod render;

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize},
    },
    thread,
    time::Duration,
};

use serde_json::Value;
use storyboard_store::open_workspace;
use tiny_http::{Method, Request, Server};

use crate::ops_api::{self, ApiError};
use api::{
    commit, create_project, export_movie, history, import_movie, import_shot, layout,
    move_movie_boundary, project_state, redo, save_layout, save_viewport, sources,
    split_movie_shot, suggest_cuts, undo, upload_audio, upload_movie, upload_source, viewport,
    workspace_index,
};
use events::{EventClient, events, project_list, watch_loop};
use http::{
    asset, host_is_local, origin_is_local, read_body, read_bytes, respond_bytes, respond_json,
    split_url, text_response,
};
use render::{RenderCache, panel_png, thumbnail};

/// Rendered panels kept in memory. Bounded, so memory does not grow with projects opened.
pub(super) const RENDER_CACHE_ENTRIES: usize = 192;
pub(super) const MAX_MOVIE_UPLOAD_BYTES: u64 = crate::movie_import::MAX_MOVIE_BYTES;
pub(super) const MAX_AUDIO_UPLOAD_BYTES: u64 = 128 * 1024 * 1024;
/// How often the watcher thread re-reads the workspace folder for added or removed projects.
pub(super) const RESCAN_EVERY: Duration = Duration::from_secs(3);

pub struct ServeOptions {
    pub workspace: PathBuf,
    pub port: u16,
    pub open_browser: bool,
}

pub(super) struct State {
    root: PathBuf,
    port: u16,
    clients: Mutex<Vec<EventClient>>,
    next_event_id: AtomicU64,
    active_event_streams: AtomicUsize,
    renders: Mutex<RenderCache>,
    claude: Mutex<Option<Arc<claude_run::Job>>>,
}

pub fn run(options: ServeOptions) -> Result<(), ApiError> {
    let root = options.workspace.canonicalize().map_err(|error| {
        ops_api::validation(format!(
            "workspace {}: {error}",
            options.workspace.display()
        ))
    })?;
    open_workspace(&root)?;
    let address = SocketAddr::from(([127, 0, 0, 1], options.port));
    let server = Server::http(address)
        .map_err(|error| ops_api::validation(format!("could not listen on {address}: {error}")))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|address| address.port())
        .unwrap_or(options.port);
    let state = Arc::new(State {
        root,
        port,
        clients: Mutex::new(Vec::new()),
        next_event_id: AtomicU64::new(1),
        active_event_streams: AtomicUsize::new(0),
        renders: Mutex::new(RenderCache::default()),
        claude: Mutex::new(None),
    });
    let url = format!("http://127.0.0.1:{port}/");
    eprintln!(
        "storyboard workbench: {url}  (workspace {})",
        state.root.display()
    );
    eprintln!("press Ctrl+C to stop");
    {
        let state = Arc::clone(&state);
        thread::spawn(move || watch_loop(&state));
    }
    if options.open_browser {
        open_in_browser(&url);
    }
    for request in server.incoming_requests() {
        let state = Arc::clone(&state);
        thread::spawn(move || handle(&state, request));
    }
    Ok(())
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(error) = result {
        eprintln!("could not open a browser ({error}); open {url} yourself");
    }
}

// ---------------------------------------------------------------- helpers

pub(super) fn project_name(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Resolves a project by folder name, and only among the workspace's own projects.
pub(super) fn resolve_project(state: &State, name: &str) -> Result<PathBuf, ApiError> {
    project_list(&state.root)
        .into_iter()
        .find(|dir| project_name(dir) == name)
        .ok_or_else(|| ops_api::validation(format!("no project named {name:?} in this workspace")))
}

pub(super) fn project_dir(
    state: &State,
    query: &HashMap<String, String>,
) -> Result<PathBuf, ApiError> {
    let name = query
        .get("project")
        .ok_or_else(|| ops_api::validation("project is required"))?;
    resolve_project(state, name)
}

pub(super) fn body_project(state: &State, body: &Value) -> Result<PathBuf, ApiError> {
    let name = body
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("project is required"))?;
    resolve_project(state, name)
}

// ---------------------------------------------------------------- requests

fn handle(state: &State, request: Request) {
    if !host_is_local(&request, state.port) {
        let _ = request.respond(text_response(403, "workbench answers on 127.0.0.1 only"));
        return;
    }
    let (path, query) = split_url(request.url());
    let method = request.method().clone();
    if method == Method::Post && !origin_is_local(&request, state.port) {
        let _ = request.respond(text_response(403, "cross-origin write refused"));
        return;
    }
    match (method, path.as_str()) {
        (Method::Get, "/api/events") => events(state, request),
        (Method::Get, "/api/panel.png") => {
            let response = panel_png(state, &query);
            respond_bytes(request, response);
        }
        (Method::Get, "/api/thumb.png") => {
            let response = thumbnail(state, &query);
            respond_bytes(request, response);
        }
        (Method::Get, "/api/movie.mp4") => media::movie(state, &query, request),
        (Method::Get, "/api/export.mp4") => media::exported_movie(state, &query, request),
        (Method::Get, "/api/audio") => media::audio(state, &query, request),
        (Method::Get, "/api/waveform") => respond_json(request, media::waveform(state, &query)),
        (Method::Get, route) => {
            let result = match route {
                "/api/workspace" => workspace_index(state),
                "/api/project" => project_state(state, &query),
                "/api/history" => history(state, &query),
                "/api/sources" => sources(state, &query),
                "/api/viewport" => viewport(state, &query),
                "/api/layout" => layout(state),
                "/api/schema" => ops_api::schema(query.get("op").map(String::as_str)),
                "/api/claude-job" => claude_run::status(state, &query),
                _ if !route.starts_with("/api/") => {
                    let response = match assets::lookup(route) {
                        Some((bytes, kind)) => asset(bytes, kind),
                        None => text_response(404, "not found"),
                    };
                    let _ = request.respond(response);
                    return;
                }
                _ => {
                    let _ = request.respond(text_response(404, "not found"));
                    return;
                }
            };
            respond_json(request, result);
        }
        (Method::Post, route) => {
            let mut request = request;
            if route == "/api/upload-source" {
                let result = read_bytes(&mut request, 32 * 1024 * 1024)
                    .and_then(|bytes| upload_source(state, &query, &bytes));
                respond_json(request, result);
                return;
            }
            if route == "/api/upload-movie" {
                let result = upload_movie(state, &query, request.as_reader());
                respond_json(request, result);
                return;
            }
            if route == "/api/upload-audio" {
                let result = upload_audio(state, &query, request.as_reader());
                respond_json(request, result);
                return;
            }
            let body = read_body(&mut request);
            let result = body.and_then(|body| match route {
                "/api/commit" => commit(state, body),
                "/api/undo" => undo(state, body),
                "/api/redo" => redo(state, body),
                "/api/viewport" => save_viewport(state, body),
                "/api/layout" => save_layout(state, body),
                "/api/new-project" => create_project(state, body),
                "/api/import-shot" => import_shot(state, body),
                "/api/import-movie" => import_movie(state, body),
                "/api/export-movie" => export_movie(state, body),
                "/api/split-movie-shot" => split_movie_shot(state, body),
                "/api/move-movie-boundary" => move_movie_boundary(state, body),
                "/api/suggest-cuts" => suggest_cuts(state, body),
                "/api/claude-job" => claude_run::start(state, body),
                "/api/claude-cancel" => claude_run::cancel(state, body),
                _ => Err(ops_api::validation(format!("no write route {route}"))),
            });
            respond_json(request, result);
        }
        _ => {
            let _ = request.respond(text_response(405, "method not allowed"));
        }
    }
}
