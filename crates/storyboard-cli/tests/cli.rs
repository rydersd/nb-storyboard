use std::{fs, path::Path, process::Command};

use assert_cmd::prelude::*;
use clap::{Command as ClapCommand, CommandFactory};
use image::{ImageBuffer, Rgba};
use serde_json::Value;
use storyboard_cli::args::Cli;
use tempfile::TempDir;

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
            .contains("add-scene")
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
            .contains("add-shot")
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
            .contains("add-image")
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
            .contains("select-take")
    );
    version = select["version"].as_str().expect("version").into();
    let note = write(&dir, &version, &["note", "--text", "Check doorway"]);
    assert!(note["summary"].as_str().expect("summary").contains("note"));
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
            .contains("reply")
    );
    version = reply["version"].as_str().expect("version").into();
    let resolve = write(&dir, &version, &["resolve", "--sticky", &sticky]);
    assert!(
        resolve["summary"]
            .as_str()
            .expect("summary")
            .contains("resolve")
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
            .contains("move-shot")
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
