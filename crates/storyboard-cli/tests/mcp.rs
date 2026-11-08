#[path = "support/mcp.rs"]
mod support;

use std::{collections::BTreeSet, fs};

use clap::{CommandFactory, ValueHint};
use serde_json::{Value, json};
use storyboard_cli::args::Cli;
use tempfile::TempDir;

use support::{Mcp, create_project, error, result};

#[test]
fn mcp_tools_list_mirrors_every_cli_verb() {
    let (workspace, _project, _version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let listed = mcp.request("tools/list", json!({}));
    let names: BTreeSet<_> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    let expected: BTreeSet<_> = Cli::command()
        .get_subcommands()
        .map(|command| command.get_name())
        .filter(|name| *name != "mcp")
        .map(|name| name.replace('-', "_"))
        .collect();
    let mirrored: BTreeSet<_> = names
        .into_iter()
        .filter(|name| *name != "apply")
        .map(str::to_owned)
        .collect();
    assert_eq!(mirrored, expected);
}

#[test]
fn mcp_every_tool_schema_matches_its_cli_arguments() {
    let (workspace, _project, _version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let listed = mcp.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let cli = Cli::command();
    for command in cli
        .get_subcommands()
        .filter(|command| command.get_name() != "mcp")
    {
        let tool_name = command.get_name().replace('-', "_");
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == tool_name)
            .unwrap_or_else(|| panic!("missing tool {tool_name}"));
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{tool_name} properties"));
        assert!(!properties.is_empty(), "{tool_name} has no properties");
        let actual: BTreeSet<_> = properties.keys().cloned().collect();
        let mut expected = BTreeSet::new();
        let mut required = BTreeSet::new();
        let is_write = command.get_name() != "notes"
            && command
                .get_arguments()
                .any(|argument| argument.get_id().as_str() == "author");
        for argument in command.get_arguments() {
            let id = argument.get_id().as_str();
            if id == "author" || id == "pretty" {
                continue;
            }
            let name = if id == "dir" { "project" } else { id };
            expected.insert(name.to_owned());
            if argument.is_required_set() {
                required.insert(name.to_owned());
            }
            let property = &properties[name];
            assert!(
                property.get("type").is_some()
                    || property.get("oneOf").is_some()
                    || property.get("anyOf").is_some()
                    || property.get("$ref").is_some(),
                "{tool_name}.{name} has no type: {property}"
            );
            assert!(
                property["description"]
                    .as_str()
                    .is_some_and(|description| !description.is_empty()),
                "{tool_name}.{name} has no description"
            );
            if argument.get_value_hint() == ValueHint::DirPath && name == "project" {
                assert_eq!(property["type"], "string");
            }
        }
        if is_write {
            expected.insert("full".into());
        }
        assert_eq!(actual, expected, "{tool_name} property drift");
        let actual_required: BTreeSet<_> = tool["inputSchema"]["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{tool_name} required"))
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        assert_eq!(actual_required, required, "{tool_name} required drift");
    }
    let trim = tools
        .iter()
        .find(|tool| tool["name"] == "trim")
        .expect("trim tool");
    assert!(
        trim["inputSchema"]["properties"]["in"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("frames"))
    );
    let apply = tools
        .iter()
        .find(|tool| tool["name"] == "apply")
        .expect("apply tool");
    assert!(apply["inputSchema"]["properties"].is_object());
    assert!(apply["inputSchema"]["required"].is_array());
}

#[test]
fn mcp_internal_ops_are_hidden_and_refused() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let schema = result(&mcp.tool("schema", json!({})));
    assert!(schema.to_string().contains("add_scene"));
    assert!(!schema.to_string().contains("restore_changes"));
    let rejected = error(&mcp.tool(
        "apply",
        json!({"project": project, "base_version": version, "ops": [{"restore_changes": {"changes": []}}]}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .expect("message")
            .contains("internal")
    );
}

#[test]
fn mcp_serves_working_agreement() {
    let (workspace, _project, _version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let response = mcp.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "Codex", "version": "test"}}),
    );
    let instructions = response["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(instructions.contains("Read"));
    assert!(instructions.contains("base_version"));
    assert!(instructions.contains("stick"));
}

#[test]
fn mcp_panel_image_returns_image_block_and_refuses_project_json_output() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let version = scene["version"].as_str().expect("version");
    let shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": version, "scene": "scene-1", "title": "Arrival"})));
    let version = shot["version"].as_str().expect("version");
    let rendered = mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "width": 64, "mode": "stage"}),
    );
    assert_eq!(
        rendered.pointer("/result/content/0/type"),
        Some(&json!("image"))
    );
    assert_eq!(
        rendered.pointer("/result/content/1/type"),
        Some(&json!("text"))
    );
    let project_json = project.join("project.json");
    let rejected = error(&mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "out": project_json, "width": 64, "mode": "stage"}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(
        fs::read_to_string(project.join("project.json"))
            .expect("project")
            .contains("nb-storyboard/v1")
    );
    assert!(version.len() > 10);
}

