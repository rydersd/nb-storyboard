//! `storyboard serve`: the workbench server. Spec: wiki/product-spec/stories/S9-workbench.md
//! Each test starts the real binary on a free loopback port and talks raw HTTP to it.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use assert_cmd::cargo::CommandCargoExt;
use serde_json::{Value, json};
use tempfile::TempDir;

struct Served {
    child: Child,
    port: u16,
    _workspace: TempDir,
    project: PathBuf,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn storyboard() -> Command {
    Command::cargo_bin("storyboard").expect("storyboard binary")
}

fn cli(args: &[&str]) -> Value {
    let output = storyboard()
        .args(args)
        .env("NB_AUTHOR", "agent:test")
        .output()
        .expect("run storyboard");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn version(project: &Path) -> String {
    cli(&["show", project.to_str().expect("UTF-8")])["version"]
        .as_str()
        .expect("version")
        .to_string()
}

#[test]
fn creates_project_inside_workspace_and_reopens_saved_edit() {
    let server = serve();
    let body = json!({"project":"second_movie", "title":"Second movie"});
    let (status, _, bytes) = request(server.port, "POST", "/api/new-project", &[], Some(&body));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created["project"], "second_movie");
    assert!(
        server
            ._workspace
            .path()
            .join("second_movie/project.json")
            .is_file()
    );

    let (status, _, _) = request(server.port, "POST", "/api/new-project", &[], Some(&body));
    assert_eq!(status, 400, "a collision must not replace a project");
    let (status, _, _) = request(
        server.port,
        "POST",
        "/api/new-project",
        &[],
        Some(&json!({"project":"../escape", "title":"Escape"})),
    );
    assert_eq!(status, 400);
    let (status, _, _) = request(
        server.port,
        "POST",
        "/api/new-project",
        &[],
        Some(&json!({"project":"invalid_fps", "title":"Invalid", "fps":29})),
    );
    assert_eq!(status, 400);
    assert!(!server._workspace.path().join("invalid_fps").exists());

    let (status, _, bytes) = request(
        server.port,
        "GET",
        "/api/project?project=second_movie",
        &[],
        None,
    );
    assert_eq!(status, 200);
    let opened: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(opened["document"]["fps"], 24);
    assert_eq!(opened["document"]["format"]["width"], 1440);
    let (status, _, bytes) = request(
        server.port,
        "POST",
        "/api/commit",
        &[],
        Some(
            &json!({"project":"second_movie", "base_version":opened["version"],
            "ops":[{"add_scene":{"title":"Revision"}}]}),
        ),
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    let (status, _, bytes) = request(
        server.port,
        "GET",
        "/api/project?project=second_movie",
        &[],
        None,
    );
    assert_eq!(status, 200);
    let reopened: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reopened["document"]["scenes"][0]["title"], "Revision");
}

#[test]
fn redo_route_returns_the_reapplied_document() {
    let served = serve();
    let base = version(&served.project);
    let (status, changed) = post_json(
        served.port,
        "/api/commit",
        &json!({"project":"night","base_version":base,"ops":[{"update_shot":{"shot":"shot-1","patch":{"title":"Changed"}}}]}),
    );
    assert_eq!(status, 200, "{changed}");
    let (status, undone) = post_json(
        served.port,
        "/api/undo",
        &json!({"project":"night","base_version":changed["version"]}),
    );
    assert_eq!(status, 200, "{undone}");
    let (status, redone) = post_json(
        served.port,
        "/api/redo",
        &json!({"project":"night","base_version":undone["version"]}),
    );
    assert_eq!(status, 200, "{redone}");
    assert_eq!(redone["version"], changed["version"]);
    assert_eq!(
        redone["document"]["scenes"][0]["shots"][0]["title"],
        "Changed"
    );
    assert!(
        redone["summary"]
            .as_str()
            .expect("summary")
            .contains("redo")
    );
}

#[test]
fn split_revised_shot_into_independent_undoable_placements() {
    let server = serve();
    let (status, prepared) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),"ops":[{"batch":{"ops":[
            {"add_panel":{"shot":"shot-1","take":"take-1","time":0.5}},
            {"add_cut":{"id":"cut-revision","name":"Revision"}},
            {"add_clip":{"cut":"cut-revision","id":"clip-first","source":{"shot":{"shot":"shot-1"}}}}
        ]}}]}),
    );
    assert_eq!(status, 200, "{prepared}");
    let original = &prepared["document"]["scenes"][0]["shots"][0];
    assert_eq!(original["takes"][0]["panels"].as_array().unwrap().len(), 2);
    let (status, before) = get_json(server.port, "/api/project?project=night&cut=cut-revision");
    assert_eq!(status, 200, "{before}");
    assert_eq!(before["sequence"]["frames"], 48);

    let (status, split) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":prepared["version"],"ops":[{"batch":{"ops":[
            {"duplicate_shot":{"shot":"shot-1","id":"shot-revision"}},
            {"update_clip":{"cut":"cut-revision","clip":"clip-first","patch":{"out":12}}},
            {"add_clip":{"cut":"cut-revision","id":"clip-second","source":{"shot":{"shot":"shot-revision"}},"index":1}},
            {"update_clip":{"cut":"cut-revision","clip":"clip-second","patch":{"in":12,"out":null}}}
        ]}}]}),
    );
    assert_eq!(status, 200, "{split}");
    let shots = split["document"]["scenes"][0]["shots"].as_array().unwrap();
    assert_eq!(shots[1]["id"], "shot-revision");
    assert_ne!(shots[0]["selected_take"], shots[1]["selected_take"]);
    assert_ne!(
        shots[0]["takes"][0]["panels"][0]["id"],
        shots[1]["takes"][0]["panels"][0]["id"]
    );
    let clips = split["document"]["cuts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cut| cut["id"] == "cut-revision")
        .unwrap()["clips"]
        .as_array()
        .unwrap();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0]["out"], 12);
    assert_eq!(clips[1]["in"], 12);
    assert_eq!(clips[1]["source"]["shot"]["shot"], "shot-revision");
    let (status, compiled) = get_json(server.port, "/api/project?project=night&cut=cut-revision");
    assert_eq!(status, 200, "{compiled}");
    assert_eq!(compiled["sequence"]["frames"], 48);

    let (status, rolled) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":split["version"],"ops":[{"batch":{"ops":[
            {"update_clip":{"cut":"cut-revision","clip":"clip-first","patch":{"out":15}}},
            {"update_clip":{"cut":"cut-revision","clip":"clip-second","patch":{"in":15}}}
        ]}}]}),
    );
    assert_eq!(status, 200, "{rolled}");
    let (status, rolled_sequence) =
        get_json(server.port, "/api/project?project=night&cut=cut-revision");
    assert_eq!(status, 200, "{rolled_sequence}");
    assert_eq!(rolled_sequence["sequence"]["frames"], 48);
    assert_eq!(rolled_sequence["sequence"]["items"][0]["end"], 15);
    assert_eq!(rolled_sequence["sequence"]["items"][1]["offset"], 15);
    let (status, reset_boundary) = post_json(
        server.port,
        "/api/undo",
        &json!({"project":"night","base_version":rolled["version"]}),
    );
    assert_eq!(status, 200, "{reset_boundary}");
    assert_eq!(
        reset_boundary["document"]["cuts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|cut| cut["id"] == "cut-revision")
            .unwrap()["clips"][0]["out"],
        12
    );

    let (status, undone) = post_json(
        server.port,
        "/api/undo",
        &json!({"project":"night","base_version":reset_boundary["version"]}),
    );
    assert_eq!(status, 200, "{undone}");
    assert_eq!(
        undone["document"]["scenes"][0]["shots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let cut = undone["document"]["cuts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cut| cut["id"] == "cut-revision")
        .unwrap();
    assert_eq!(cut["clips"].as_array().unwrap().len(), 1);
    assert_eq!(cut["clips"][0]["out"], Value::Null);
}

#[test]
fn merges_adjacent_parts_of_one_shot_without_changing_the_cut() {
    let server = serve();
    let (status, prepared) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),"ops":[{"batch":{"ops":[
            {"add_cut":{"id":"cut-merge","name":"Merge"}},
            {"add_clip":{"cut":"cut-merge","id":"clip-left","source":{"shot":{"shot":"shot-1"}}}},
            {"add_clip":{"cut":"cut-merge","id":"clip-right","source":{"shot":{"shot":"shot-1"}}}},
            {"update_clip":{"cut":"cut-merge","clip":"clip-left","patch":{"out":20}}},
            {"update_clip":{"cut":"cut-merge","clip":"clip-right","patch":{"in":20}}},
            {"add_sticky":{"id":"sticky-merge","text":"Review the second part","kind":"note","author":"human","pin":"clip:cut-merge/clip-right","created":"2026-11-08T00:00:00Z"}}
        ]}}]}),
    );
    assert_eq!(status, 200, "{prepared}");
    let (status, before) = get_json(server.port, "/api/project?project=night&cut=cut-merge");
    assert_eq!(status, 200, "{before}");
    assert_eq!(before["sequence"]["frames"], 48);
    assert_eq!(before["sequence"]["items"][1]["start"], 20);
    let (status, merged) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":prepared["version"],"ops":[{"merge_clips":{"cut":"cut-merge","left":"clip-left","right":"clip-right"}}]}),
    );
    assert_eq!(status, 200, "{merged}");
    let (status, after) = get_json(server.port, "/api/project?project=night&cut=cut-merge");
    assert_eq!(status, 200, "{after}");
    assert_eq!(after["sequence"]["frames"], 48);
    assert_eq!(after["sequence"]["items"].as_array().unwrap().len(), 1);
    assert!(
        after["document"]["stickies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sticky| sticky["id"] == "sticky-merge"
                && sticky["pin"] == "clip:cut-merge/clip-left")
    );
    assert_eq!(
        after["document"]["cuts"][0]["clips"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (status, undone) = post_json(
        server.port,
        "/api/undo",
        &json!({"project":"night","base_version":merged["version"]}),
    );
    assert_eq!(status, 200, "{undone}");
    assert_eq!(
        undone["document"]["cuts"][0]["clips"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (status, restored) = get_json(server.port, "/api/project?project=night&cut=cut-merge");
    assert_eq!(status, 200, "{restored}");
    assert_eq!(restored["sequence"]["frames"], 48);
    assert_eq!(restored["sequence"]["items"][1]["start"], 20);
    assert!(
        restored["document"]["stickies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sticky| sticky["id"] == "sticky-merge"
                && sticky["pin"] == "clip:cut-merge/clip-right")
    );
}

#[test]
fn imports_mp4_into_editable_source_linked_shots() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return; // The app reports the missing required tools when the import is attempted.
    }
    let server = serve();
    let movie = server._workspace.path().join("sample.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=320x180:r=24:d=2",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("ffmpeg fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(&movie).unwrap();
    let folder = "12345678-1234-1234-1234-123456789abc";
    let (status, _, body) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-movie?project=night&folder={folder}"),
        &[],
        &bytes,
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let uploaded: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(uploaded["media"]["width"], 320);
    let base = version(&server.project);
    let (status, _, body) = request(
        server.port,
        "POST",
        "/api/import-movie",
        &[],
        Some(
            &json!({"project":"night","base_version":base,"folder":folder,
            "scene":"scene-1","title":"Rough","interval":1.0}),
        ),
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let result: Value = serde_json::from_slice(&body).unwrap();
    let shots = result["document"]["scenes"][0]["shots"].as_array().unwrap();
    assert_eq!(shots.len(), 4);
    let imported = &shots[2..];
    assert_eq!(imported[0]["source_movie"]["start"], 0.0);
    assert_eq!(imported[0]["source_movie"]["end"], 1.0);
    assert_eq!(imported[1]["source_movie"]["start"], 1.0);
    assert!(
        imported[0]["takes"][0]["panels"][0]["layers"][0]["src"]
            .as_str()
            .unwrap()
            .ends_with("frame-001.png")
    );
    assert!(
        server
            .project
            .join("sources/imported-movies")
            .join(folder)
            .join("original.mp4")
            .is_file()
    );
    let handoff = cli(&["show", server.project.to_str().unwrap()]);
    assert_eq!(
        handoff["scenes"][0]["shots"][2]["source_movie"]["start"],
        0.0
    );
    let (status, headers, chunk) = request_bytes(
        server.port,
        "GET",
        &format!("/api/movie.mp4?project=night&folder={folder}"),
        &[("Range", "bytes=0-15")],
        &[],
    );
    assert_eq!(status, 206);
    assert!(headers.contains("Content-Range: bytes 0-15/"), "{headers}");
    assert_eq!(chunk, bytes[..16]);
    let imported_shot = imported[0]["id"].as_str().unwrap();
    let (status, cut_result) = post_json(
        server.port,
        "/api/commit",
        &json!({
            "project":"night", "base_version":version(&server.project),
            "ops":[{"add_cut":{"name":"Revision"}}]
        }),
    );
    assert_eq!(status, 200, "{cut_result}");
    let cut = cut_result["ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|id| id.starts_with("cut-"))
        .unwrap();
    let (status, placed) = post_json(
        server.port,
        "/api/commit",
        &json!({
            "project":"night", "base_version":version(&server.project),
            "ops":[{"add_clip":{"cut":cut,"source":{"shot":{"shot":imported_shot}}}}]
        }),
    );
    assert_eq!(status, 200, "{placed}");
    let clip = placed["ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|id| id.starts_with("clip-"))
        .unwrap();
    let next_shot = imported[1]["id"].as_str().unwrap();
    let (status, second) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),
            "ops":[{"add_clip":{"cut":cut,"source":{"shot":{"shot":next_shot}}}}]}),
    );
    assert_eq!(status, 200, "{second}");
    let second_clip = second["ids"][0].as_str().unwrap();
    let before_boundary = version(&server.project);
    let (status, _) = post_json(
        server.port,
        "/api/move-movie-boundary",
        &json!({"project":"night","base_version":before_boundary,
            "cut":cut,"left_clip":clip,"frame":0}),
    );
    assert_eq!(status, 400);
    assert_eq!(version(&server.project), before_boundary);
    let (status, moved) = post_json(
        server.port,
        "/api/move-movie-boundary",
        &json!({"project":"night","base_version":before_boundary,
            "cut":cut,"left_clip":clip,"frame":18}),
    );
    assert_eq!(status, 200, "{moved}");
    let changed = moved["document"]["scenes"][0]["shots"].as_array().unwrap();
    assert_eq!(changed[2]["source_movie"]["end"], 0.75);
    assert_eq!(changed[3]["source_movie"]["start"], 0.75);
    assert_eq!(changed[2]["duration"], 0.75);
    assert_eq!(changed[3]["duration"], 1.25);
    let (status, rolled) = get_json(
        server.port,
        &format!("/api/project?project=night&cut={cut}"),
    );
    assert_eq!(status, 200);
    assert_eq!(rolled["sequence"]["frames"], 48);
    for shot in &changed[2..4] {
        let source = shot["takes"][0]["panels"][0]["layers"][0]["src"]
            .as_str()
            .unwrap();
        assert!(server.project.join(source).is_file(), "{source}");
    }
    let (status, undone) = post_json(
        server.port,
        "/api/undo",
        &json!({"project":"night","base_version":version(&server.project)}),
    );
    assert_eq!(status, 200, "{undone}");
    assert_eq!(
        undone["document"]["scenes"][0]["shots"][2]["source_movie"]["end"],
        1.0
    );
    assert_eq!(
        undone["document"]["scenes"][0]["shots"][3]["source_movie"]["start"],
        1.0
    );
    let (status, reused) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),
            "ops":[{"add_clip":{"cut":cut,"source":{"shot":{"shot":next_shot}}}}]}),
    );
    assert_eq!(status, 200, "{reused}");
    let reuse_clip = reused["ids"][0].as_str().unwrap();
    let reused_version = version(&server.project);
    let (status, _) = post_json(
        server.port,
        "/api/move-movie-boundary",
        &json!({"project":"night","base_version":reused_version,
            "cut":cut,"left_clip":clip,"frame":18}),
    );
    assert_eq!(status, 400, "reused shots must not be silently retimed");
    assert_eq!(version(&server.project), reused_version);
    let (status, removed_reuse) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),
            "ops":[{"remove_clip":{"cut":cut,"clip":reuse_clip}}]}),
    );
    assert_eq!(status, 200, "{removed_reuse}");
    let (status, removed) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),
            "ops":[{"remove_clip":{"cut":cut,"clip":second_clip}}]}),
    );
    assert_eq!(status, 200, "{removed}");
    let before = version(&server.project);
    let (status, _) = post_json(
        server.port,
        "/api/split-movie-shot",
        &json!({
            "project":"night", "base_version":before, "cut":cut, "clip":clip, "frame":0
        }),
    );
    assert_eq!(status, 400);
    assert_eq!(version(&server.project), before);
    let (status, split) = post_json(
        server.port,
        "/api/split-movie-shot",
        &json!({
            "project":"night", "base_version":before, "cut":cut, "clip":clip, "frame":12
        }),
    );
    assert_eq!(status, 200, "{split}");
    let revised = split["document"]["scenes"][0]["shots"].as_array().unwrap();
    assert_eq!(revised.len(), 5);
    assert_eq!(revised[2]["source_movie"]["end"], 0.5);
    assert_eq!(revised[3]["source_movie"]["start"], 0.5);
    assert_eq!(revised[3]["source_movie"]["end"], 1.0);
    assert_eq!(
        split["document"]["cuts"][0]["clips"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for shot in &revised[2..4] {
        let src = shot["takes"][0]["panels"][0]["layers"][0]["src"]
            .as_str()
            .unwrap();
        assert!(server.project.join(src).is_file(), "{src}");
    }
    let (status, undone) = post_json(
        server.port,
        "/api/undo",
        &json!({
            "project":"night", "base_version":version(&server.project)
        }),
    );
    assert_eq!(status, 200, "{undone}");
    assert_eq!(
        undone["document"]["scenes"][0]["shots"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        undone["document"]["cuts"][0]["clips"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let panel = undone["document"]["scenes"][0]["shots"][2]["takes"][0]["panels"][0]["id"]
        .as_str()
        .unwrap();
    let take = undone["document"]["scenes"][0]["shots"][2]["takes"][0]["id"]
        .as_str()
        .unwrap();
    let (status, revised_panel) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":undone["version"],
            "ops":[{"update_panel":{"shot":imported_shot,"take":take,"panel":panel,
                "patch":{"time":0.25}}}]}),
    );
    assert_eq!(status, 200, "{revised_panel}");
    let (status, refusal) = post_json(
        server.port,
        "/api/split-movie-shot",
        &json!({"project":"night","base_version":revised_panel["version"],
            "cut":cut,"clip":clip,"frame":12}),
    );
    assert_eq!(status, 400, "{refusal}");
    assert_eq!(version(&server.project), revised_panel["version"]);
}

#[test]
fn interval_import_folds_subframe_tail_into_previous_shot() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let movie = server._workspace.path().join("tail.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x64:r=24:d=2",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("ffmpeg fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let folder = "12345678-1234-1234-1234-123456789abd";
    let (status, _, body) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-movie?project=night&folder={folder}"),
        &[],
        &std::fs::read(&movie).unwrap(),
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let (status, result) = post_json(
        server.port,
        "/api/import-movie",
        &json!({"project":"night", "base_version":version(&server.project),
            "folder":folder, "scene":"scene-1", "title":"Tail", "interval":0.99}),
    );
    assert_eq!(status, 200, "{result}");
    let shots = result["document"]["scenes"][0]["shots"].as_array().unwrap();
    let imported = &shots[2..];
    assert_eq!(imported.len(), 2, "a subframe tail must not become a shot");
    assert_eq!(imported[0]["source_movie"]["end"], 1.0);
    assert_eq!(imported[1]["source_movie"]["start"], 1.0);
    assert_eq!(imported[1]["source_movie"]["end"], 2.0);
    assert_eq!(imported[0]["duration"], 1.0);
    assert_eq!(imported[1]["duration"], 1.0);
}

#[test]
fn rejects_mp4_with_video_codec_the_browser_cannot_play() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let movie = server._workspace.path().join("unsupported.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x64:r=24:d=1",
            "-c:v",
            "mpeg4",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("ffmpeg fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let folder = "12345678-1234-1234-1234-123456789abe";
    let (status, _, body) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-movie?project=night&folder={folder}"),
        &[],
        &std::fs::read(&movie).unwrap(),
    );
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    assert!(String::from_utf8_lossy(&body).contains("H.264"));
    assert!(
        !server
            .project
            .join(format!("sources/imported-movies/{folder}/original.mp4"))
            .exists()
    );

    let audio_movie = server._workspace.path().join("unsupported-audio.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x64:r=24:d=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:v",
            "libx264",
            "-c:a",
            "ac3",
            "-y",
        ])
        .arg(&audio_movie)
        .output()
        .expect("ffmpeg audio fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let folder = "12345678-1234-1234-1234-123456789abf";
    let (status, _, body) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-movie?project=night&folder={folder}"),
        &[],
        &std::fs::read(&audio_movie).unwrap(),
    );
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    assert!(String::from_utf8_lossy(&body).contains("AAC"));
    assert!(
        !server
            .project
            .join(format!("sources/imported-movies/{folder}/original.mp4"))
            .exists()
    );
}

#[test]
fn streams_agent_replaced_movie_with_byte_ranges() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let movie = server._workspace.path().join("generated.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x90:r=24:d=1",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("ffmpeg fixture");
    assert!(output.status.success());
    let replaced = cli(&[
        "replace-movie",
        server.project.to_str().unwrap(),
        "--base-version",
        &version(&server.project),
        "--author",
        "agent:test",
        "--shot",
        "shot-1",
        "--source",
        movie.to_str().unwrap(),
    ]);
    let folder = replaced["path"]
        .as_str()
        .and_then(|path| path.strip_prefix("sources/imported-movies/"))
        .and_then(|path| path.strip_suffix("/original.mp4"))
        .expect("generated movie folder");
    let (status, headers, chunk) = request_bytes(
        server.port,
        "GET",
        &format!("/api/movie.mp4?project=night&folder={folder}"),
        &[("Range", "bytes=0-15")],
        &[],
    );
    assert_eq!(status, 206, "{headers}");
    assert!(headers.contains("Content-Type: video/mp4"));
    assert_eq!(chunk, std::fs::read(&movie).unwrap()[..16]);
}

#[test]
fn exports_cut_over_http_and_downloads_mp4_with_byte_ranges() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let stale = json!({"project":"night", "base_version":"stale", "mode":"boards"});
    let (status, _) = post_json(server.port, "/api/export-movie", &stale);
    assert_eq!(status, 400);
    let current = version(&server.project);
    let body = json!({"project":"night", "base_version":current, "mode":"boards"});
    let (status, rendered) = post_json(server.port, "/api/export-movie", &body);
    assert_eq!(status, 200, "{rendered}");
    let file = rendered["file"].as_str().expect("export file");
    assert_eq!(rendered["frames"], 96);
    let path = server.project.join(".cache/exports").join(file);
    assert!(path.is_file());
    assert_eq!(
        version(&server.project),
        current,
        "export leaves project unchanged"
    );
    let (status, headers, chunk) = request_bytes(
        server.port,
        "GET",
        &format!("/api/export.mp4?project=night&file={file}"),
        &[("Range", "bytes=0-15")],
        &[],
    );
    assert_eq!(status, 206, "{headers}");
    assert!(headers.contains("Content-Type: video/mp4"));
    assert!(headers.contains("Content-Disposition: attachment"));
    assert_eq!(chunk, std::fs::read(path).unwrap()[..16]);
    let (status, _, _) = request_bytes(
        server.port,
        "GET",
        "/api/export.mp4?project=night&file=..%2Fproject.json",
        &[],
        &[],
    );
    assert_eq!(status, 404);
}

