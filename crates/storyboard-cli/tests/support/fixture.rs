use std::path::PathBuf;

use storyboard_core::{Author, Format, Op};
use storyboard_store::{commit, create};
use tempfile::TempDir;

pub fn thirty_scene_project() -> (TempDir, PathBuf, String) {
    let workspace = TempDir::new().expect("temporary workspace");
    let project = workspace.path().join("thirty-scenes");
    let created = create(
        &project,
        "Thirty scenes",
        24,
        Format {
            width: 1440,
            height: 900,
            extra: Default::default(),
        },
    )
    .expect("create fixture project");
    let mut ops = Vec::new();
    for number in 1..=30 {
        ops.push(Op::AddScene {
            id: Some(format!("scene-{number}")),
            title: format!("Scene {number}"),
            index: None,
        });
        ops.push(Op::AddShot {
            scene: format!("scene-{number}"),
            id: Some(format!("shot-{number}")),
            title: format!("Shot {number}"),
            index: None,
        });
    }
    let seeded = commit(&project, &created.version, ops, Author::Human, None)
        .expect("seed 30-scene fixture");
    (workspace, project, seeded.version)
}
