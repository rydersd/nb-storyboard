use std::{fs, path::Path, process::Command};

use assert_cmd::prelude::*;
use clap::{Command as ClapCommand, CommandFactory};
use image::{ImageBuffer, Rgba};
use serde_json::Value;
use storyboard_cli::args::Cli;
use tempfile::TempDir;

#[path = "support/fixture.rs"]
mod fixture_support;

fn storyboard() -> Command {
    Command::cargo_bin("storyboard").expect("storyboard binary")
}

fn json(command: &mut Command) -> Value {
    let output = command.output().expect("run storyboard");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn error(command: &mut Command) -> Value {
    let output = command.output().expect("run storyboard");
    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stderr).expect("JSON error")
}

fn new_project() -> (TempDir, std::path::PathBuf, String) {
    let workspace = TempDir::new().expect("temporary workspace");
    let dir = workspace.path().join("story");
    let created = json(storyboard().args([
        "new",
        dir.to_str().expect("UTF-8 path"),
        "--title",
        "Four shots",
    ]));
    let version = created["version"].as_str().expect("version").to_owned();
    (workspace, dir, version)
}

#[test]
fn cli_workspace_index_includes_fps() {
    let (workspace, _dir, _version) = new_project();
    let indexed = json(storyboard().args(["index", workspace.path().to_str().expect("UTF-8")]));
    assert_eq!(indexed["projects"][0]["fps"], 24);
}

fn write(dir: &Path, version: &str, args: &[&str]) -> Value {
    let mut command = storyboard();
    command
        .args(args)
        .arg(dir)
        .args(["--base-version", version, "--author", "human"]);
    json(&mut command)
}

fn write_version(dir: &Path, version: &str, args: &[&str]) -> String {
    write(dir, version, args)["version"]
        .as_str()
        .expect("updated version")
        .to_owned()
}

fn add_opening_shot(dir: &Path, version: &str) -> String {
    let version = write_version(dir, version, &["add-scene", "--title", "Opening"]);
    write_version(
        dir,
        &version,
        &["add-shot", "--scene", "scene-1", "--title", "Arrival"],
    )
}

fn write_png(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("image parent");
    }
    ImageBuffer::<Rgba<u8>, _>::from_pixel(2, 2, Rgba([255, 0, 0, 255]))
        .save(path)
        .expect("source PNG");
}

fn assert_png(path: &Path) {
    let bytes = fs::read(path).expect("PNG output");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn imports_numbered_images_as_one_ordered_undoable_shot() {
    let (workspace, dir, base) = new_project();
    let base = write_version(&dir, &base, &["add-scene", "--title", "Opening"]);
    let files: Vec<_> = ["frame-10.png", "frame-2.png", "frame-1.png"]
        .into_iter()
        .map(|name| workspace.path().join(name))
        .collect();
    for file in &files {
        write_png(file);
    }
    let mut command = storyboard();
    command.args([
        "import-shot",
        dir.to_str().unwrap(),
        "--base-version",
        &base,
        "--author",
        "human",
        "--scene",
        "scene-1",
        "--title",
        "Sequence",
    ]);
    for file in &files {
        command.arg(file);
    }
    let result = json(&mut command);
    let document: Value =
        serde_json::from_slice(&fs::read(dir.join("project.json")).unwrap()).unwrap();
    let shot = &document["scenes"][0]["shots"][0];
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
    let undone = write(&dir, result["version"].as_str().unwrap(), &["undo"]);
    assert!(undone["version"].is_string());
    let document: Value =
        serde_json::from_slice(&fs::read(dir.join("project.json")).unwrap()).unwrap();
    assert!(
        document["scenes"][0]["shots"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
}

fn assert_refuses_without_overwrite(command: &mut Command, project: &Path) {
    let before = fs::read(project).expect("read original project");
    let output = command.output().expect("run storyboard");
    assert!(
        !output.status.success(),
        "render unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fs::read(project).expect("read project after rejection"),
        before
    );
}

fn runnable_shell_blocks(readme: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = None;

    for line in readme.lines() {
        if line == "```sh runnable" {
            assert!(current.is_none(), "runnable shell blocks cannot nest");
            current = Some(String::new());
        } else if line == "```" {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
        } else if let Some(block) = &mut current {
            block.push_str(line);
            block.push('\n');
        }
    }

    assert!(
        current.is_none(),
        "README has an unclosed runnable shell block"
    );
    blocks
}

#[test]
fn cli_readme_commands_are_real() {
    let readme = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"))
        .expect("read README");
    let blocks = runnable_shell_blocks(&readme);
    assert!(
        !blocks.is_empty(),
        "README must mark its executable walkthrough shell blocks as `sh runnable`"
    );

    let workspace = TempDir::new().expect("temporary README workspace");
    let output = Command::new("sh")
        .args(["-eu", "-c", &blocks.join("\n")])
        .current_dir(workspace.path())
        .env("STORYBOARD", assert_cmd::cargo::cargo_bin("storyboard"))
        .env("WORKSPACE", workspace.path())
        .output()
        .expect("run README walkthrough");
    assert!(
        output.status.success(),
        "README walkthrough failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        workspace
            .path()
            .join("night-station/project.json")
            .is_file()
    );
    assert!(
        workspace
            .path()
            .join("night-station/.cache/panel.png")
            .is_file()
    );
    assert!(
        workspace
            .path()
            .join("night-station/.cache/cut-sheet.png")
            .is_file()
    );
}

#[test]
fn cli_readme_mcp_setup_invokes_storyboard_mcp() {
    let readme = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"))
        .expect("read README");
    let claude = readme
        .lines()
        .find(|line| line.starts_with("claude mcp add storyboard -- "))
        .expect("README Claude MCP command");
    let claude_command = claude
        .split_once(" -- ")
        .expect("Claude MCP command separator")
        .1;
    assert_eq!(claude_command, "\"$STORYBOARD\" mcp \"$WORKSPACE\"");

    let codex_command = readme
        .lines()
        .find_map(|line| {
            line.strip_prefix("command = \"")
                .and_then(|line| line.strip_suffix('\"'))
        })
        .expect("README Codex command");
    assert!(
        codex_command.starts_with('/'),
        "Codex command must name an absolute downloaded-binary path"
    );
    let codex_args = readme
        .lines()
        .find_map(|line| line.strip_prefix("args = "))
        .expect("README Codex args");
    assert_eq!(
        codex_args,
        "[\"mcp\", \"/absolute/path/to/storyboard-workspace\"]"
    );

    let help = storyboard()
        .args(["mcp", "--help"])
        .output()
        .expect("MCP help");
    assert!(help.status.success(), "storyboard mcp must exist");
    let typo = storyboard()
        .args(["mpc", "--help"])
        .output()
        .expect("typo help");
    assert!(!typo.status.success(), "storyboard mpc must fail");
}

#[test]
fn cli_walkthrough_builds_four_shots_without_json() {
    let (workspace, dir, mut version) = new_project();
    let added_scene = write(&dir, &version, &["add-scene", "--title", "Opening"]);
    assert_eq!(added_scene["ids"], serde_json::json!(["scene-1"]));
    version = added_scene["version"].as_str().expect("version").into();
    for title in ["Arrival", "Question", "Decision", "Exit"] {
        version = write_version(
            &dir,
            &version,
            &["add-shot", "--scene", "scene-1", "--title", title],
        );
    }
    version = write_version(
        &dir,
        &version,
        &[
            "add-panel",
            "--shot",
            "shot-1",
            "--take",
            "take-1",
            "--time",
            "1",
        ],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "add-text",
            "--shot",
            "shot-1",
            "--take",
            "take-1",
            "--panel",
            "panel-2",
            "--text",
            "A red door",
        ],
    );
    let source = workspace.path().join("red.png");
    write_png(&source);
    json(storyboard().args([
        "import",
        dir.to_str().expect("UTF-8 path"),
        source.to_str().expect("UTF-8 path"),
        "--destination",
        "art/red.png",
    ]));
    version = write_version(
        &dir,
        &version,
        &[
            "add-image",
            "--shot",
            "shot-1",
            "--take",
            "take-1",
            "--panel",
            "panel-2",
            "--src",
            "art/red.png",
        ],
    );
    let panel_out = workspace.path().join("panel.png");
    let sheet_out = workspace.path().join("sheet.png");
    json(storyboard().args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--mode",
        "board",
        "--out",
        panel_out.to_str().expect("UTF-8 path"),
    ]));
    json(storyboard().args([
        "cut-sheet",
        dir.to_str().expect("UTF-8 path"),
        "--out",
        sheet_out.to_str().expect("UTF-8 path"),
    ]));
    assert_png(&panel_out);
    assert_png(&sheet_out);
    assert!(version.len() > 10);
}