#[test]
fn mcp_writes_are_compact_and_default_author_from_client() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let write = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    assert_eq!(write["ids"], json!(["scene-1"]));
    assert!(write["version"].is_string());
    assert!(write["summary"].is_string());
    assert!(write.get("project").is_none());
    let note = result(&mcp.tool(
        "note",
        json!({"project": project, "base_version": write["version"], "text": "Check the door"}),
    ));
    assert_eq!(note["ids"], json!(["sticky-1"]));
    let shown = result(&mcp.tool("show", json!({"project": project})));
    assert_eq!(shown["stickies"][0]["author"]["agent"]["name"], "codex");
}

#[test]
fn mcp_layer_schema_is_exposed_and_errors_have_contract_fields() {
    let (workspace, project, _version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let schema = result(&mcp.tool("schema", json!({"project": project})));
    assert_eq!(schema["layers"]["image"]["required"], json!(["src"]));
    assert_eq!(
        schema["layers"]["text"]["required"],
        json!(["text", "font_size", "color"])
    );
    assert_eq!(schema["layers"]["solid"]["required"], json!(["fill"]));
    assert_eq!(schema["layers"]["box"]["required"], json!([]));
    assert_eq!(
        schema["layers"]["image"]["properties"]["src"]["type"],
        "string"
    );
    assert_eq!(
        schema["layers"]["text"]["properties"]["text"]["type"],
        "string"
    );
    assert_eq!(
        schema["layers"]["text"]["properties"]["font_size"]["type"],
        "number"
    );
    assert_eq!(
        schema["layers"]["text"]["properties"]["color"]["type"],
        "string"
    );
    assert!(schema["layers"]["text"]["properties"]["weight"]["enum"].is_array());
    assert!(schema["layers"]["text"]["properties"]["align"]["enum"].is_array());
    let rejected = error(&mcp.tool("show", json!({"project": project, "cut": "unknown"})));
    for key in ["kind", "message", "hint", "valid"] {
        assert!(
            rejected["error"].get(key).is_some(),
            "missing {key}: {rejected}"
        );
    }
}

#[test]
fn mcp_render_refuses_sibling_project_json() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let _shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let sibling = workspace.path().join("sibling");
    let created = result(&mcp.tool("new", json!({"project": "sibling", "title": "Sibling"})));
    assert!(created["version"].is_string(), "new sibling: {created}");
    let document = sibling.join("project.json");
    let before = fs::read(&document).expect("sibling project");
    let rejected = error(&mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "out": document, "width": 64}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert_eq!(
        fs::read(sibling.join("project.json")).expect("sibling project"),
        before
    );
}

#[cfg(unix)]
#[test]
fn mcp_render_replaces_hardlink_without_touching_sibling() {
    use std::os::unix::fs::MetadataExt;

    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let _shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let sibling = workspace.path().join("sibling");
    result(&mcp.tool("new", json!({"project": "sibling", "title": "Sibling"})));
    let sibling_document = sibling.join("project.json");
    let before = fs::read(&sibling_document).expect("sibling project");
    let output = workspace.path().join("hl.png");
    fs::hard_link(&sibling_document, &output).expect("hardlink output to sibling document");

    let rendered = mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "out": "hl.png", "width": 64}),
    );

    assert_eq!(
        rendered.pointer("/result/content/0/type"),
        Some(&json!("image"))
    );
    assert!(
        fs::read(&sibling_document).expect("sibling project after render") == before,
        "render changed sibling project.json through the output hardlink"
    );
    assert_eq!(
        &fs::read(&output).expect("PNG output")[..8],
        b"\x89PNG\r\n\x1a\n"
    );
    assert_eq!(fs::metadata(&output).expect("output metadata").nlink(), 1);
}