#[test]
fn suggests_reviewable_movie_cuts_without_editing_project() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let served = serve();
    let movie = served._workspace.path().join("hard-cut.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x90:r=24:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=160x90:r=24:d=1",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("ffmpeg fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let folder = "12345678-1234-1234-1234-123456789abc";
    let (status, _, _) = request_bytes(
        served.port,
        "POST",
        &format!("/api/upload-movie?project=night&folder={folder}"),
        &[],
        &std::fs::read(&movie).unwrap(),
    );
    assert_eq!(status, 200);
    let before = version(&served.project);
    let (status, result) = post_json(
        served.port,
        "/api/suggest-cuts",
        &json!({
            "project":"night", "folder":folder, "threshold":10
        }),
    );
    assert_eq!(status, 200, "{result}");
    let marks = result["cut_times"].as_array().unwrap();
    assert_eq!(marks.len(), 1, "{result}");
    assert!((marks[0].as_f64().unwrap() - 1.0).abs() < 1.0 / 24.0);
    assert_eq!(
        version(&served.project),
        before,
        "suggestions must not edit the project"
    );
    let (status, sensitive) = post_json(
        served.port,
        "/api/suggest-cuts",
        &json!({"project":"night", "folder":folder, "threshold":0.5}),
    );
    assert_eq!(status, 200, "{sensitive}");
    assert_eq!(sensitive["cut_times"].as_array().unwrap().len(), 1);
    assert_eq!(version(&served.project), before);
    let (status, _) = post_json(
        served.port,
        "/api/suggest-cuts",
        &json!({
            "project":"night", "folder":folder, "threshold":0
        }),
    );
    assert_eq!(status, 400);
    let (status, _) = post_json(
        served.port,
        "/api/suggest-cuts",
        &json!({"project":"night", "folder":folder, "threshold":0.09}),
    );
    assert_eq!(status, 400);
}

#[test]
fn imports_mp4_at_explicit_cut_times_without_partial_edits() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let movie = server._workspace.path().join("marked.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=green:s=320x180:r=30:d=2",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("ffmpeg fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let folder = "abcdefab-1234-1234-1234-abcdefabcdef";
    let (status, _, _) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-movie?project=night&folder={folder}"),
        &[],
        &std::fs::read(movie).unwrap(),
    );
    assert_eq!(status, 200);
    let base = version(&server.project);
    let body = |cuts: Value| {
        json!({"project":"night","base_version":base,"folder":folder,
        "scene":"scene-1","title":"Marked","interval":1.0,"cut_times":cuts})
    };
    for invalid in [json!([0.5, 0.5]), json!([0.001]), json!([1.99])] {
        let (status, error) = post_json(server.port, "/api/import-movie", &body(invalid));
        assert_eq!(status, 400, "{error}");
        assert_eq!(version(&server.project), base);
    }
    let (status, result) = post_json(server.port, "/api/import-movie", &body(json!([0.5, 1.5])));
    assert_eq!(status, 200, "{result}");
    let shots = result["document"]["scenes"][0]["shots"].as_array().unwrap();
    let imported = &shots[2..];
    assert_eq!(imported.len(), 3);
    assert_eq!(imported[0]["source_movie"]["start"], 0.0);
    assert_eq!(imported[0]["source_movie"]["end"], 0.5);
    assert_eq!(imported[1]["source_movie"]["start"], 0.5);
    assert_eq!(imported[1]["source_movie"]["end"], 1.5);
    assert_eq!(imported[2]["source_movie"]["start"], 1.5);
    assert_eq!(imported[2]["source_movie"]["end"], 2.0);
    assert_eq!(imported[1]["duration"], 1.0);
    assert!(
        server
            .project
            .join(format!("sources/imported-movies/{folder}/frame-003.png"))
            .is_file()
    );
}

#[test]
fn attaches_audio_to_a_shot_and_streams_saved_source() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let audio = server._workspace.path().join("voice.wav");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-y",
        ])
        .arg(&audio)
        .output()
        .expect("audio fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(&audio).unwrap();
    let folder = "12345678-1234-1234-1234-123456789abc";
    let (status, _, body) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-audio?project=night&folder={folder}&ext=wav"),
        &[],
        &bytes,
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let uploaded: Value = serde_json::from_slice(&body).unwrap();
    let source = uploaded["path"].as_str().unwrap();
    assert!(source.ends_with("original.wav"));
    let duration = uploaded["media"]["duration"].as_f64().unwrap();
    assert!((duration - 3.0).abs() < 0.01);
    let (status, result) = post_json(
        server.port,
        "/api/commit",
        &json!({
            "project":"night", "base_version":version(&server.project),
            "ops":[{"update_shot":{"shot":"shot-1", "patch":{
                "source_audio":{"src":source,"start":0.0,"end":duration},"duration":duration
            }}}]
        }),
    );
    assert_eq!(status, 200, "{result}");
    let (status, reopened) = get_json(server.port, "/api/project?project=night");
    assert_eq!(status, 200);
    assert_eq!(
        reopened["document"]["scenes"][0]["shots"][0]["source_audio"]["src"],
        source
    );
    let (status, headers, chunk) = request_bytes(
        server.port,
        "GET",
        &format!("/api/audio?project=night&folder={folder}&ext=wav"),
        &[("Range", "bytes=0-15")],
        &[],
    );
    assert_eq!(status, 206);
    assert!(headers.contains("Content-Type: audio/wav"), "{headers}");
    assert_eq!(chunk, bytes[..16]);
    let before = version(&server.project);
    let (status, waveform) = get_json(
        server.port,
        &format!("/api/waveform?project=night&kind=audio&folder={folder}&ext=wav&start=0&end=3"),
    );
    assert_eq!(status, 200, "{waveform}");
    let peaks = waveform["peaks"].as_array().unwrap();
    assert_eq!(peaks.len(), 128);
    assert!(peaks.iter().any(|peak| peak.as_u64().unwrap() > 0));
    assert_eq!(version(&server.project), before, "waveform is read-only");
    let (status, _) = get_json(
        server.port,
        &format!("/api/waveform?project=night&kind=audio&folder={folder}&ext=wav&start=0&end=301"),
    );
    assert_eq!(status, 400);
}