#[test]
fn cli_render_refuses_project_json_output() {
    let (_workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let project = dir.join("project.json");
    let result = error(storyboard().args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--out",
        project.to_str().expect("UTF-8 path"),
    ]));
    assert_eq!(result["error"]["kind"], "validation");
    assert!(
        fs::read_to_string(&project)
            .expect("project remains readable")
            .contains("nb-storyboard/v1")
    );
    assert!(version.len() > 10);
}

#[cfg(unix)]
#[test]
fn cli_render_refuses_out_through_symlink_to_project() {
    use std::os::unix::fs::symlink;

    let (workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let link = workspace.path().join("plink");
    symlink(&dir, &link).expect("project symlink");
    let project = dir.join("project.json");
    let mut command = storyboard();
    command.args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--out",
        link.join("project.json").to_str().expect("UTF-8 path"),
    ]);
    assert_refuses_without_overwrite(&mut command, &project);
}

#[cfg(unix)]
#[test]
fn cli_cut_sheet_refuses_out_through_symlink_to_project() {
    use std::os::unix::fs::symlink;

    let (workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let link = workspace.path().join("plink");
    symlink(&dir, &link).expect("project symlink");
    let project = dir.join("project.json");
    let mut command = storyboard();
    command.args([
        "cut-sheet",
        dir.to_str().expect("UTF-8 path"),
        "--out",
        link.join("project.json").to_str().expect("UTF-8 path"),
    ]);
    assert_refuses_without_overwrite(&mut command, &project);
}

#[cfg(unix)]
#[test]
fn cli_render_refuses_project_given_through_symlink() {
    use std::os::unix::fs::symlink;

    let (workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let link = workspace.path().join("plink");
    symlink(&dir, &link).expect("project symlink");
    let project = dir.join("project.json");
    let mut command = storyboard();
    command.args([
        "panel-image",
        link.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--out",
        project.to_str().expect("UTF-8 path"),
    ]);
    assert_refuses_without_overwrite(&mut command, &project);
}

#[test]
fn cli_render_refuses_case_variant_path() {
    let (workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let case_variant = workspace.path().join("P");
    if fs::canonicalize(&case_variant).ok() != fs::canonicalize(&dir).ok() {
        return;
    }
    let project = dir.join("project.json");
    let mut command = storyboard();
    command.args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--out",
        case_variant
            .join("project.json")
            .to_str()
            .expect("UTF-8 path"),
    ]);
    assert_refuses_without_overwrite(&mut command, &project);
}

#[cfg(unix)]
#[test]
fn cli_render_refuses_symlinked_cache() {
    use std::os::unix::fs::symlink;
    let (workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    fs::remove_dir(dir.join(".cache")).expect("remove owned empty cache");
    let outside = workspace.path().join("outside");
    fs::create_dir(&outside).expect("outside directory");
    symlink(&outside, dir.join(".cache")).expect("cache symlink");
    let result = error(storyboard().args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
    ]));
    assert_eq!(result["error"]["kind"], "validation");
    assert!(
        fs::read_dir(outside)
            .expect("outside remains empty")
            .next()
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn cli_render_refuses_parent_segments_after_a_symlink() {
    use std::os::unix::fs::symlink;
    let (_workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let cache = dir.join(".cache");
    let real = dir.join("real-cache");
    fs::rename(&cache, &real).expect("move cache");
    symlink(&real, &cache).expect("cache symlink");
    let out = cache.join("..").join("project.json");
    let result = error(storyboard().args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--out",
        out.to_str().expect("UTF-8 path"),
    ]));
    assert_eq!(result["error"]["kind"], "validation");
    assert!(
        fs::read_to_string(dir.join("project.json"))
            .expect("project remains readable")
            .contains("nb-storyboard/v1")
    );
}

#[test]
fn cli_undo_reverts_a_write() {
    let (_workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let undone = json(storyboard().args([
        "undo",
        dir.to_str().expect("UTF-8 path"),
        "--base-version",
        &version,
        "--author",
        "human",
    ]));
    assert!(undone["version"].is_string());
    let shown = json(storyboard().args(["show", dir.to_str().expect("UTF-8 path")]));
    assert!(shown["shots"].as_array().expect("shots").is_empty());
}

#[test]
fn cli_redo_reapplies_an_undone_write() {
    let (_workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let undone = write(&dir, &version, &["undo"]);
    let redone = write(
        &dir,
        undone["version"].as_str().expect("undo version"),
        &["redo"],
    );
    assert!(
        redone["summary"]
            .as_str()
            .expect("summary")
            .contains("redo")
    );
    let shown = json(storyboard().args(["show", dir.to_str().expect("UTF-8 path")]));
    assert_eq!(shown["shots"].as_array().expect("shots").len(), 1);
}

#[test]
fn cli_pretty_show_is_a_readable_table() {
    let (_workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let output = storyboard()
        .args(["--pretty", "show", dir.to_str().expect("UTF-8 path")])
        .output()
        .expect("show");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(text.contains("SCENES"));
    assert!(text.contains("Opening"));
    assert!(text.contains("Arrival"));
    assert!(!text.trim_start().starts_with('{'));
}

#[test]
fn cli_pretty_show_includes_stickies_and_cut_positions() {
    let (_workspace, dir, version) = new_project();
    let mut version = add_opening_shot(&dir, &version);
    version = write_version(&dir, &version, &["add-cut", "--name", "Trailer"]);
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-1"],
    );
    let _version = write_version(&dir, &version, &["note", "--text", "Check the door"]);
    let output = storyboard()
        .args([
            "--pretty",
            "show",
            dir.to_str().expect("UTF-8 path"),
            "--cut",
            "cut-1",
        ])
        .output()
        .expect("show");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(text.contains("SEQUENCE (cut-1)"));
    assert!(text.contains("1  shot-1"));
    assert!(text.contains("STICKIES"));
    assert!(text.contains("Check the door"));
}

#[test]
fn cli_pretty_help_says_show_only() {
    let output = storyboard().args(["--help"]).output().expect("help");
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 help")
            .contains("show only")
    );
}

#[test]
fn cli_describe_reads_panel_layers_and_pretty_show_points_to_it() {
    let (_workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let _version = write_version(&dir, &version, &["add-text", "--at", "1", "--text", "Door"]);
    let described = json(storyboard().args(["describe", dir.to_str().unwrap(), "--at", "1"]));
    assert_eq!(described["layers"][0]["id"], "layer-1");
    assert_eq!(described["layers"][0]["kind"], "text");
    assert_eq!(described["layers"][0]["text"], "Door");
    assert!(described["timing"]["panel_time"].is_number());
    assert!(described["notes"].is_string());

    let output = storyboard()
        .args(["--pretty", "show", dir.to_str().unwrap()])
        .output()
        .expect("pretty show");
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("describe --at 1")
    );
}

#[test]
fn cli_sources_show_missing_refs_and_add_ref_warns() {
    let (_workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let added = write(
        &dir,
        &version,
        &[
            "add-ref",
            "--shot",
            "shot-1",
            "--reference",
            "sources/gone.mov",
        ],
    );
    assert!(
        added["warning"]
            .as_str()
            .is_some_and(|value| value.contains("gone.mov"))
    );
    assert!(added["summary"].as_str().unwrap().contains("shot shot-1"));
    let sources = json(storyboard().args(["sources", dir.to_str().unwrap()]));
    assert_eq!(sources["missing"][0]["path"], "sources/gone.mov");
    let shown = json(storyboard().args(["show", dir.to_str().unwrap()]));
    assert!(
        shown["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["message"].as_str().unwrap().contains("gone.mov"))
    );
}

#[test]
fn cli_notes_all_nests_replies_and_reply_reopens_thread() {
    let (_workspace, dir, version) = new_project();
    let note = write(
        &dir,
        &version,
        &["note", "--kind", "question", "--text", "Which door?"],
    );
    let resolved = write(
        &dir,
        note["version"].as_str().unwrap(),
        &["resolve", "--sticky", "sticky-1"],
    );
    let reply = write(
        &dir,
        resolved["version"].as_str().unwrap(),
        &["reply", "--sticky", "sticky-1", "--text", "The red one"],
    );
    assert!(reply["summary"].as_str().unwrap().contains("reopen"));
    let notes = json(storyboard().args(["notes", dir.to_str().unwrap(), "--all"]));
    assert_eq!(notes["notes"][0]["resolved"], false);
    assert_eq!(notes["notes"][0]["replies"][0]["text"], "The red one");
}

#[test]
fn cli_has_human_verbs_for_removal_branching_reorder_and_refs() {
    let help = String::from_utf8(storyboard().arg("--help").output().unwrap().stdout).unwrap();
    for verb in [
        "remove-shot",
        "remove-scene",
        "remove-panel",
        "remove-layer",
        "set-active-cut",
        "reorder-clip",
        "add-ref",
        "remove-ref",
        "move-shot-to-scene",
        "reorder-shot",
    ] {
        assert!(help.contains(verb), "missing CLI verb {verb}");
    }
}

#[test]
fn cli_surface_verbs_execute_without_json() {
    let (_workspace, dir, mut version) = new_project();
    version = write_version(&dir, &version, &["add-scene", "--title", "One"]);
    version = write_version(&dir, &version, &["add-scene", "--title", "Two"]);
    version = write_version(
        &dir,
        &version,
        &["add-shot", "--scene", "scene-1", "--title", "A"],
    );
    version = write_version(
        &dir,
        &version,
        &["add-shot", "--scene", "scene-1", "--title", "B"],
    );
    version = write_version(&dir, &version, &["add-text", "--at", "1", "--text", "Door"]);
    version = write_version(
        &dir,
        &version,
        &["reorder-shot", "--shot", "shot-2", "--index", "0"],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "move-shot-to-scene",
            "--shot",
            "shot-2",
            "--scene",
            "scene-2",
            "--index",
            "0",
        ],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "add-ref",
            "--shot",
            "shot-1",
            "--reference",
            "sources/gone.png",
        ],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "remove-ref",
            "--shot",
            "shot-1",
            "--reference",
            "sources/gone.png",
        ],
    );
    version = write_version(&dir, &version, &["add-cut", "--name", "Alt"]);
    version = write_version(&dir, &version, &["set-active-cut", "--cut", "cut-1"]);
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-1"],
    );
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-2"],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "reorder-clip",
            "--cut",
            "cut-1",
            "--clip",
            "clip-2",
            "--index",
            "0",
        ],
    );
    version = write_version(
        &dir,
        &version,
        &["remove-clip", "--cut", "cut-1", "--clip", "clip-1"],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "remove-layer",
            "--shot",
            "shot-1",
            "--take",
            "take-1",
            "--panel",
            "panel-1",
            "--layer",
            "layer-1",
        ],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "remove-panel",
            "--shot",
            "shot-1",
            "--take",
            "take-1",
            "--panel",
            "panel-1",
        ],
    );
    version = write_version(&dir, &version, &["remove-shot", "--shot", "shot-1"]);
    let _version = write_version(&dir, &version, &["remove-scene", "--scene", "scene-1"]);
}

