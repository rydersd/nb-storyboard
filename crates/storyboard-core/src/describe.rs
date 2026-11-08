//! Readable, deterministic summaries of storyboard data.

use crate::{Compiled, Project};

/// Describes the project without exposing its serialized representation.
pub fn describe_project(project: &Project) -> String {
    let scenes = project.scenes.len();
    let shots = project
        .scenes
        .iter()
        .map(|scene| scene.shots.len())
        .sum::<usize>();
    format!(
        "{}: {scenes} scene{}, {shots} shot{}, {} fps, {}×{}",
        project.title,
        if scenes == 1 { "" } else { "s" },
        if shots == 1 { "" } else { "s" },
        project.fps,
        project.format.width,
        project.format.height
    )
}

/// Describes compiled sequence items in playback order.
pub fn describe_sequence(sequence: &Compiled) -> String {
    let items = sequence
        .items
        .iter()
        .map(|item| {
            format!(
                "{} ({}/{}) {}–{}",
                item.shot, item.take, item.offset, item.start, item.end
            )
        })
        .collect::<Vec<_>>();
    format!("{} frames: {}", sequence.frames, items.join(", "))
}

/// Describes one panel in its scene, shot and take context.
pub fn describe_panel(
    project: &Project,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
) -> Result<String, DescribeError> {
    for scene in &project.scenes {
        if let Some(shot) = scene.shots.iter().find(|shot| shot.id == shot_id) {
            let take = shot
                .takes
                .iter()
                .find(|take| take.id == take_id)
                .ok_or_else(|| DescribeError::Take {
                    shot: shot_id.into(),
                    take: take_id.into(),
                })?;
            let panel = take
                .panels
                .iter()
                .find(|panel| panel.id == panel_id)
                .ok_or_else(|| DescribeError::Panel {
                    shot: shot_id.into(),
                    take: take_id.into(),
                    panel: panel_id.into(),
                })?;
            let mut parts = vec![format!(
                "{} / {} / {} at {:.3}s",
                scene.title, shot.title, take.name, panel.time
            )];
            parts.push(format!("Duration: {:.3}s", shot.duration));
            if !shot.speaker.is_empty() {
                parts.push(format!("Speaker: {}", shot.speaker));
            }
            if !shot.camera.is_empty() {
                parts.push(format!("Camera: {}", shot.camera));
            }
            if !panel.title.is_empty() {
                parts.push(panel.title.clone());
            }
            if !panel.direction.is_empty() {
                parts.push(format!("Direction: {}", panel.direction));
            }
            if !shot.action.is_empty() {
                parts.push(format!("Action: {}", shot.action));
            }
            if !shot.dialogue.is_empty() {
                parts.push(format!("Dialogue: {}", shot.dialogue));
            }
            parts.push(format!("Layers ({})", panel.layers.len()));
            for layer in &panel.layers {
                let mut style = format!(
                    "{}: {} at {:.3},{:.3} {:.3}×{:.3}, opacity {:.3}",
                    layer.id,
                    layer.layer_type,
                    layer.x,
                    layer.y,
                    layer.width,
                    layer.height,
                    layer.opacity
                );
                if !layer.label.is_empty() {
                    style.push_str(&format!(", label {}", layer.label));
                }
                if let Some(fill) = &layer.fill {
                    style.push_str(&format!(", fill {fill}"));
                }
                if let Some(color) = &layer.color {
                    style.push_str(&format!(", color {color}"));
                }
                if let Some(size) = layer.font_size {
                    style.push_str(&format!(", font {size:.3}"));
                }
                if let Some(weight) = &layer.weight {
                    style.push_str(&format!(", weight {weight}"));
                }
                if let Some(align) = &layer.align {
                    style.push_str(&format!(", align {align}"));
                }
                if let Some(src) = &layer.src {
                    style.push_str(&format!(", source {src}"));
                }
                parts.push(style);
            }
            parts.push(format!("Strokes: {}", panel.strokes.len()));
            let refs = shot.refs.iter().chain(take.refs.iter()).collect::<Vec<_>>();
            if !refs.is_empty() {
                parts.push(format!(
                    "Refs: {}",
                    refs.into_iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            return Ok(parts.join("\n"));
        }
    }
    Err(DescribeError::Shot(shot_id.into()))
}

/// Returns the document-relative source files directly placed by a panel.
pub fn panel_source_paths(
    project: &Project,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
) -> Result<Vec<String>, DescribeError> {
    for scene in &project.scenes {
        if let Some(shot) = scene.shots.iter().find(|shot| shot.id == shot_id) {
            let take = shot
                .takes
                .iter()
                .find(|take| take.id == take_id)
                .ok_or_else(|| DescribeError::Take {
                    shot: shot_id.into(),
                    take: take_id.into(),
                })?;
            let panel = take
                .panels
                .iter()
                .find(|panel| panel.id == panel_id)
                .ok_or_else(|| DescribeError::Panel {
                    shot: shot_id.into(),
                    take: take_id.into(),
                    panel: panel_id.into(),
                })?;
            return Ok(panel
                .layers
                .iter()
                .filter_map(|layer| layer.src.clone())
                .collect());
        }
    }
    Err(DescribeError::Shot(shot_id.into()))
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DescribeError {
    #[error("unknown shot {0}")]
    Shot(String),
    #[error("unknown take {take} for shot {shot}")]
    Take { shot: String, take: String },
    #[error("unknown panel {panel} for {shot}/{take}")]
    Panel {
        shot: String,
        take: String,
        panel: String,
    },
}