#[test]
fn placement_voice_extends_only_one_reused_shot_and_undo_restores_cut() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let server = serve();
    let audio = server._workspace.path().join("placement.wav");
    let encoded = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-y",
        ])
        .arg(&audio)
        .output()
        .expect("audio fixture");
    assert!(encoded.status.success());
    let folder = "22222222-2222-4222-8222-222222222222";
    let (status, _, body) = request_bytes(
        server.port,
        "POST",
        &format!("/api/upload-audio?project=night&folder={folder}&ext=wav"),
        &[],
        &std::fs::read(&audio).unwrap(),
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let uploaded: Value = serde_json::from_slice(&body).unwrap();
    let source = uploaded["path"].as_str().unwrap();
    let (status, prepared) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&server.project),"ops":[{"batch":{"ops":[
            {"add_cut":{"id":"cut-placement-vo","name":"Placement VO"}},
            {"add_clip":{"cut":"cut-placement-vo","id":"clip-vo","source":{"shot":{"shot":"shot-1"}}}},
            {"add_clip":{"cut":"cut-placement-vo","id":"clip-plain","source":{"shot":{"shot":"shot-1"}}}}
        ]}}]}),
    );
    assert_eq!(status, 200, "{prepared}");
    let (status, attached) = post_json(
        server.port,
        "/api/commit",
        &json!({"project":"night","base_version":prepared["version"],"ops":[
            {"update_clip":{"cut":"cut-placement-vo","clip":"clip-vo","patch":{
                "source_audio":{"src":source,"start":0.0,"end":3.0}
            }}}
        ]}),
    );
    assert_eq!(status, 200, "{attached}");
    let (status, reopened) = get_json(
        server.port,
        "/api/project?project=night&cut=cut-placement-vo",
    );
    assert_eq!(status, 200, "{reopened}");
    assert_eq!(reopened["sequence"]["frames"], 120);
    assert_eq!(reopened["sequence"]["items"][1]["start"], 72);
    assert_eq!(
        reopened["document"]["cuts"][0]["clips"][0]["source_audio"]["src"],
        source
    );
    assert!(reopened["document"]["cuts"][0]["clips"][1]["source_audio"].is_null());
    let shown = storyboard_cli::ops_api::show(&server.project, Some("cut-placement-vo")).unwrap();
    assert_eq!(shown["cuts"][0]["clips"][0]["source_audio"]["src"], source);
    assert_eq!(shown["shots"][1]["start"], 72);
    assert!(
        storyboard_store::check_refs(
            &server.project,
            &storyboard_store::open(&server.project).unwrap().project
        )
        .unwrap()
        .is_empty()
    );
    let source_entry = storyboard_store::scan_sources(&server.project)
        .unwrap()
        .into_iter()
        .find(|entry| entry.path == source)
        .expect("placement source is indexed");
    assert!(
        source_entry
            .uses
            .iter()
            .any(|usage| usage.target == "clip:cut-placement-vo/clip-vo")
    );
    let (status, undone) = post_json(
        server.port,
        "/api/undo",
        &json!({"project":"night","base_version":attached["version"]}),
    );
    assert_eq!(status, 200, "{undone}");
    let (status, reset) = get_json(
        server.port,
        "/api/project?project=night&cut=cut-placement-vo",
    );
    assert_eq!(status, 200, "{reset}");
    assert_eq!(reset["sequence"]["frames"], 96);
}