#[test]
fn cli_failed_new_removes_created_project_folder() {
    let workspace = TempDir::new().unwrap();
    let project = workspace.path().join("invalid");
    let rejected = error(storyboard().args([
        "new",
        project.to_str().unwrap(),
        "--title",
        "Bad",
        "--fps",
        "23",
    ]));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(
        !project.exists(),
        "failed new left its project folder behind"
    );
}

#[test]
fn cli_import_collision_has_actionable_hint() {
    let (workspace, dir, _version) = new_project();
    let input = workspace.path().join("input.txt");
    fs::write(&input, "one").unwrap();
    json(storyboard().args([
        "import",
        dir.to_str().unwrap(),
        input.to_str().unwrap(),
        "--destination",
        "same.txt",
    ]));
    let rejected = error(storyboard().args([
        "import",
        dir.to_str().unwrap(),
        input.to_str().unwrap(),
        "--destination",
        "same.txt",
    ]));
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("same.txt")
    );
    assert!(
        rejected["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("choose another name")
    );
}

#[test]
fn cli_schema_single_op_carries_referenced_defs() {
    let schema = json(storyboard().args(["schema", "add-ref"]));
    assert!(schema["op"].to_string().contains("#/$defs/RefTarget"));
    assert!(schema["$defs"]["RefTarget"].is_object());
}

