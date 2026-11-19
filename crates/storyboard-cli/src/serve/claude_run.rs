//! A single bounded Claude job per local workbench. The child receives only the storyboard MCP
//! server and runs without a shell; all project writes still go through the normal ops path.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};
use storyboard_store::{open, version_of};

use super::{State, body_project, project_name};
use crate::ops_api::{self, ApiError};

const MAX_OUTPUT: usize = 65_536;
const DEADLINE: Duration = Duration::from_secs(900);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(super) struct Job {
    id: u64,
    project: String,
    dir: PathBuf,
    version_before: String,
    shots_before: HashMap<String, String>,
    cuts_before: HashMap<String, String>,
    movies_before: HashMap<String, Value>,
    exports_before: HashSet<String>,
    cancel: AtomicBool,
    view: Mutex<Value>,
}

impl Job {
    fn snapshot(&self) -> Value {
        self.view.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn running(&self) -> bool {
        self.snapshot()["status"] == "running"
    }
}

pub(super) fn start(state: &State, body: Value) -> Result<Value, ApiError> {
    let dir = body_project(state, &body)?;
    let project = project_name(&dir);
    let base = required_text(&body, "base_version", 200)?;
    let loaded = open(&dir)?;
    if loaded.version != base {
        return Err(ops_api::validation(
            "project changed; refresh the brief before running Claude",
        ));
    }
    let brief = required_text(&body, "brief", 80_000)?;
    let instruction = required_text(&body, "instruction", 10_000)?;
    let mut slot = state.claude.lock().unwrap_or_else(|p| p.into_inner());
    if slot.as_ref().is_some_and(|job| job.running()) {
        return Err(ops_api::validation(
            "a Claude job is already running; stop it before starting another",
        ));
    }

    let binary = std::env::current_exe()
        .map_err(|error| ops_api::validation(format!("storyboard executable: {error}")))?;
    let config = json!({"mcpServers":{"storyboard":{"command":binary,"args":["mcp",dir]}}});
    let prompt = format!(
        "You are revising the storyboard project {project:?} at {dir:?}. Use that absolute directory for the project argument in every storyboard MCP call. Follow the user's instruction below. Use the storyboard MCP tools to inspect and edit this project. If asked for an MP4 assembled from the current project, call export_movie and report its returned path, mode and version. That tool renders existing boards or source footage; it does not synthesize new footage. Give a concise summary of actual changes. Do not claim new footage was generated unless an output file was imported.\n\nUser instruction:\n{instruction}\n\nSource-bound project brief:\n{brief}"
    );
    let exports_before = export_files(&dir);
    let command = std::env::var_os("NB_STORYBOARD_CLAUDE_CLI").unwrap_or_else(|| "claude".into());
    let mut child_command = Command::new(command);
    child_command
        .args([
            "-p",
            "--output-format",
            "json",
            "--strict-mcp-config",
            "--mcp-config",
            &config.to_string(),
            "--restricted",
            "--permission-mode",
            "dontAsk",
            "--allowedTools",
            "mcp__storyboard__*",
            "--no-session-persistence",
            &prompt,
        ])
        .current_dir(&dir)
        .env("NB_STORYBOARD_MCP_SINGLE_PROJECT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    child_command.process_group(0);
    let child = child_command
        .spawn()
        .map_err(|error| ops_api::validation(format!("could not start Claude: {error}")))?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let job = Arc::new(Job {
        id,
        project: project.clone(),
        dir,
        version_before: loaded.version,
        shots_before: loaded
            .project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .map(|shot| (shot.id.clone(), fingerprint(shot)))
            .collect(),
        cuts_before: loaded
            .project
            .cuts
            .iter()
            .map(|cut| (cut.id.clone(), fingerprint(cut)))
            .collect(),
        movies_before: loaded
            .project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .map(|shot| (shot.id.clone(), json!(shot.source_movie)))
            .collect(),
        exports_before,
        cancel: AtomicBool::new(false),
        view: Mutex::new(json!({"id":id,"project":project,"status":"running","output":""})),
    });
    *slot = Some(Arc::clone(&job));
    thread::spawn(move || run(child, job));
    Ok(slot.as_ref().expect("job set").snapshot())
}

pub(super) fn status(state: &State, query: &HashMap<String, String>) -> Result<Value, ApiError> {
    let project = query
        .get("project")
        .ok_or_else(|| ops_api::validation("project is required"))?;
    let slot = state.claude.lock().unwrap_or_else(|p| p.into_inner());
    Ok(match slot.as_ref() {
        Some(job) if &job.project == project => job.snapshot(),
        _ => json!({"project":project,"status":"none"}),
    })
}

pub(super) fn cancel(state: &State, body: Value) -> Result<Value, ApiError> {
    let project = body
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation("project is required"))?;
    let id = body
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| ops_api::validation("job id is required"))?;
    let slot = state.claude.lock().unwrap_or_else(|p| p.into_inner());
    let job = slot
        .as_ref()
        .filter(|job| job.project == project && job.id == id)
        .ok_or_else(|| ops_api::validation("Claude job not found"))?;
    if job.running() {
        job.cancel.store(true, Ordering::Release);
    }
    Ok(job.snapshot())
}