/// A workspace with one project, one scene and two shots, served on a free port.
fn serve() -> Served {
    serve_with_claude(None)
}

fn serve_with_claude(claude: Option<&Path>) -> Served {
    let workspace = TempDir::new().expect("workspace");
    let project = workspace.path().join("night");
    let dir = project.to_str().expect("UTF-8");
    cli(&["new", dir, "--title", "Night"]);
    cli(&[
        "add-scene",
        dir,
        "--base-version",
        &version(&project),
        "--title",
        "Platform",
    ]);
    for title in ["Arrival", "Departure"] {
        cli(&[
            "add-shot",
            dir,
            "--base-version",
            &version(&project),
            "--scene",
            "scene-1",
            "--title",
            title,
        ]);
    }
    let mut command = storyboard();
    if let Some(claude) = claude {
        command.env("NB_STORYBOARD_CLAUDE_CLI", claude);
    }
    let mut child = command
        .args([
            "serve",
            workspace.path().to_str().expect("UTF-8"),
            "--port",
            "0",
            "--no-open",
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("start serve");
    let stderr = child.stderr.take().expect("stderr");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("serve announces its URL");
    let port = line
        .split("http://127.0.0.1:")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|port| port.parse().ok())
        .unwrap_or_else(|| panic!("no port in {line:?}"));
    Served {
        child,
        port,
        _workspace: workspace,
        project,
    }
}

#[cfg(unix)]
#[test]
fn claude_job_reports_completion_and_rejects_stale_briefs() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().unwrap();
    let script = fixture.path().join("claude");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s' '{\"result\":\"Storyboard reviewed\"}'\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let served = serve_with_claude(Some(&script));
    let base = version(&served.project);
    let body = json!({"project":"night","base_version":base,"brief":"Review shot 1","instruction":"Inspect the first shot"});
    let (status, started) = post_json(served.port, "/api/claude-job", &body);
    assert_eq!(status, 200, "{started}");
    assert_eq!(started["status"], "running");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (status, job) = get_json(served.port, "/api/claude-job?project=night");
        assert_eq!(status, 200);
        if job["status"] == "completed" {
            assert_eq!(job["output"], "Storyboard reviewed");
            assert_eq!(job["project_changed"], false);
            assert_eq!(job["shot_changes"], json!([]));
            assert_eq!(job["cut_changes"], json!([]));
            assert_eq!(job["new_exports"], json!([]));
            assert_eq!(job["version_before"], base);
            assert_eq!(job["version_after"], base);
            break;
        }
        assert!(Instant::now() < deadline, "job did not complete: {job}");
        thread::sleep(Duration::from_millis(50));
    }
    let mut stale_body = body;
    stale_body["base_version"] = json!("stale");
    let (status, stale) = post_json(served.port, "/api/claude-job", &stale_body);
    assert_eq!(status, 400, "{stale}");
}