#[test]
fn cli_thirty_scene_add_text_and_board_render_stays_under_two_seconds() {
    use std::time::{Duration, Instant};

    use storyboard_cli::{args::PanelImageArgs, ops_api};
    use storyboard_core::Author;

    let (_workspace, project, version) = fixture_support::thirty_scene_project();
    let started = Instant::now();
    let op = ops_api::add_text_op(
        &project,
        None,
        None,
        None,
        Some("1"),
        "Performance marker".into(),
        0.0,
        0.0,
        400.0,
        100.0,
        32.0,
        "#000000".into(),
    )
    .expect("build add-text op");
    ops_api::commit_ops(&project, &version, vec![op], Author::Human, None)
        .expect("commit add-text");
    ops_api::panel_image(PanelImageArgs {
        dir: project,
        shot: None,
        take: None,
        panel: None,
        at: Some("1".into()),
        width: 960,
        mode: "board".into(),
        out: None,
    })
    .expect("render board panel");
    let elapsed = started.elapsed();
    eprintln!("30-scene add-text plus board render: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(2),
        "edit plus render took {elapsed:?}"
    );
}

#[test]
fn cli_schema_prints_op_shapes() {
    let all = json(storyboard().args(["schema"]));
    assert!(all["ops"]["oneOf"].as_array().expect("op variants").len() > 10);
    let one = json(storyboard().args(["schema", "add_shot"]));
    assert!(one["op"]["properties"].is_object() || one["op"]["$ref"].is_string());
    let dashed = json(storyboard().args(["schema", "add-shot"]));
    assert_eq!(dashed["op"], one["op"]);
}

#[test]
fn cli_every_flag_has_help() {
    fn walk(command: &ClapCommand) {
        for argument in command.get_arguments() {
            assert!(
                argument.get_help().is_some(),
                "{} argument {:?} lacks help",
                command.get_name(),
                argument.get_id()
            );
        }
        for subcommand in command.get_subcommands() {
            walk(subcommand);
        }
    }
    walk(&Cli::command());
}

#[test]
fn cli_add_image_accepts_paths_relative_to_sources() {
    let (_workspace, dir, version) = new_project();
    let mut version = add_opening_shot(&dir, &version);
    write_png(&dir.join("sources/art/red.png"));
    version = write_version(
        &dir,
        &version,
        &["add-image", "--at", "1", "--src", "art/red.png"],
    );
    let rendered = json(
        storyboard().args([
            "panel-image",
            dir.to_str().expect("UTF-8 path"),
            "--at",
            "1",
            "--out",
            dir.join(".cache/relative.png")
                .to_str()
                .expect("UTF-8 path"),
        ]),
    );
    assert!(
        rendered["warnings"]
            .as_array()
            .expect("warnings")
            .is_empty()
    );
    let _version = write_version(
        &dir,
        &version,
        &["add-image", "--at", "1", "--src", "sources/art/red.png"],
    );
}

#[test]
fn cli_trim_and_select_take_succeed() {
    let (_workspace, dir, version) = new_project();
    let mut version = add_opening_shot(&dir, &version);
    version = write_version(
        &dir,
        &version,
        &["add-take", "--shot", "shot-1", "--name", "Alt"],
    );
    version = write_version(
        &dir,
        &version,
        &["select-take", "--shot", "shot-1", "--take", "take-2"],
    );
    version = write_version(&dir, &version, &["add-cut", "--name", "Trailer"]);
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-1"],
    );
    let result = write(
        &dir,
        &version,
        &[
            "trim", "--cut", "cut-1", "--clip", "clip-1", "--in", "1", "--out", "10",
        ],
    );
    assert_eq!(
        result["version"].as_str().map(str::len).unwrap_or_default(),
        64
    );
}

#[test]
fn cli_add_panel_accepts_at_and_targets_panel_selector() {
    let (_workspace, dir, version) = new_project();
    let mut version = add_opening_shot(&dir, &version);
    version = write_version(&dir, &version, &["add-panel", "--at", "1"]);
    version = write_version(
        &dir,
        &version,
        &["add-text", "--at", "1/2", "--text", "Second panel"],
    );
    let shown = json(storyboard().args(["show", dir.to_str().expect("UTF-8 path")]));
    assert_eq!(shown["scenes"][0]["shots"][0]["takes"][0]["panels"], 2);
    let _version = version;
}

#[test]
fn cli_write_summaries_name_each_requested_operation() {
    let (_workspace, dir, version) = new_project();
    let mut version = version;
    let add_scene = write(&dir, &version, &["add-scene", "--title", "Opening"]);
    assert!(
        add_scene["summary"]
            .as_str()
            .expect("summary")
            .contains("added scene \u{201c}Opening\u{201d}")
    );
    version = add_scene["version"].as_str().expect("version").into();
    let add_second_scene = write(&dir, &version, &["add-scene", "--title", "Ending"]);
    version = add_second_scene["version"]
        .as_str()
        .expect("version")
        .into();
    let add_shot = write(
        &dir,
        &version,
        &["add-shot", "--scene", "scene-1", "--title", "Arrival"],
    );
    assert!(
        add_shot["summary"]
            .as_str()
            .expect("summary")
            .contains("added shot \u{201c}Arrival\u{201d} to \u{201c}Opening\u{201d}")
    );
    version = add_shot["version"].as_str().expect("version").into();
    let add_image = write(
        &dir,
        &version,
        &["add-image", "--at", "1", "--src", "art/red.png"],
    );
    assert!(
        add_image["summary"]
            .as_str()
            .expect("summary")
            .contains("added image layer to \u{201c}Arrival\u{201d}")
    );
    version = add_image["version"].as_str().expect("version").into();
    version = write_version(
        &dir,
        &version,
        &["add-take", "--shot", "shot-1", "--name", "Alt"],
    );
    let select = write(
        &dir,
        &version,
        &["select-take", "--shot", "shot-1", "--take", "take-2"],
    );
    assert!(
        select["summary"]
            .as_str()
            .expect("summary")
            .contains("selected take \u{201c}Alt\u{201d} for \u{201c}Arrival\u{201d}")
    );
    version = select["version"].as_str().expect("version").into();
    let note = write(&dir, &version, &["note", "--text", "Check doorway"]);
    assert!(
        note["summary"]
            .as_str()
            .expect("summary")
            .contains("added note")
    );
    version = note["version"].as_str().expect("version").into();
    let sticky = note["ids"][0].as_str().expect("sticky").to_owned();
    let reply = write(
        &dir,
        &version,
        &["reply", "--sticky", &sticky, "--text", "Will do"],
    );
    assert!(
        reply["summary"]
            .as_str()
            .expect("summary")
            .contains("replied to note")
    );
    version = reply["version"].as_str().expect("version").into();
    let resolve = write(&dir, &version, &["resolve", "--sticky", &sticky]);
    assert!(
        resolve["summary"]
            .as_str()
            .expect("summary")
            .contains("resolved note")
    );
    version = resolve["version"].as_str().expect("version").into();
    let moved = write(
        &dir,
        &version,
        &[
            "move-shot",
            "--shot",
            "shot-1",
            "--scene",
            "scene-2",
            "--index",
            "0",
        ],
    );
    assert!(
        moved["summary"]
            .as_str()
            .expect("summary")
            .contains("moved \u{201c}Arrival\u{201d} to \u{201c}Ending\u{201d}")
    );
}