fn required_text(body: &Value, key: &str, max: usize) -> Result<String, ApiError> {
    let value = body
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ops_api::validation(format!("{key} is required")))?;
    if value.trim().is_empty() || value.len() > max {
        return Err(ops_api::validation(format!(
            "{key} must contain 1 to {max} bytes"
        )));
    }
    Ok(value.to_owned())
}

fn fingerprint(value: &impl serde::Serialize) -> String {
    version_of(&serde_json::to_vec(value).expect("project model serializes"))
}

fn changed_ids(before: &HashMap<String, String>, after: HashMap<String, String>) -> Vec<Value> {
    let mut changes = Vec::new();
    for (id, current) in &after {
        let kind = match before.get(id) {
            None => "added",
            Some(previous) if previous != current => "edited",
            Some(_) => continue,
        };
        changes.push(json!({"id":id,"kind":kind}));
    }
    for id in before.keys() {
        if !after.contains_key(id) {
            changes.push(json!({"id":id,"kind":"removed"}));
        }
    }
    changes.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    changes
}

fn export_files(dir: &std::path::Path) -> HashSet<String> {
    let mut files = HashSet::new();
    let Ok(entries) = fs::read_dir(dir.join(".cache/exports")) else {
        return files;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".mp4")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && entry.path().symlink_metadata().is_ok_and(|meta| {
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() > 0
            })
        {
            files.insert(name);
        }
    }
    files
}

fn read_bounded(mut stream: impl Read) -> String {
    let mut collected = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if collected.len() < MAX_OUTPUT {
                    collected.extend_from_slice(&buffer[..count.min(MAX_OUTPUT - collected.len())]);
                }
            }
        }
    }
    String::from_utf8_lossy(&collected).into_owned()
}

fn terminate_child_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // The Claude child was spawned as the leader of its own process group.
        // Its MCP server inherits that group and the output pipes.
        unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    let _ = child.kill();
}

fn run(mut child: Child, job: Arc<Job>) {
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let out = thread::spawn(move || read_bounded(stdout));
    let err = thread::spawn(move || read_bounded(stderr));
    let began = Instant::now();
    let (status, exit) = loop {
        if job.cancel.load(Ordering::Acquire) {
            terminate_child_tree(&mut child);
            break ("canceled", child.wait().ok());
        }
        if began.elapsed() >= DEADLINE {
            terminate_child_tree(&mut child);
            break ("timed_out", child.wait().ok());
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                break (
                    if exit.success() {
                        "completed"
                    } else {
                        "failed"
                    },
                    Some(exit),
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(_) => break ("failed", child.wait().ok()),
        }
    };
    // The CLI may exit while an MCP child still owns its stdout/stderr pipes.
    // Stop that isolated process group before joining the pipe readers.
    terminate_child_tree(&mut child);
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&stdout).ok();
    let failed = parsed
        .as_ref()
        .is_some_and(|value| value["is_error"] == true);
    let final_status = if status == "completed" && failed {
        "failed"
    } else {
        status
    };
    let output = parsed
        .as_ref()
        .and_then(|value| value["result"].as_str())
        .unwrap_or(&stdout);
    let (version_after, shot_changes, cut_changes, movie_changes, project_error) =
        match open(&job.dir) {
            Ok(loaded) => {
                let shot_changes = changed_ids(
                    &job.shots_before,
                    loaded
                        .project
                        .scenes
                        .iter()
                        .flat_map(|scene| &scene.shots)
                        .map(|shot| (shot.id.clone(), fingerprint(shot)))
                        .collect(),
                );
                let cut_changes = changed_ids(
                    &job.cuts_before,
                    loaded
                        .project
                        .cuts
                        .iter()
                        .map(|cut| (cut.id.clone(), fingerprint(cut)))
                        .collect(),
                );
                let changes = loaded
                    .project
                    .scenes
                    .iter()
                    .flat_map(|scene| &scene.shots)
                    .filter_map(|shot| {
                        let current = json!(shot.source_movie);
                        (job.movies_before.get(&shot.id) != Some(&current))
                            .then(|| json!({"shot": shot.id, "source_movie": shot.source_movie}))
                    })
                    .collect::<Vec<_>>();
                (
                    Some(loaded.version),
                    shot_changes,
                    cut_changes,
                    changes,
                    None,
                )
            }
            Err(error) => (
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(error.to_string()),
            ),
        };
    let project_changed = version_after
        .as_ref()
        .is_some_and(|version| *version != job.version_before);
    let mut new_exports: Vec<_> = export_files(&job.dir)
        .difference(&job.exports_before)
        .cloned()
        .collect();
    new_exports.sort();
    *job.view.lock().unwrap_or_else(|p| p.into_inner()) = json!({
        "id": job.id,
        "project": job.project,
        "status": if project_error.is_some() { "failed" } else { final_status },
        "output": output,
        "error": stderr,
        "exit_code": exit.and_then(|value| value.code()),
        "version_before": job.version_before,
        "version_after": version_after,
        "project_changed": project_changed,
        "shot_changes": shot_changes,
        "cut_changes": cut_changes,
        "movie_changes": movie_changes,
        "new_exports": new_exports,
        "project_error": project_error,
    });
}