#[cfg(unix)]
#[test]
fn claude_job_finishes_when_a_child_keeps_output_pipes_open() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().unwrap();
    let script = fixture.path().join("claude");
    std::fs::write(
        &script,
        "#!/bin/sh\nsleep 8 &\nprintf '%s' '{\"result\":\"Done\"}'\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let served = serve_with_claude(Some(&script));
    let base = version(&served.project);
    let (status, started) = post_json(
        served.port,
        "/api/claude-job",
        &json!({"project":"night","base_version":base,"brief":"Review","instruction":"Inspect"}),
    );
    assert_eq!(status, 200, "{started}");
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let (status, job) = get_json(served.port, "/api/claude-job?project=night");
        assert_eq!(status, 200);
        if job["status"] == "completed" {
            assert_eq!(job["output"], "Done");
            break;
        }
        assert!(Instant::now() < deadline, "job did not finish: {job}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn claude_job_reports_a_rendered_board_movie_without_project_edits() {
    use std::os::unix::fs::PermissionsExt;

    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let fixture = TempDir::new().unwrap();
    let script = fixture.path().join("claude");
    let served = serve_with_claude(Some(&script));
    let base = version(&served.project);
    let binary = storyboard()
        .get_program()
        .to_string_lossy()
        .replace('"', "\\\"");
    std::fs::write(
        &script,
        format!("#!/bin/sh\n\"{binary}\" export-movie . --mode boards >/dev/null\nprintf '%s' '{{\"result\":\"Rendered boards\"}}'\n"),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let body = json!({"project":"night","base_version":base,"brief":"Render the boards","instruction":"Export the cut"});
    let (status, started) = post_json(served.port, "/api/claude-job", &body);
    assert_eq!(status, 200, "{started}");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, job) = get_json(served.port, "/api/claude-job?project=night");
        assert_eq!(status, 200);
        if job["status"] == "completed" {
            assert_eq!(job["project_changed"], false);
            assert_eq!(job["version_after"], base);
            let files = job["new_exports"].as_array().expect("new exports");
            assert_eq!(files.len(), 1, "{job}");
            let file = files[0].as_str().unwrap();
            let (status, headers, bytes) = request_bytes(
                served.port,
                "GET",
                &format!("/api/export.mp4?project=night&file={file}"),
                &[("Range", "bytes=0-15")],
                &[],
            );
            assert_eq!(status, 206, "{headers}");
            assert_eq!(bytes.len(), 16);
            break;
        }
        assert!(Instant::now() < deadline, "job did not complete: {job}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn claude_job_surfaces_a_child_process_project_edit() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().unwrap();
    let script = fixture.path().join("claude");
    let config_path = fixture.path().join("mcp-config.json");
    let served = serve_with_claude(Some(&script));
    let base = version(&served.project);
    let binary = storyboard()
        .get_program()
        .to_string_lossy()
        .replace('"', "\\\"");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ntest \"$NB_STORYBOARD_MCP_SINGLE_PROJECT\" = 1 || exit 10\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--mcp-config' ]; then shift; printf '%s' \"$1\" > \"{}\"; break; fi\n  shift\ndone\n\"{binary}\" set-shot . --base-version {base} --shot shot-1 --action 'Revise this shot' --author agent:mock >/dev/null\nprintf '%s' '{{\"result\":\"Updated shot 1\"}}'\n",
            config_path.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let body = json!({"project":"night","base_version":base,"brief":"Revise shot 1","instruction":"Update shot 1"});
    let (status, started) = post_json(served.port, "/api/claude-job", &body);
    assert_eq!(status, 200, "{started}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (status, job) = get_json(served.port, "/api/claude-job?project=night");
        assert_eq!(status, 200);
        if job["status"] == "completed" {
            assert_eq!(job["output"], "Updated shot 1");
            assert_eq!(job["project_changed"], true);
            assert_eq!(
                job["shot_changes"],
                json!([{"id":"shot-1","kind":"edited"}])
            );
            assert_eq!(job["cut_changes"], json!([]));
            assert_ne!(job["version_after"], base);
            assert_eq!(job["movie_changes"], json!([]));
            break;
        }
        assert!(Instant::now() < deadline, "job did not complete: {job}");
        thread::sleep(Duration::from_millis(50));
    }
    let (status, reopened) = get_json(served.port, "/api/project?project=night");
    assert_eq!(status, 200, "{reopened}");
    assert_ne!(reopened["version"], base);
    assert_eq!(
        reopened["document"]["scenes"][0]["shots"][0]["action"],
        "Revise this shot"
    );
    let history = cli(&["history", served.project.to_str().expect("UTF-8")]);
    assert_eq!(history["history"][0]["author"]["agent"]["name"], "mock");
    let config: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(
        PathBuf::from(
            config["mcpServers"]["storyboard"]["args"][1]
                .as_str()
                .unwrap()
        )
        .canonicalize()
        .unwrap(),
        served.project.canonicalize().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn claude_job_reports_a_saved_replacement_movie() {
    use std::os::unix::fs::PermissionsExt;

    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let fixture = TempDir::new().unwrap();
    let movie = fixture.path().join("replacement.mp4");
    let encoded = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=160x90:r=24",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&movie)
        .output()
        .unwrap();
    assert!(
        encoded.status.success(),
        "{}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    let script = fixture.path().join("claude");
    let served = serve_with_claude(Some(&script));
    let base = version(&served.project);
    let binary = storyboard()
        .get_program()
        .to_string_lossy()
        .replace('"', "\\\"");
    std::fs::write(&script, format!(
        "#!/bin/sh\n\"{binary}\" replace-movie . --base-version {base} --shot shot-1 --source \"{}\" --author agent:mock >/dev/null\nprintf '%s' '{{\"result\":\"Imported replacement\"}}'\n",
        movie.display()
    )).unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let (status, started) = post_json(
        served.port,
        "/api/claude-job",
        &json!({
            "project":"night", "base_version":base, "brief":"Replace shot 1", "instruction":"Import replacement MP4"
        }),
    );
    assert_eq!(status, 200, "{started}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, job) = get_json(served.port, "/api/claude-job?project=night");
        assert_eq!(status, 200);
        if job["status"] == "completed" {
            assert_eq!(job["project_changed"], true);
            assert_eq!(
                job["shot_changes"],
                json!([{"id":"shot-1","kind":"edited"}])
            );
            assert_eq!(job["movie_changes"][0]["shot"], "shot-1");
            let source = job["movie_changes"][0]["source_movie"]["src"]
                .as_str()
                .unwrap();
            assert!(source.starts_with("sources/imported-movies/"), "{source}");
            assert!(served.project.join(source).is_file());
            break;
        }
        assert!(Instant::now() < deadline, "job did not complete: {job}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn claude_job_can_be_canceled_and_blocks_parallel_runs() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().unwrap();
    let script = fixture.path().join("claude");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 10\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let served = serve_with_claude(Some(&script));
    let body = json!({"project":"night","base_version":version(&served.project),"brief":"Review shot 1","instruction":"Inspect the first shot"});
    let (status, started) = post_json(served.port, "/api/claude-job", &body);
    assert_eq!(status, 200, "{started}");
    let (status, _) = post_json(served.port, "/api/claude-job", &body);
    assert_eq!(status, 400, "a parallel run must be rejected");
    let (status, _) = post_json(
        served.port,
        "/api/claude-cancel",
        &json!({"project":"night","id":started["id"]}),
    );
    assert_eq!(status, 200);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (_, job) = get_json(served.port, "/api/claude-job?project=night");
        if job["status"] == "canceled" {
            break;
        }
        assert!(Instant::now() < deadline, "job did not cancel: {job}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn canceling_claude_stops_its_inherited_mcp_child() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().unwrap();
    let script = fixture.path().join("claude");
    let child_pid_file = fixture.path().join("mcp-child.pid");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
            child_pid_file.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let served = serve_with_claude(Some(&script));
    let body = json!({"project":"night","base_version":version(&served.project),"brief":"Review shot 1","instruction":"Inspect the first shot"});
    let (status, started) = post_json(served.port, "/api/claude-job", &body);
    assert_eq!(status, 200, "{started}");
    let deadline = Instant::now() + Duration::from_secs(3);
    let child_pid = loop {
        if let Ok(pid) = std::fs::read_to_string(&child_pid_file) {
            break pid;
        }
        assert!(Instant::now() < deadline, "mock MCP child did not start");
        thread::sleep(Duration::from_millis(20));
    };
    let (status, _) = post_json(
        served.port,
        "/api/claude-cancel",
        &json!({"project":"night","id":started["id"]}),
    );
    assert_eq!(status, 200);
    let deadline = Instant::now() + Duration::from_secs(3);
    let canceled = loop {
        let (_, job) = get_json(served.port, "/api/claude-job?project=night");
        if job["status"] == "canceled" {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(20));
    };
    if !canceled {
        let _ = Command::new("kill").arg(child_pid.trim()).output();
    }
    assert!(
        canceled,
        "cancel must stop the inherited MCP child promptly"
    );
}

/// One HTTP/1.1 request with `Connection: close`; returns status, headers and body.
fn request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, String, Vec<u8>) {
    let payload = body.map(Value::to_string).unwrap_or_default();
    request_bytes(port, method, path, headers, payload.as_bytes())
}

fn request_bytes(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    payload: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .expect("timeout");
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        payload.len()
    );
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("Host"))
    {
        head.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).expect("write head");
    stream.write_all(payload).expect("write body");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header end");
    let header_text = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = header_text
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status");
    let mut body = raw[split + 4..].to_vec();
    if header_text
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    (status, header_text, body)
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("chunk size");
        let size = usize::from_str_radix(String::from_utf8_lossy(&body[..line_end]).trim(), 16)
            .expect("hex size");
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&body[line_end + 2..line_end + 2 + size]);
        body = &body[line_end + 2 + size + 2..];
    }
}

fn get_json(port: u16, path: &str) -> (u16, Value) {
    let (status, _, body) = request(port, "GET", path, &[], None);
    (status, serde_json::from_slice(&body).expect("JSON body"))
}

fn post_json(port: u16, path: &str, body: &Value) -> (u16, Value) {
    let (status, _, body) = request(port, "POST", path, &[], Some(body));
    (status, serde_json::from_slice(&body).expect("JSON body"))
}

#[test]
fn uploads_images_and_imports_one_undoable_shot() {
    let served = serve();
    let (_, project) = get_json(served.port, "/api/project?project=night");
    let base = project["version"].as_str().unwrap();
    let mut files = Vec::new();
    for name in ["frame-10.png", "frame-2.png", "frame-1.png"] {
        let path = format!("/api/upload-source?project=night&folder=abc123&name={name}");
        let (status, _, body) = request_bytes(served.port, "POST", &path, &[], b"image bytes");
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let uploaded: Value = serde_json::from_slice(&body).unwrap();
        files.push(json!({"name": name, "path": uploaded["path"]}));
    }
    let (status, invalid) = post_json(
        served.port,
        "/api/import-shot",
        &json!({
            "project": "night", "base_version": base, "scene": "scene-1",
            "title": "Too short", "duration": 0.01, "files": files,
        }),
    );
    assert_eq!(status, 400, "{invalid}");
    let (status, invalid) = post_json(
        served.port,
        "/api/import-shot",
        &json!({
            "project": "night", "base_version": base, "scene": "scene-1",
            "title": "Bad path", "files": [{"name": "frame-1.png", "path": "../frame-1.png"}],
        }),
    );
    assert_eq!(status, 400, "{invalid}");
    let (status, result) = post_json(
        served.port,
        "/api/import-shot",
        &json!({
            "project": "night", "base_version": base, "scene": "scene-1",
            "title": "Sequence", "files": files,
        }),
    );
    assert_eq!(status, 200, "{result}");
    let scene = &result["document"]["scenes"][0];
    let shot = scene["shots"].as_array().unwrap().last().unwrap();
    assert_eq!(shot["title"], "Sequence");
    assert_eq!(shot["duration"], 3.0);
    let panels = shot["takes"][0]["panels"].as_array().unwrap();
    assert_eq!(panels.len(), 3);
    assert_eq!(
        panels
            .iter()
            .map(|p| p["time"].as_f64().unwrap())
            .collect::<Vec<_>>(),
        [0.0, 1.0, 2.0]
    );
    let names: Vec<_> = panels
        .iter()
        .map(|panel| {
            panel["layers"][0]["src"]
                .as_str()
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap()
        })
        .collect();
    assert_eq!(names, ["frame-1.png", "frame-2.png", "frame-10.png"]);
    let (status, undone) = post_json(
        served.port,
        "/api/undo",
        &json!({
            "project": "night", "base_version": result["version"]
        }),
    );
    assert_eq!(status, 200, "{undone}");
    assert_eq!(
        undone["document"]["scenes"][0]["shots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        served
            .project
            .join("sources/imported-shots/abc123/frame-1.png")
            .exists()
    );
}

#[test]
fn sources_route_reports_usage_missing_refs_and_upload_refuses_overwrite() {
    let served = serve();
    fs::write(served.project.join("sources/existing.png"), b"existing").expect("source fixture");
    let (status, attached) = post_json(
        served.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&served.project),"ops":[
            {"add_ref":{"target":{"shot":{"shot":"shot-1"}},"reference":"sources/existing.png"}},
            {"add_ref":{"target":{"shot":{"shot":"shot-1"}},"reference":"sources/missing.png"}},
            {"add_layer":{"shot":"shot-1","take":"take-1","panel":"panel-1","layer_type":"image","fields":{"src":"sources/existing.png"}}},
            {"add_layer":{"shot":"shot-1","take":"take-1","panel":"panel-1","layer_type":"image","fields":{"src":"sources/gone-layer.png"}}}
        ]}),
    );
    assert_eq!(status, 200, "{attached}");

    let (status, sources) = get_json(served.port, "/api/sources?project=night");
    assert_eq!(status, 200, "{sources}");
    let existing = sources["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .find(|source| source["path"] == "sources/existing.png")
        .expect("existing source");
    assert_eq!(existing["kind"], "image");
    assert_eq!(existing["size"], 8);
    assert!(existing["modified"].is_string());
    assert!(existing["dimensions"].is_null());
    assert!(
        existing["used_by"]
            .as_array()
            .expect("uses")
            .iter()
            .any(|source_use| source_use["target"] == "shot:shot-1")
    );
    assert!(
        existing["used_by"]
            .as_array()
            .expect("uses")
            .iter()
            .any(|source_use| source_use["target"] == "layer:shot-1/take-1/panel-1/layer-1")
    );
    assert!(existing.get("uses").is_none());
    assert_eq!(sources["missing_refs"][0]["path"], "sources/missing.png");
    assert_eq!(sources["missing_refs"][0]["target"], "shot:shot-1");
    assert!(
        sources["missing_refs"]
            .as_array()
            .expect("missing refs")
            .iter()
            .any(|missing| missing["path"] == "sources/gone-layer.png"
                && missing["target"] == "layer:shot-1/take-1/panel-1/layer-2")
    );

    let upload = "/api/upload-source?project=night&folder=abc123&name=outside.png";
    let (status, _, first) = request_bytes(served.port, "POST", upload, &[], b"first");
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&first));
    let (status, _, _) = request_bytes(served.port, "POST", upload, &[], b"replacement");
    assert_ne!(status, 200);
    assert_eq!(
        fs::read(
            served
                .project
                .join("sources/imported-shots/abc123/outside.png")
        )
        .expect("uploaded source"),
        b"first"
    );
}