#[test]
fn cli_parse_errors_preserve_clap_usage_when_base_version_was_supplied() {
    let (_workspace, dir, version) = new_project();
    for args in [
        vec![
            "add-shot",
            dir.to_str().expect("UTF-8 path"),
            "--base-version",
            &version,
            "--author",
            "human",
            "--scene",
            "scene-1",
            "--titel",
            "Arrival",
        ],
        vec![
            "add-shot",
            dir.to_str().expect("UTF-8 path"),
            "--base-version",
            &version,
            "--author",
            "human",
            "--title",
            "Arrival",
        ],
        vec![
            "set-shot",
            dir.to_str().expect("UTF-8 path"),
            "--base-version",
            &version,
            "--author",
            "human",
            "--shot",
            "shot-1",
            "--duration",
            "-1",
        ],
    ] {
        let result = error(storyboard().args(args));
        assert_eq!(result["error"]["kind"], "usage");
        assert_ne!(
            result["error"]["message"],
            "a write requires --base-version"
        );
        assert!(
            result["error"]["hint"]
                .as_str()
                .is_some_and(|hint| !hint.is_empty())
        );
    }
}

#[test]
fn cli_set_shot_story_text_shows_in_caption() {
    let (workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let version = write_version(
        &dir,
        &version,
        &[
            "set-shot",
            "--shot",
            "shot-1",
            "--action",
            "Mara opens the door",
            "--dialogue",
            "Hello",
            "--duration",
            "2.5",
            "--notes",
            "Keep it quiet",
        ],
    );
    let out = workspace.path().join("caption.png");
    json(storyboard().args([
        "panel-image",
        dir.to_str().expect("UTF-8 path"),
        "--at",
        "1",
        "--mode",
        "board",
        "--out",
        out.to_str().expect("UTF-8 path"),
    ]));
    assert_png(&out);
    assert_eq!(version.len(), 64);
}

#[test]
fn cli_import_sources_notes_reply_resolve_move_shot_and_history_succeed() {
    let (workspace, dir, version) = new_project();
    let mut version = write_version(&dir, &version, &["add-scene", "--title", "Opening"]);
    version = write_version(&dir, &version, &["add-scene", "--title", "Ending"]);
    version = write_version(
        &dir,
        &version,
        &["add-shot", "--scene", "scene-1", "--title", "Arrival"],
    );
    version = write_version(
        &dir,
        &version,
        &[
            "move-shot",
            "--shot",
            "shot-1",
            "--scene",
            "scene-2",
            "--index",
            "0",
        ],
    );
    let source = workspace.path().join("source.png");
    write_png(&source);
    let imported = json(storyboard().args([
        "import",
        dir.to_str().expect("UTF-8 path"),
        source.to_str().expect("UTF-8 path"),
        "--destination",
        "art/red.png",
    ]));
    assert_eq!(imported["path"], "sources/art/red.png");
    assert_eq!(
        json(storyboard().args(["sources", dir.to_str().expect("UTF-8 path")]))["sources"]
            .as_array()
            .expect("sources")
            .len(),
        1
    );
    let noted = write(
        &dir,
        &version,
        &["note", "--text", "Check doorway", "--pin", "shot:shot-1"],
    );
    version = noted["version"].as_str().expect("version").into();
    let sticky = noted["ids"][0].as_str().expect("sticky").to_owned();
    version = write_version(
        &dir,
        &version,
        &["reply", "--sticky", &sticky, "--text", "Will do"],
    );
    version = write_version(&dir, &version, &["resolve", "--sticky", &sticky]);
    assert_eq!(json(storyboard().args(["notes", dir.to_str().expect("UTF-8 path"), "--resolved"]))["notes"].as_array().expect("notes").len(), 1);
    assert!(
        !json(storyboard().args(["history", dir.to_str().expect("UTF-8 path")]))["history"]
            .as_array()
            .expect("history")
            .is_empty()
    );
    assert_eq!(version.len(), 64);
}

#[test]
fn cli_write_results_include_new_ids_and_version() {
    let (_workspace, dir, version) = new_project();
    let result = write(&dir, &version, &["add-scene", "--title", "Opening"]);
    assert_eq!(result["ids"], serde_json::json!(["scene-1"]));
    assert_eq!(result["version"].as_str().map(str::len), Some(64));
}

#[test]
fn cli_write_reports_every_created_id() {
    let (_workspace, dir, version) = new_project();
    let version = write_version(&dir, &version, &["add-scene", "--title", "Opening"]);
    let result = write(
        &dir,
        &version,
        &["add-shot", "--scene", "scene-1", "--title", "Arrival"],
    );
    assert_eq!(
        result["ids"],
        serde_json::json!(["panel-1", "shot-1", "take-1"])
    );
}

#[test]
fn cli_render_refuses_sibling_project() {
    let workspace = TempDir::new().expect("workspace");
    let project = workspace.path().join("story");
    let sibling = workspace.path().join("sibling");
    let created = json(storyboard().args([
        "new",
        project.to_str().expect("UTF-8 project"),
        "--title",
        "Story",
    ]));
    let sibling_created = json(storyboard().args([
        "new",
        sibling.to_str().expect("UTF-8 sibling"),
        "--title",
        "Sibling",
    ]));
    let version = add_opening_shot(
        &project,
        created["version"].as_str().expect("project version"),
    );
    let sibling_document = sibling.join("project.json");
    let before = fs::read(&sibling_document).expect("sibling document");
    let result = error(storyboard().args([
        "panel-image",
        project.to_str().expect("UTF-8 project"),
        "--at",
        "1",
        "--out",
        sibling_document.to_str().expect("UTF-8 sibling document"),
    ]));
    assert_eq!(result["error"]["kind"], "validation");
    assert_eq!(
        fs::read(sibling.join("project.json")).expect("sibling document"),
        before
    );
    assert_eq!(version.len(), 64);
    assert!(sibling_created["version"].is_string());
}

#[cfg(unix)]
#[test]
fn cli_render_replaces_hardlink_without_touching_sibling() {
    use std::os::unix::fs::MetadataExt;

    let workspace = TempDir::new().expect("workspace");
    let project = workspace.path().join("story");
    let sibling = workspace.path().join("sibling");
    let created = json(storyboard().args([
        "new",
        project.to_str().expect("UTF-8 project"),
        "--title",
        "Story",
    ]));
    json(storyboard().args([
        "new",
        sibling.to_str().expect("UTF-8 sibling"),
        "--title",
        "Sibling",
    ]));
    let _version = add_opening_shot(
        &project,
        created["version"].as_str().expect("project version"),
    );
    let sibling_document = sibling.join("project.json");
    let before = fs::read(&sibling_document).expect("sibling document");
    let output = workspace.path().join("hl.png");
    fs::hard_link(&sibling_document, &output).expect("hardlink output to sibling document");

    json(storyboard().args([
        "cut-sheet",
        project.to_str().expect("UTF-8 project"),
        "--out",
        output.to_str().expect("UTF-8 output"),
    ]));

    assert!(
        fs::read(&sibling_document).expect("sibling document after render") == before,
        "render changed sibling project.json through the output hardlink"
    );
    assert_png(&output);
    assert_eq!(fs::metadata(&output).expect("output metadata").nlink(), 1);
}

#[test]
fn cli_new_refuses_target_whose_ancestor_is_project() {
    let workspace = TempDir::new().expect("workspace");
    let project = workspace.path().join("Story");
    json(storyboard().args([
        "new",
        project.to_str().expect("UTF-8 project"),
        "--title",
        "Story",
    ]));

    let nested = project.join("nested");
    let rejected = error(storyboard().args([
        "new",
        nested.to_str().expect("UTF-8 nested project"),
        "--title",
        "Nested",
    ]));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(!nested.join("project.json").exists());

    let case_variant = workspace.path().join("sTORY");
    if fs::canonicalize(&case_variant).ok() == fs::canonicalize(&project).ok() {
        let nested = case_variant.join("case-nested");
        let rejected = error(storyboard().args([
            "new",
            nested.to_str().expect("UTF-8 case-variant project"),
            "--title",
            "Case nested",
        ]));
        assert_eq!(rejected["error"]["kind"], "validation");
        assert!(!project.join("case-nested/project.json").exists());
    }
}

#[test]
fn cli_render_allows_parent_segments_outside_projects() {
    let workspace = TempDir::new().expect("workspace");
    let caller = workspace.path().join("caller");
    let renders = workspace.path().join("renders");
    fs::create_dir(&caller).expect("caller directory");
    fs::create_dir(&renders).expect("renders directory");
    let project = workspace.path().join("story");
    let created = json(storyboard().args([
        "new",
        project.to_str().expect("UTF-8 project"),
        "--title",
        "Story",
    ]));
    let _version = add_opening_shot(
        &project,
        created["version"].as_str().expect("project version"),
    );

    json(storyboard().current_dir(&caller).args([
        "cut-sheet",
        project.to_str().expect("UTF-8 project"),
        "--out",
        "../renders/x.png",
    ]));

    assert_png(&renders.join("x.png"));
}

#[test]
fn cli_actionable_errors_list_valid_choices() {
    let (_workspace, dir, version) = new_project();
    let _version = add_opening_shot(&dir, &version);
    let invalid_kind = error(storyboard().args([
        "notes",
        dir.to_str().expect("UTF-8 path"),
        "--kind",
        "bogus",
    ]));
    assert_eq!(
        invalid_kind["error"]["valid"],
        serde_json::json!(["note", "todo", "question"])
    );
    let invalid_pin = error(storyboard().args([
        "note",
        dir.to_str().expect("UTF-8 path"),
        "--base-version",
        &version,
        "--author",
        "human",
        "--text",
        "x",
        "--pin",
        "bogus",
    ]));
    assert!(
        invalid_pin["error"]["valid"]
            .as_array()
            .expect("pin forms")
            .contains(&Value::String("panel:H/T/P".into()))
    );
    let invalid_cut =
        error(storyboard().args(["show", dir.to_str().expect("UTF-8 path"), "--cut", "nope"]));
    assert!(invalid_cut["error"]["valid"].is_array());
}

#[test]
fn cli_write_requires_author_or_nb_author() {
    let (_workspace, dir, version) = new_project();
    storyboard()
        .env_remove("NB_AUTHOR")
        .args([
            "add-scene",
            dir.to_str().expect("UTF-8 path"),
            "--base-version",
            &version,
            "--title",
            "Opening",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("\"hint\""));
}

#[test]
fn cli_replaces_shot_movie_from_agent_output_and_undoes_link() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let (workspace, dir, version) = new_project();
    let version = add_opening_shot(&dir, &version);
    let movie = workspace.path().join("generated.mp4");
    let encoded = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x90:r=24",
            "-t",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&movie)
        .output()
        .expect("generate MP4 fixture");
    assert!(
        encoded.status.success(),
        "{}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    let bad = error(storyboard().args([
        "replace-movie",
        dir.to_str().unwrap(),
        "--base-version",
        &version,
        "--author",
        "agent:claude",
        "--shot",
        "missing",
        "--source",
        movie.to_str().unwrap(),
    ]));
    assert_eq!(bad["error"]["kind"], "validation");
    let replaced = json(storyboard().args([
        "replace-movie",
        dir.to_str().unwrap(),
        "--base-version",
        &version,
        "--author",
        "agent:claude",
        "--shot",
        "shot-1",
        "--source",
        movie.to_str().unwrap(),
    ]));
    let stored = replaced["path"].as_str().expect("project source path");
    let folder = stored
        .strip_prefix("sources/imported-movies/")
        .and_then(|path| path.strip_suffix("/original.mp4"))
        .expect("generated movie has a streamable source path");
    assert!(uuid::Uuid::parse_str(folder).is_ok());
    assert_eq!(
        fs::read(&movie).unwrap(),
        fs::read(dir.join(stored)).unwrap()
    );
    let shot = &storyboard_store::open(&dir).unwrap().project.scenes[0].shots[0];
    assert_eq!(shot.source_movie.as_ref().unwrap().src, stored);
    assert_eq!(shot.duration, 2.0);
    let undone = json(storyboard().args([
        "undo",
        dir.to_str().unwrap(),
        "--base-version",
        replaced["version"].as_str().unwrap(),
        "--author",
        "agent:claude",
    ]));
    assert!(undone["version"].is_string());
    assert!(
        storyboard_store::open(&dir).unwrap().project.scenes[0].shots[0]
            .source_movie
            .is_none()
    );
    assert!(
        dir.join(stored).is_file(),
        "copied source remains recoverable for history"
    );
}

