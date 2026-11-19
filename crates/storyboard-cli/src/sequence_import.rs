//! S21 numbered images: source ordering and one validated batch for the new shot.

use std::{cmp::Ordering, collections::HashSet, path::Path};

use serde_json::{Map, Value, json};
use storyboard_core::{Op, Project};

use crate::ops_api::{self, ApiError};

pub fn image_name(name: &str) -> bool {
    let path = Path::new(name);
    path.file_name().is_some_and(|part| part == name)
        && !name.starts_with('.')
        && !name.chars().any(char::is_control)
        && matches!(
            path.extension()
                .and_then(|part| part.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "tif" | "tiff" | "svg")
        )
}

pub fn sort_images<T>(files: &mut [(String, T)]) -> Result<(), ApiError> {
    if files.is_empty() || files.len() > 500 {
        return Err(ops_api::validation("select 1 to 500 image files"));
    }
    let mut names = HashSet::new();
    for (name, _) in files.iter() {
        if !image_name(name) {
            return Err(ops_api::validation(format!(
                "unsupported image filename {name:?}"
            )));
        }
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(ops_api::validation(format!(
                "duplicate image name {name:?}"
            )));
        }
    }
    files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    Ok(())
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let originals = (a, b);
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    while !a.is_empty() && !b.is_empty() {
        if a[0].is_ascii_digit() != b[0].is_ascii_digit() {
            return a[0].to_ascii_lowercase().cmp(&b[0].to_ascii_lowercase());
        }
        let digits = a[0].is_ascii_digit() && b[0].is_ascii_digit();
        let end_a = a
            .iter()
            .position(|byte| byte.is_ascii_digit() != digits)
            .unwrap_or(a.len());
        let end_b = b
            .iter()
            .position(|byte| byte.is_ascii_digit() != digits)
            .unwrap_or(b.len());
        let (part_a, rest_a) = a.split_at(end_a);
        let (part_b, rest_b) = b.split_at(end_b);
        let order = if digits {
            let a_number = part_a
                .iter()
                .skip_while(|byte| **byte == b'0')
                .copied()
                .collect::<Vec<_>>();
            let b_number = part_b
                .iter()
                .skip_while(|byte| **byte == b'0')
                .copied()
                .collect::<Vec<_>>();
            a_number
                .len()
                .cmp(&b_number.len())
                .then_with(|| a_number.cmp(&b_number))
        } else {
            part_a
                .to_ascii_lowercase()
                .cmp(&part_b.to_ascii_lowercase())
        };
        if order != Ordering::Equal {
            return order;
        }
        a = rest_a;
        b = rest_b;
    }
    a.len()
        .cmp(&b.len())
        .then_with(|| {
            originals
                .0
                .to_ascii_lowercase()
                .cmp(&originals.1.to_ascii_lowercase())
        })
        .then_with(|| originals.0.cmp(originals.1))
}

fn first_free(used: &mut HashSet<String>, kind: &str) -> String {
    for number in 1.. {
        let candidate = format!("{kind}-{number}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

pub fn import_ops(
    project: &Project,
    scene: &str,
    title: &str,
    duration: Option<f64>,
    index: Option<usize>,
    files: &[(String, String)],
) -> Result<Vec<Op>, ApiError> {
    if !project.scenes.iter().any(|item| item.id == scene) {
        return Err(ops_api::validation(format!("unknown scene {scene}")));
    }
    if files.is_empty() || files.len() > 500 {
        return Err(ops_api::validation("select 1 to 500 image files"));
    }
    let duration = duration.unwrap_or(files.len() as f64);
    if !duration.is_finite() || duration <= 0.0 || duration > 3600.0 {
        return Err(ops_api::validation(
            "duration must be above 0 and at most 3600 seconds",
        ));
    }
    let frames = (duration * f64::from(project.fps)).round() as usize;
    if frames < files.len() {
        return Err(ops_api::validation(format!(
            "duration needs at least {} frames for {} images",
            files.len(),
            files.len()
        )));
    }
    let mut shots: HashSet<_> = project
        .scenes
        .iter()
        .flat_map(|s| &s.shots)
        .map(|s| s.id.clone())
        .collect();
    let mut takes: HashSet<_> = project
        .scenes
        .iter()
        .flat_map(|s| &s.shots)
        .flat_map(|s| &s.takes)
        .map(|t| t.id.clone())
        .collect();
    let mut panels: HashSet<_> = project
        .scenes
        .iter()
        .flat_map(|s| &s.shots)
        .flat_map(|s| &s.takes)
        .flat_map(|t| &t.panels)
        .map(|p| p.id.clone())
        .collect();
    let mut layers: HashSet<_> = project
        .scenes
        .iter()
        .flat_map(|s| &s.shots)
        .flat_map(|s| &s.takes)
        .flat_map(|t| &t.panels)
        .flat_map(|p| &p.layers)
        .map(|l| l.id.clone())
        .collect();
    let shot = first_free(&mut shots, "shot");
    let take = first_free(&mut takes, "take");
    let first_panel = first_free(&mut panels, "panel");
    let mut ops = vec![Op::AddShot {
        scene: scene.into(),
        id: Some(shot.clone()),
        title: title.into(),
        index,
    }];
    let mut patch = Map::new();
    patch.insert("duration".into(), json!(duration));
    ops.push(Op::UpdateShot {
        shot: shot.clone(),
        patch,
    });
    for (number, (_, source)) in files.iter().enumerate() {
        let panel = if number == 0 {
            first_panel.clone()
        } else {
            let panel = first_free(&mut panels, "panel");
            ops.push(Op::AddPanel {
                shot: shot.clone(),
                take: take.clone(),
                id: Some(panel.clone()),
                time: Some((number * frames / files.len()) as f64 / f64::from(project.fps)),
                index: None,
            });
            panel
        };
        let mut fields = Map::<String, Value>::new();
        fields.insert("src".into(), json!(source));
        fields.insert("fit".into(), json!("contain"));
        ops.push(Op::AddLayer {
            shot: shot.clone(),
            take: take.clone(),
            panel,
            id: Some(first_free(&mut layers, "layer")),
            layer_type: "image".into(),
            fields,
            index: None,
        });
    }
    Ok(vec![Op::Batch { ops }])
}