#[test]
fn serve_page_and_assets_are_embedded() {
    let served = serve();
    for (path, kind) in [
        ("/", "text/html"),
        ("/js/main.js", "text/javascript"),
        ("/js/panels/import-shot.js", "text/javascript"),
        ("/js/core/registry.js", "text/javascript"),
        ("/css/tokens.css", "text/css"),
    ] {
        let (status, headers, body) = request(served.port, "GET", path, &[], None);
        assert_eq!(status, 200, "{path}");
        assert!(
            headers.to_ascii_lowercase().contains(kind),
            "{path}: {headers}"
        );
        assert!(!body.is_empty(), "{path} is empty");
    }
}

#[test]
fn serve_refuses_foreign_hosts_and_origins() {
    let served = serve();
    let (status, _, _) = request(
        served.port,
        "GET",
        "/api/workspace",
        &[("Host", "evil.example")],
        None,
    );
    assert_eq!(status, 403);
    let body = json!({"project": "night", "base_version": version(&served.project), "ops": []});
    let (status, _, _) = request(
        served.port,
        "POST",
        "/api/commit",
        &[("Origin", "http://evil.example")],
        Some(&body),
    );
    assert_eq!(status, 403);
}

#[test]
fn serve_lists_the_workspace_and_reads_a_project() {
    let served = serve();
    let (status, index) = get_json(served.port, "/api/workspace");
    assert_eq!(status, 200);
    assert_eq!(
        index["root_path"],
        served
            ._workspace
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(index["cli_path"].as_str().unwrap().ends_with("storyboard"));
    assert_eq!(index["projects"][0]["project"], "night");
    assert_eq!(index["projects"][0]["shot_count"], 2);
    assert_eq!(index["projects"][0]["fps"], 24);
    let (status, project) = get_json(served.port, "/api/project?project=night");
    assert_eq!(status, 200);
    assert_eq!(project["version"], version(&served.project));
    let order: Vec<_> = project["sequence"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["shot"].clone())
        .collect();
    assert_eq!(order, [json!("shot-1"), json!("shot-2")]);
    let (status, error) = get_json(served.port, "/api/project?project=..%2F..");
    assert_eq!(status, 400, "{error}");
}

#[test]
fn viewport_route_round_trips_cache_only_and_rejects_invalid_writes() {
    let served = serve();
    cli(&[
        "add-cut",
        served.project.to_str().expect("UTF-8"),
        "--base-version",
        &version(&served.project),
        "--name",
        "Assembly",
        "--id",
        "cut-assembly",
    ]);
    let project_before = fs::read(served.project.join("project.json")).expect("project before");
    let history_before =
        fs::read(served.project.join("history/log.jsonl")).expect("history before");

    let (status, missing) = get_json(served.port, "/api/viewport?project=night");
    assert_eq!(status, 200, "{missing}");
    assert!(missing["viewport"].is_null());

    let viewport = json!({
        "pan_x": -125.5,
        "pan_y": 40.25,
        "zoom": 1.75,
        "viewed_cut": "cut-assembly"
    });
    let (status, saved) = post_json(
        served.port,
        "/api/viewport",
        &json!({"project":"night", "viewport": viewport}),
    );
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["viewport"], viewport);
    assert_eq!(
        fs::read(served.project.join("project.json")).expect("project after"),
        project_before
    );
    assert_eq!(
        fs::read(served.project.join("history/log.jsonl")).expect("history after"),
        history_before
    );
    assert!(served.project.join(".cache/viewport.json").is_file());
    assert!(!served.project.join(".cache/viewport.json.tmp").exists());
    let (status, loaded) = get_json(served.port, "/api/viewport?project=night");
    assert_eq!(status, 200, "{loaded}");
    assert_eq!(loaded["viewport"], viewport);

    for invalid in [
        json!({"project":"night", "viewport":{"pan_x":0,"pan_y":0,"zoom":0,"viewed_cut":null}}),
        json!({"project":"night", "viewport":{"pan_x":0,"pan_y":0,"zoom":1,"viewed_cut":"not-a-cut"}}),
        json!({"project":"night", "viewport":{"pan_x":0,"pan_y":0,"zoom":1,"viewed_cut":null,"surprise":true}}),
        json!({"project":"night", "viewport":{"pan_x":0,"pan_y":0,"zoom":1,"viewed_cut":null},"surprise":true}),
        json!({"project":"night", "viewport":{"pan_x":0,"pan_y":0,"zoom":1,"viewed_cut":"x".repeat(5000)}}),
    ] {
        let (status, rejected) = post_json(served.port, "/api/viewport", &invalid);
        assert_eq!(status, 400, "{rejected}");
    }
    let (status, unchanged) = get_json(served.port, "/api/viewport?project=night");
    assert_eq!(status, 200);
    assert_eq!(unchanged["viewport"], viewport);

    fs::write(served.project.join(".cache/viewport.json"), b"not JSON").expect("corrupt viewport");
    let (status, corrupt) = get_json(served.port, "/api/viewport?project=night");
    assert_eq!(status, 200, "{corrupt}");
    assert!(corrupt["viewport"].is_null());
}