#[test]
fn mcp_read_only_tools_do_not_advertise_full() {
    let (workspace, _project, _version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let listed = mcp.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().expect("tools");
    for name in [
        "sources",
        "show",
        "panel_image",
        "cut_sheet",
        "notes",
        "schema",
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        assert!(
            tool["inputSchema"]["properties"].get("full").is_none(),
            "read-only tool {name} advertises full"
        );
    }
}

#[test]
fn mcp_render_refuses_outside_workspace() {
    let (workspace, project, version) = create_project();
    let outside = TempDir::new().expect("outside");
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let _shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let out = outside.path().join("escape.png");
    let rejected = error(&mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "out": out, "width": 64}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(!outside.path().join("escape.png").exists());
}

#[cfg(unix)]
#[test]
fn mcp_render_refuses_symlink_out() {
    use std::os::unix::fs::symlink;

    let (workspace, project, version) = create_project();
    let outside = TempDir::new().expect("outside");
    symlink(outside.path(), workspace.path().join("escape")).expect("workspace symlink");
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let _shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let rejected = error(&mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "out": "escape/render.png", "width": 64}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(!outside.path().join("render.png").exists());
}

#[test]
fn mcp_relative_out_resolves_in_workspace() {
    let root = TempDir::new().expect("root");
    let workspace = root.path().join("workspace");
    let server_cwd = root.path().join("server-cwd");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&server_cwd).expect("server cwd");
    let mut mcp = Mcp::start_in(&workspace, "Codex", Some(&server_cwd));
    let created = result(&mcp.tool("new", json!({"project": "story", "title": "Story"})));
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": "story", "base_version": created["version"], "title": "Opening"}),
    ));
    let _shot = result(&mcp.tool("add_shot", json!({"project": "story", "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let rendered = mcp.tool(
        "panel_image",
        json!({"project": "story", "at": "1", "out": "render.png", "width": 64}),
    );
    assert_eq!(
        rendered.pointer("/result/content/0/type"),
        Some(&json!("image"))
    );
    assert!(workspace.join("render.png").is_file());
    assert!(!server_cwd.join("render.png").exists());
}

#[test]
fn mcp_new_refuses_parent_segments() {
    let root = TempDir::new().expect("root");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let mut mcp = Mcp::start(&workspace, "Codex");
    let rejected = error(&mcp.tool(
        "new",
        json!({"project": "../outside/evil", "title": "Escape"}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(!root.path().join("outside/evil/project.json").exists());
}

#[cfg(unix)]
#[test]
fn mcp_new_refuses_symlinked_parent() {
    use std::os::unix::fs::symlink;

    let root = TempDir::new().expect("root");
    let workspace = root.path().join("workspace");
    let outside = root.path().join("outside");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&outside).expect("outside");
    symlink(&outside, workspace.join("escape")).expect("workspace symlink");
    let mut mcp = Mcp::start(&workspace, "Codex");
    let rejected = error(&mcp.tool("new", json!({"project": "escape/evil", "title": "Escape"})));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert!(!outside.join("evil/project.json").exists());
}

#[test]
fn mcp_add_panel_accepts_explicit_shot_and_take() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let panel = result(&mcp.tool("add_panel", json!({"project": project, "base_version": shot["version"], "shot": "shot-1", "take": "take-1"})));
    let shown = result(&mcp.tool("show", json!({"project": project})));
    assert_eq!(shown["scenes"][0]["shots"][0]["takes"][0]["panels"], 2);
    assert_eq!(panel["ids"], json!(["panel-2"]));
}

#[test]
fn mcp_full_write_returns_full_project() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let write = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening", "full": true}),
    ));
    assert_eq!(write["project"]["title"], "MCP story");
    assert_eq!(write["project"]["scenes"][0]["title"], "Opening");
    assert_eq!(write["version"].as_str().map(str::len), Some(64));
}

#[test]
fn mcp_apply_reports_every_created_id() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let write = result(&mcp.tool(
        "apply",
        json!({
            "project": project,
            "base_version": version,
            "ops": [
                {"add_scene": {"id": null, "title": "One", "index": null}},
                {"add_scene": {"id": null, "title": "Two", "index": null}}
            ]
        }),
    ));
    assert_eq!(write["ids"], json!(["scene-1", "scene-2"]));
}

#[test]
fn mcp_ping_and_unknown_methods_use_json_rpc_results() {
    let (workspace, _project, _version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let ping = mcp.request("ping", json!({}));
    assert_eq!(ping["result"], json!({}));
    assert!(ping.get("error").is_none());
    let unknown = mcp.request("storyboard/nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    assert!(unknown.get("result").is_none());
}

#[test]
fn mcp_forged_sticky_author_is_refused() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let rejected = error(&mcp.tool(
        "apply",
        json!({
            "project": project,
            "base_version": version,
            "ops": [{"add_sticky": {"id": null, "text": "forged", "kind": "note", "author": "human", "pin": "none", "created": "2026-11-07T00:00:00Z"}}]
        }),
    ));
    assert_eq!(rejected["error"]["kind"], "authorship");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .expect("message")
            .contains("own author")
    );
}