#[test]
fn cli_exports_numbered_panels_then_replacement_movie_with_audio() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let workspace = TempDir::new().unwrap();
    let dir = workspace.path().join("project");
    let initial = json(storyboard().args([
        "new",
        dir.to_str().unwrap(),
        "--title",
        "Export",
        "--width",
        "320",
        "--height",
        "180",
    ]));
    let scene_version = write_version(
        &dir,
        initial["version"].as_str().unwrap(),
        &["add-scene", "--title", "Scene"],
    );
    let red = workspace.path().join("frame-001.png");
    let green = workspace.path().join("frame-002.png");
    ImageBuffer::<Rgba<u8>, _>::from_pixel(320, 180, Rgba([255, 0, 0, 255]))
        .save(&red)
        .unwrap();
    ImageBuffer::<Rgba<u8>, _>::from_pixel(320, 180, Rgba([0, 255, 0, 255]))
        .save(&green)
        .unwrap();
    let imported = json(storyboard().args([
        "import-shot",
        dir.to_str().unwrap(),
        "--base-version",
        &scene_version,
        "--author",
        "human",
        "--scene",
        "scene-1",
        "--title",
        "Boards",
        "--duration",
        "1",
        red.to_str().unwrap(),
        green.to_str().unwrap(),
    ]));
    let second_version = write_version(
        &dir,
        imported["version"].as_str().unwrap(),
        &["add-shot", "--scene", "scene-1", "--title", "Replacement"],
    );
    let movie = workspace.path().join("blue.mp4");
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=320x180:r=24:d=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            "-y",
        ])
        .arg(&movie)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    json(storyboard().args([
        "replace-movie",
        dir.to_str().unwrap(),
        "--base-version",
        &second_version,
        "--author",
        "human",
        "--shot",
        "shot-2",
        "--source",
        movie.to_str().unwrap(),
    ]));
    let document_before = fs::read(dir.join("project.json")).unwrap();
    let result = json(storyboard().args(["export-movie", dir.to_str().unwrap()]));
    let output = Path::new(result["path"].as_str().unwrap());
    assert!(output.is_file());
    assert_eq!(result["frames"], 48);
    assert_eq!(
        fs::read(dir.join("project.json")).unwrap(),
        document_before,
        "export must not edit the project"
    );
    let count = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(count.status.success());
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "48");
    let audio = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&audio.stdout).trim(), "aac");
    let pixel = |seconds: &str| {
        let frame = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                seconds,
            ])
            .arg("-i")
            .arg(output)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=1:1",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(frame.status.success());
        frame.stdout
    };
    let first = pixel("0.25");
    let second = pixel("0.75");
    let third = pixel("1.25");
    assert!(
        first[0] > 150 && first[1] < 100 && first[2] < 100,
        "red panel: {first:?}"
    );
    assert!(
        second[1] > 150 && second[0] < 100 && second[2] < 100,
        "green panel: {second:?}"
    );
    assert!(
        third[2] > 100 && third[0] < 100 && third[1] < 100,
        "blue movie: {third:?}"
    );
    let cached = json(storyboard().args(["export-movie", dir.to_str().unwrap()]));
    assert_eq!(cached["cached"], true);
}