#[test]
fn layout_route_round_trips_workspace_cache_and_normalizes_unknown_panels() {
    let served = serve();
    let layout_before = fs::read(served.project.join("project.json")).expect("project before");
    let history_before =
        fs::read(served.project.join("history/log.jsonl")).expect("history before");

    let (status, missing) = get_json(served.port, "/api/layout");
    assert_eq!(status, 200, "{missing}");
    assert!(missing["layout"].is_null());

    let layout = json!({
        "left": ["sources", "not-installed", "sources"],
        "right": ["inspector"],
        "bottom": ["player"],
        "hidden": ["history"],
        "sizes": {"left": 280, "right": 420, "bottom": 260}
    });
    let (status, saved) = post_json(served.port, "/api/layout", &json!({"layout": layout}));
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["layout"]["left"], json!(["sources"]));
    assert_eq!(saved["layout"]["right"], json!(["inspector"]));
    assert_eq!(saved["layout"]["bottom"], json!(["player"]));
    assert_eq!(saved["layout"]["center"], json!([]));
    assert_eq!(saved["layout"]["hidden"], json!(["history"]));
    assert_eq!(
        fs::read(served.project.join("project.json")).expect("project after"),
        layout_before
    );
    assert_eq!(
        fs::read(served.project.join("history/log.jsonl")).expect("history after"),
        history_before
    );
    let workspace_root = served.project.parent().expect("workspace root");
    assert!(
        workspace_root
            .join(".cache/workbench-layout.json")
            .is_file()
    );
    assert!(
        !workspace_root
            .join(".cache/workbench-layout.json.tmp")
            .exists()
    );

    let (status, loaded) = get_json(served.port, "/api/layout");
    assert_eq!(status, 200, "{loaded}");
    assert_eq!(loaded, saved);

    for invalid in [
        json!({"layout":{"left":[],"right":[],"bottom":[],"hidden":[],"sizes":{"left":0}}}),
        json!({"layout":{"left":[],"right":[],"bottom":[],"hidden":[],"sizes":{},"surprise":true}}),
        json!({"layout":{"left":["sources", 7],"right":[],"bottom":[],"hidden":[],"sizes":{}}}),
        json!({"layout":{"left":[],"right":[],"bottom":[],"hidden":[],"sizes":{}},"surprise":true}),
        json!({"layout":{"left":["x".repeat(5000)],"right":[],"bottom":[],"hidden":[],"sizes":{}}}),
    ] {
        let (status, rejected) = post_json(served.port, "/api/layout", &invalid);
        assert_eq!(status, 400, "{rejected}");
    }

    fs::write(
        workspace_root.join(".cache/workbench-layout.json"),
        b"not JSON",
    )
    .expect("corrupt layout");
    let (status, corrupt) = get_json(served.port, "/api/layout");
    assert_eq!(status, 200, "{corrupt}");
    assert!(corrupt["layout"].is_null());
}

#[test]
fn history_route_exposes_commit_footprints_for_exact_reveal() {
    let served = serve();
    let (status, committed) = post_json(
        served.port,
        "/api/commit",
        &json!({
            "project": "night",
            "base_version": version(&served.project),
            "ops": [{"update_shot": {"shot": "shot-1", "patch": {"notes": "Exact reveal"}}}]
        }),
    );
    assert_eq!(status, 200, "{committed}");
    let (status, history) = get_json(served.port, "/api/history?project=night");
    assert_eq!(status, 200, "{history}");
    assert!(
        history["history"][0]["footprints"]
            .as_array()
            .expect("history exposes footprints")
            .iter()
            .any(|value| value == "shot/shot-1/field/notes"),
        "{history}"
    );
}

