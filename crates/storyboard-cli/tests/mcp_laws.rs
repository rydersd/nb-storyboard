#[path = "support/mcp.rs"]
mod support;

use serde_json::json;

use support::{Mcp, create_project, error, result};

#[test]
fn law_containment_mcp_render_never_overwrites_project_document() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let scene = result(&mcp.tool(
        "add_scene",
        json!({"project": project, "base_version": version, "title": "Opening"}),
    ));
    let shot = result(&mcp.tool("add_shot", json!({"project": project, "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})));
    let original = std::fs::read(project.join("project.json")).expect("project bytes");
    let rejected = error(&mcp.tool("panel_image", json!({"project": project, "at": "1", "out": project.join("project.json"), "width": 64, "mode": "stage"})));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert_eq!(
        std::fs::read(project.join("project.json")).expect("project bytes"),
        original
    );
    let sibling = workspace.path().join("sibling");
    let created = result(&mcp.tool("new", json!({"project": "sibling", "title": "Sibling"})));
    assert!(created["version"].is_string(), "new sibling: {created}");
    let sibling_document = sibling.join("project.json");
    let sibling_original = std::fs::read(&sibling_document).expect("sibling bytes");
    let rejected = error(&mcp.tool(
        "panel_image",
        json!({"project": project, "at": "1", "out": sibling_document, "width": 64}),
    ));
    assert_eq!(rejected["error"]["kind"], "validation");
    assert_eq!(
        std::fs::read(sibling.join("project.json")).expect("sibling bytes"),
        sibling_original
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = tempfile::TempDir::new().expect("outside");
        symlink(outside.path(), workspace.path().join("escape")).expect("workspace symlink");
        let rejected = error(&mcp.tool(
            "panel_image",
            json!({"project": project, "at": "1", "out": "escape/render.png", "width": 64}),
        ));
        assert_eq!(rejected["error"]["kind"], "validation");
        assert!(!outside.path().join("render.png").exists());
    }
    assert!(shot["version"].is_string());
}

#[test]
fn law_honest_authorship_mcp_refuses_foreign_sticky_author() {
    let (workspace, project, version) = create_project();
    let mut mcp = Mcp::start(workspace.path(), "Codex");
    let created = result(&mcp.tool(
        "note",
        json!({"project": project, "base_version": version, "text": "real"}),
    ));
    let rejected = error(&mcp.tool("apply", json!({
        "project": project,
        "base_version": created["version"],
        "ops": [{"add_sticky": {"id": null, "text": "forged", "kind": "note", "author": "human", "pin": "none", "created": "2026-11-07T00:00:00Z"}}]
    })));
    assert_eq!(rejected["error"]["kind"], "authorship");
    let shown = result(&mcp.tool("show", json!({"project": project})));
    assert_eq!(shown["stickies"].as_array().expect("stickies").len(), 1);
    assert_eq!(shown["stickies"][0]["author"]["agent"]["name"], "codex");
}