#[test]
fn cli_export_holds_last_frame_while_voice_over_continues() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let workspace = TempDir::new().unwrap();
    let dir = workspace.path().join("project");
    let initial = json(storyboard().args([
        "new",
        dir.to_str().unwrap(),
        "--title",
        "Hold",
        "--width",
        "320",
        "--height",
        "180",
    ]));
    let shot_version = add_opening_shot(&dir, initial["version"].as_str().unwrap());
    let movie = workspace.path().join("movie.mp4");
    let voice = workspace.path().join("voice.wav");
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=320x180:r=120:d=1",
            "-vf",
            "drawbox=x=0:y=0:w=iw:h=ih:color=blue:t=fill:enable='eq(n,119)'",
            "-an",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&movie)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=2",
            "-y",
        ])
        .arg(&voice)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let replaced = json(storyboard().args([
        "replace-movie",
        dir.to_str().unwrap(),
        "--base-version",
        &shot_version,
        "--author",
        "human",
        "--shot",
        "shot-1",
        "--source",
        movie.to_str().unwrap(),
    ]));
    let relative = std::path::PathBuf::from("sources/imported-audio")
        .join(uuid::Uuid::new_v4().to_string())
        .join("original.wav");
    storyboard_store::import_source(&dir, &voice, &relative).unwrap();
    let movie_source = storyboard_store::open(&dir).unwrap().project.scenes[0].shots[0]
        .source_movie
        .clone()
        .unwrap();
    let patch: storyboard_core::Patch = serde_json::from_value(serde_json::json!({
        "duration": 2.0,
        "source_movie": {"src": movie_source.src, "start": 0.5, "end": 1.0},
        "source_audio": {"src": relative.to_str().unwrap(), "start": 0.0, "end": 2.0}
    }))
    .unwrap();
    storyboard_store::commit(
        &dir,
        replaced["version"].as_str().unwrap(),
        vec![storyboard_core::Op::UpdateShot {
            shot: "shot-1".into(),
            patch,
        }],
        storyboard_core::Author::Human,
        None,
    )
    .unwrap();
    let result = json(storyboard().args(["export-movie", dir.to_str().unwrap()]));
    let output = Path::new(result["path"].as_str().unwrap());
    assert_eq!(result["frames"], 48);
    let video_frames = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&video_frames.stdout).trim(), "48");
    let held = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            "1.5",
        ])
        .arg("-i")
        .arg(output)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=1:1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(held.status.success());
    assert!(
        held.stdout[2] > 100 && held.stdout[0] < 100 && held.stdout[1] < 100,
        "held blue frame: {:?}",
        held.stdout
    );
    let late_voice = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            "1.4",
        ])
        .arg("-i")
        .arg(output)
        .args(["-t", "0.2", "-vn", "-ac", "1", "-f", "s16le", "pipe:1"])
        .output()
        .unwrap();
    assert!(late_voice.status.success());
    assert!(
        late_voice
            .stdout
            .chunks_exact(2)
            .any(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]).unsigned_abs() > 500),
        "voice-over should continue after the movie ends"
    );

    // Starting a placement after the movie ends must hold the actual last source frame.
    let mut version = storyboard_store::open(&dir).unwrap().version;
    version = write_version(&dir, &version, &["add-cut", "--name", "Late start"]);
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-1"],
    );
    let _version = write_version(
        &dir,
        &version,
        &[
            "trim", "--cut", "cut-1", "--clip", "clip-1", "--in", "36", "--out", "48",
        ],
    );
    let late = json(storyboard().args(["export-movie", dir.to_str().unwrap(), "--cut", "cut-1"]));
    assert_eq!(late["frames"], 12);
    let late_frame = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(late["path"].as_str().unwrap())
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=1:1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(late_frame.status.success());
    assert!(
        late_frame.stdout[2] > 100 && late_frame.stdout[0] < 100 && late_frame.stdout[1] < 100,
        "late-start held frame: {:?}",
        late_frame.stdout
    );
}