#[test]
fn serve_preserves_repeated_scene_returns_in_cut_order() {
    let served = serve();
    let dir = served.project.to_str().expect("UTF-8");
    cli(&[
        "add-scene",
        dir,
        "--base-version",
        &version(&served.project),
        "--title",
        "Elsewhere",
    ]);
    cli(&[
        "add-shot",
        dir,
        "--base-version",
        &version(&served.project),
        "--scene",
        "scene-2",
        "--title",
        "Parallel action",
    ]);
    let (status, added) = post_json(
        served.port,
        "/api/commit",
        &json!({"project":"night","base_version":version(&served.project),
            "ops":[{"add_cut":{"name":"Intercut"}}]}),
    );
    assert_eq!(status, 200, "{added}");
    let cut = added["ids"][0].as_str().expect("new cut");
    let (status, intercut) = post_json(
        served.port,
        "/api/commit",
        &json!({"project":"night","base_version":added["version"],
        "ops":[{"batch":{"ops":[
            {"add_clip":{"cut":cut,"source":{"shot":{"shot":"shot-1"}}}},
            {"add_clip":{"cut":cut,"source":{"shot":{"shot":"shot-3"}}}},
            {"add_clip":{"cut":cut,"source":{"shot":{"shot":"shot-2"}}}}
        ]}}]}),
    );
    assert_eq!(status, 200, "{intercut}");
    let (status, player) = get_json(
        served.port,
        &format!("/api/project?project=night&cut={cut}"),
    );
    assert_eq!(status, 200, "{player}");
    let items = player["sequence"]["items"].as_array().expect("sequence");
    let order: Vec<_> = items
        .iter()
        .map(|item| item["shot"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["shot-1", "shot-3", "shot-2"]);
    assert_eq!(player["document"]["scenes"][1]["shots"][0]["id"], "shot-3");
    assert_eq!(items[0]["end"], items[1]["start"]);
    assert_eq!(items[1]["end"], items[2]["start"]);
    assert_eq!(items[2]["end"], player["sequence"]["frames"]);
}

#[test]
fn serve_commits_as_human_through_validated_ops() {
    let served = serve();
    let base = version(&served.project);
    let reorder = json!({"project": "night", "base_version": base, "ops": [
        {"batch": {"ops": [{"move_shot": {"shot": "shot-2", "index": 0}}, {"move_node": {"node": "shot:shot-2", "position": {"x": 10, "y": 20}}}]}}
    ]});
    let (status, result) = post_json(served.port, "/api/commit", &reorder);
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["document"]["scenes"][0]["shots"][0]["id"], "shot-2");
    assert_eq!(
        result["document"]["layout"]["positions"]["shot:shot-2"],
        json!({"x": 10.0, "y": 20.0})
    );
    let history = cli(&["history", served.project.to_str().expect("UTF-8")]);
    assert_eq!(history["history"][0]["author"], "human", "{history}");

    let invalid = json!({"project": "night", "base_version": result["version"], "ops": [{"update_shot": {"shot": "shot-1", "patch": {"bogus": 1}}}]});
    let (status, error) = post_json(served.port, "/api/commit", &invalid);
    assert_eq!(status, 400);
    assert!(
        error["error"]["valid"]
            .as_array()
            .expect("valid")
            .contains(&json!("title")),
        "{error}"
    );

    let spoof = json!({"project": "night", "base_version": result["version"], "ops": [{"add_sticky": {"text": "hi", "kind": "note", "author": {"agent": {"name": "x"}}, "pin": "none", "created": "2026-11-08T00:00:00Z"}}]});
    let (status, error) = post_json(served.port, "/api/commit", &spoof);
    assert_eq!(status, 400);
    assert_eq!(error["error"]["kind"], "authorship");

    let (status, undone) = post_json(
        served.port,
        "/api/undo",
        &json!({"project": "night", "base_version": result["version"]}),
    );
    assert_eq!(status, 200, "{undone}");
    assert_eq!(undone["document"]["scenes"][0]["shots"][0]["id"], "shot-1");
}

#[test]
fn serve_reports_an_overlapping_stale_write_as_a_conflict() {
    let served = serve();
    let stale = version(&served.project);
    let dir = served.project.to_str().expect("UTF-8");
    cli(&[
        "set-shot",
        dir,
        "--base-version",
        &stale,
        "--shot",
        "shot-1",
        "--action",
        "agent first",
    ]);
    let mine = json!({"project": "night", "base_version": stale, "ops": [{"update_shot": {"shot": "shot-1", "patch": {"action": "person second"}}}]});
    let (status, error) = post_json(served.port, "/api/commit", &mine);
    assert_eq!(status, 409, "{error}");
    assert_eq!(error["error"]["kind"], "conflict");
    assert_eq!(
        error["error"]["issues"][0]["current_version"],
        version(&served.project)
    );
}

#[test]
fn serve_renders_panels_and_reuses_the_render() {
    let served = serve();
    let path = "/api/panel.png?project=night&shot=shot-1&width=240";
    let (status, headers, png) = request(served.port, "GET", path, &[], None);
    assert_eq!(status, 200, "{headers}");
    assert_eq!(&png[1..4], b"PNG");
    assert_eq!(
        u32::from_be_bytes(png[16..20].try_into().expect("width")),
        240
    );
    let (_, _, again) = request(served.port, "GET", path, &[], None);
    assert_eq!(png, again);
}

#[test]
fn serve_board_render_changes_when_earlier_shot_duration_changes() {
    let served = serve();
    let path = "/api/panel.png?project=night&shot=shot-2&width=480&mode=board";
    let (status, _, before) = request(served.port, "GET", path, &[], None);
    assert_eq!(status, 200);

    let (status, result) = post_json(
        served.port,
        "/api/commit",
        &json!({"project": "night", "base_version": version(&served.project), "ops": [
            {"update_shot": {"shot": "shot-1", "patch": {"duration": 8.0}}}
        ]}),
    );
    assert_eq!(status, 200, "{result}");
    let (status, _, after) = request(served.port, "GET", path, &[], None);
    assert_eq!(status, 200);
    assert!(
        before != after,
        "board caption must show the new start frame"
    );
}

#[test]
fn serve_panel_urls_are_not_browser_immutable() {
    let served = serve();
    let (status, headers, _) = request(
        served.port,
        "GET",
        "/api/panel.png?project=night&shot=shot-1&width=240",
        &[],
        None,
    );
    assert_eq!(status, 200);
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("cache-control: no-store"),
        "the same URL can render new content after a commit: {headers}"
    );
}

#[test]
fn serve_pushes_an_agent_cli_write_to_the_page_within_two_seconds() {
    let served = serve();
    let mut stream = TcpStream::connect(("127.0.0.1", served.port)).expect("connect");
    write!(
        stream,
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        served.port
    )
    .expect("subscribe");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while !line.starts_with(": connected") {
        line.clear();
        reader.read_line(&mut line).expect("stream opens");
    }
    // The watcher is armed when the stream opens; give it a moment to register its watches.
    thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    let dir = served.project.to_str().expect("UTF-8");
    cli(&[
        "set-shot",
        dir,
        "--base-version",
        &version(&served.project),
        "--shot",
        "shot-2",
        "--action",
        "from the agent",
    ]);
    let expected = version(&served.project);
    loop {
        line.clear();
        reader
            .read_line(&mut line)
            .expect("event arrives before the read timeout");
        if let Some(data) = line.strip_prefix("data: ") {
            let event: Value = serde_json::from_str(data.trim()).expect("event JSON");
            if event["version"] == expected.as_str() {
                assert_eq!(event["project"], "night");
                assert_eq!(event["author"], "agent:test");
                break;
            }
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn live_updates_limit_open_streams_and_keep_http_responsive() {
    let served = serve();
    let mut streams = Vec::new();
    for _ in 0..32 {
        let mut stream = TcpStream::connect(("127.0.0.1", served.port)).expect("connect");
        write!(
            stream,
            "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            served.port
        )
        .expect("subscribe");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while !line.starts_with(": connected") {
            line.clear();
            reader.read_line(&mut line).expect("stream opens");
        }
        streams.push(reader);
    }

    let mut extra = TcpStream::connect(("127.0.0.1", served.port)).expect("connect extra");
    write!(
        extra,
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        served.port
    )
    .expect("subscribe extra");
    extra
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout extra");
    let mut status = String::new();
    BufReader::new(extra)
        .read_line(&mut status)
        .expect("response status");
    assert!(status.starts_with("HTTP/1.1 503"), "{status:?}");

    let (status, workspace) = get_json(served.port, "/api/workspace");
    assert_eq!(status, 200, "{workspace}");
    assert_eq!(workspace["projects"].as_array().unwrap().len(), 1);

    streams
        .pop()
        .unwrap()
        .into_inner()
        .shutdown(Shutdown::Both)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let mut replacement = TcpStream::connect(("127.0.0.1", served.port)).unwrap();
        write!(
            replacement,
            "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            served.port
        )
        .unwrap();
        replacement
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut status = String::new();
        BufReader::new(replacement).read_line(&mut status).unwrap();
        if status.starts_with("HTTP/1.1 200") {
            break;
        }
        assert!(Instant::now() < deadline, "stream slot did not reopen");
        thread::sleep(Duration::from_millis(200));
    }
}
