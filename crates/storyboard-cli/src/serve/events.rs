//! Live updates: the store watcher feeds server-sent events to every open page.

use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::Ordering,
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use storyboard_core::Author;
use storyboard_store::{Change, Workspace, open_workspace, watch};
use tiny_http::Request;

use super::http::text_response;
use super::{RESCAN_EVERY, State, project_name};

const MAX_EVENT_STREAMS: usize = 32;
const EVENT_QUEUE_FRAMES: usize = 64;
const HEARTBEAT_EVERY: Duration = Duration::from_secs(3);

pub(super) struct EventClient {
    id: u64,
    sender: SyncSender<String>,
}

struct EventSlot<'a> {
    state: &'a State,
    id: u64,
}

impl Drop for EventSlot<'_> {
    fn drop(&mut self) {
        self.state
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|client| client.id != self.id);
        self.state
            .active_event_streams
            .fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn broadcast(state: &State, event: &str, data: &Value) {
    let frame = format!("event: {event}\ndata: {data}\n\n");
    let mut clients = state
        .clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    clients.retain(|client| client.sender.try_send(frame.clone()).is_ok());
}

pub(super) fn project_list(root: &Path) -> Vec<PathBuf> {
    open_workspace(root)
        .map(|workspace| workspace.projects)
        .unwrap_or_default()
}

pub(super) fn watch_loop(state: &State) {
    let mut last_versions: HashMap<PathBuf, String> = HashMap::new();
    loop {
        let projects = project_list(&state.root);
        let workspace = Workspace {
            root: state.root.clone(),
            projects: projects.clone(),
        };
        let (watcher, changes) = match watch(&workspace) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("watch failed: {error}; retrying");
                thread::sleep(RESCAN_EVERY);
                continue;
            }
        };
        broadcast(state, "workspace", &json!({}));
        let mut rescan_at = Instant::now();
        loop {
            match changes.recv_timeout(Duration::from_millis(500)) {
                Ok(Change::Project { dir, version }) => {
                    if last_versions.get(&dir) != Some(&version) {
                        last_versions.insert(dir.clone(), version.clone());
                        let (author, summary) = commit_for(&dir, &version);
                        broadcast(
                            state,
                            "project",
                            &json!({"project": project_name(&dir), "version": version, "author": author, "summary": summary}),
                        );
                    }
                }
                Ok(Change::Source { dir, path }) => {
                    broadcast(
                        state,
                        "source",
                        &json!({"project": project_name(&dir), "path": path}),
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if rescan_at.elapsed() >= RESCAN_EVERY {
                if project_list(&state.root) != projects {
                    break;
                }
                rescan_at = Instant::now();
            }
        }
        drop(watcher);
    }
}

/// The commit that produced `version`: its author and summary. The document lands before its history
/// record, so this waits briefly for the record; a hand edit has none and reports no author.
pub(super) fn commit_for(dir: &Path, version: &str) -> (Option<String>, Option<String>) {
    for _ in 0..12 {
        if let Ok(records) = storyboard_store::history(dir, 8)
            && let Some(record) = records.into_iter().find(|record| record.version == version)
        {
            return (Some(author_label(&record.author)), Some(record.summary));
        }
        thread::sleep(Duration::from_millis(25));
    }
    (None, None)
}

pub(super) fn author_label(author: &Author) -> String {
    match author {
        Author::Human => "human".into(),
        Author::Agent { agent, .. } => format!("agent:{}", agent.name),
    }
}

/// Server-sent events, written straight to the connection and flushed per event. tiny_http's own
/// response path buffers chunked bodies, which would hold events back.
pub(super) fn events(state: &State, request: Request) {
    if state
        .active_event_streams
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
            (active < MAX_EVENT_STREAMS).then_some(active + 1)
        })
        .is_err()
    {
        let _ = request.respond(text_response(503, "too many live-update connections"));
        return;
    }
    let id = state.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _slot = EventSlot { state, id };
    let (sender, receiver) = mpsc::sync_channel::<String>(EVENT_QUEUE_FRAMES);
    state
        .clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(EventClient { id, sender });
    let mut writer = request.into_writer();
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n: connected\n\n";
    if writer
        .write_all(head.as_bytes())
        .and_then(|()| writer.flush())
        .is_err()
    {
        return;
    }
    loop {
        let frame = match receiver.recv_timeout(HEARTBEAT_EVERY) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => ": keep-alive\n\n".to_string(),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if writer
            .write_all(frame.as_bytes())
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
    }
}