#[test]
fn cli_export_uses_placement_voice_only_for_that_clip() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let workspace = TempDir::new().unwrap();
    let dir = workspace.path().join("project");
    let initial = json(storyboard().args([
        "new",
        dir.to_str().unwrap(),
        "--title",
        "Placement voice",
        "--width",
        "320",
        "--height",
        "180",
    ]));
    let mut version = add_opening_shot(&dir, initial["version"].as_str().unwrap());
    version = write_version(&dir, &version, &["add-cut", "--name", "Revision"]);
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-1"],
    );
    version = write_version(
        &dir,
        &version,
        &["add-clip", "--cut", "cut-1", "--shot", "shot-1"],
    );
    let voice = workspace.path().join("voice.wav");
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=3",
            "-y",
        ])
        .arg(&voice)
        .output()
        .unwrap();
    assert!(generated.status.success());
    let relative = std::path::PathBuf::from("sources/imported-audio")
        .join(uuid::Uuid::new_v4().to_string())
        .join("original.wav");
    storyboard_store::import_source(&dir, &voice, &relative).unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "source_audio": {"src": relative.to_str().unwrap(), "start": 0.0, "end": 3.0}
    }))
    .unwrap();
    storyboard_store::commit(
        &dir,
        &version,
        vec![storyboard_core::Op::UpdateClip {
            cut: "cut-1".into(),
            clip: "clip-1".into(),
            patch,
        }],
        storyboard_core::Author::Human,
        None,
    )
    .unwrap();
    let result = json(storyboard().args(["export-movie", dir.to_str().unwrap(), "--cut", "cut-1"]));
    assert_eq!(result["frames"], 120);
    let output = Path::new(result["path"].as_str().unwrap());
    let sample = |at: &str| {
        let decoded = Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-ss", at])
            .arg("-i")
            .arg(output)
            .args(["-t", "0.2", "-vn", "-ac", "1", "-f", "s16le", "pipe:1"])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        decoded
            .stdout
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]).unsigned_abs())
            .max()
            .unwrap_or_default()
    };
    assert!(
        sample("2.5") > 500,
        "placement VO should play after source shot time"
    );
    assert!(sample("3.5") < 100, "the next placement should be silent");
}

#[test]
fn cli_exports_more_than_256_cut_segments_without_dropping_frames() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || Command::new("ffprobe").arg("-version").output().is_err()
    {
        return;
    }
    let workspace = TempDir::new().unwrap();
    let dir = workspace.path().join("project");
    let created = json(storyboard().args([
        "new",
        dir.to_str().unwrap(),
        "--title",
        "Long cut",
        "--width",
        "320",
        "--height",
        "180",
    ]));
    let version = add_opening_shot(&dir, created["version"].as_str().unwrap());
    for (index, color) in [[255, 0, 0, 255], [0, 255, 0, 255]].into_iter().enumerate() {
        let frame = workspace.path().join(format!("frame-{}.png", index + 1));
        ImageBuffer::<Rgba<u8>, _>::from_pixel(320, 180, Rgba(color))
            .save(frame)
            .unwrap();
    }
    let movie = workspace.path().join("two-frames.mp4");
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            "24",
            "-i",
        ])
        .arg(workspace.path().join("frame-%d.png"))
        .args([
            "-frames:v",
            "2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&movie)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let replaced = json(storyboard().args([
        "replace-movie",
        dir.to_str().unwrap(),
        "--base-version",
        &version,
        "--author",
        "human",
        "--shot",
        "shot-1",
        "--source",
        movie.to_str().unwrap(),
    ]));
    let version = write_version(
        &dir,
        replaced["version"].as_str().unwrap(),
        &["add-cut", "--name", "Many segments"],
    );
    let voice = workspace.path().join("voice.wav");
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2",
            "-y",
        ])
        .arg(&voice)
        .output()
        .unwrap();
    assert!(generated.status.success());
    let relative = std::path::PathBuf::from("sources/imported-audio")
        .join(uuid::Uuid::new_v4().to_string())
        .join("original.wav");
    storyboard_store::import_source(&dir, &voice, &relative).unwrap();
    let mut ops = vec![serde_json::from_value(serde_json::json!({
        "update_shot": {"shot":"shot-1", "patch":{"duration":2.0, "source_audio":{"src":relative.to_str().unwrap(), "start":0.0, "end":2.0}}}
    })).unwrap()];
    for index in 0..257 {
        let id = format!("clip-many-{index}");
        ops.push(
            serde_json::from_value(serde_json::json!({
                "add_clip": {"cut":"cut-1", "id":id, "source":{"shot":{"shot":"shot-1"}}}
            }))
            .unwrap(),
        );
        ops.push(
            serde_json::from_value(serde_json::json!({
                "update_clip": {"cut":"cut-1", "clip":id, "patch":{"in":index % 2, "out":index % 2 + 1}}
            }))
            .unwrap(),
        );
    }
    storyboard_store::commit(
        &dir,
        &version,
        vec![storyboard_core::Op::Batch { ops }],
        storyboard_core::Author::Human,
        None,
    )
    .unwrap();
    let before = fs::read(dir.join("project.json")).unwrap();
    let exported =
        json(storyboard().args(["export-movie", dir.to_str().unwrap(), "--cut", "cut-1"]));
    assert_eq!(exported["frames"], 257);
    assert_eq!(fs::read(dir.join("project.json")).unwrap(), before);
    let path = Path::new(exported["path"].as_str().unwrap());
    let count = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(count.status.success());
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "257");
    let audio = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&audio.stdout).trim(), "aac");
    let pixels = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(path)
        .args([
            "-vf",
            "scale=1:1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(pixels.status.success());
    assert_eq!(pixels.stdout.len(), 257 * 3);
    for frame in [31, 32, 63, 64, 255, 256] {
        let bytes = &pixels.stdout[frame * 3..frame * 3 + 3];
        if frame % 2 == 0 {
            assert!(bytes[0] > 100 && bytes[1] < 100, "frame {frame}: {bytes:?}");
        } else {
            assert!(bytes[1] > 100 && bytes[0] < 100, "frame {frame}: {bytes:?}");
        }
    }
    for time in ["1.35", "2.69", "9.35"] {
        let samples = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                time,
            ])
            .arg("-i")
            .arg(path)
            .args(["-t", "0.05", "-vn", "-ac", "1", "-f", "s16le", "pipe:1"])
            .output()
            .unwrap();
        assert!(samples.status.success());
        assert!(
            samples
                .stdout
                .chunks_exact(2)
                .any(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]).unsigned_abs() > 500),
            "audio missing after a chunk boundary at {time}s"
        );
    }
}
