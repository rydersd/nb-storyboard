use std::collections::{HashMap, HashSet};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::compile::{
    MAX_CLIPS_PER_CUT, MAX_CUTS, MAX_SCENES, MAX_SHOTS, ProjectIndex, analyze_cuts_with_index,
    check_count_limits, preflight_story,
};
use crate::{Author, ClipSource, CompileError, Layer, Pin, Project, ids::is_valid_id, parse_color};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub valid: Vec<String>,
}

impl ValidationIssue {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            valid: Vec::new(),
        }
    }

    fn options(field: impl Into<String>, message: impl Into<String>, valid: &[&str]) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            valid: valid.iter().map(ToString::to_string).collect(),
        }
    }
}

/// Validates every static document rule at once. Cross-reference checks are deliberately
/// here as well, so later writers have one gate before they persist a document.
pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if project.schema != "nb-storyboard/v1" {
        issues.push(ValidationIssue::options(
            "schema",
            "unsupported schema",
            &["nb-storyboard/v1"],
        ));
    }
    if ![24, 25, 30, 60].contains(&project.fps) {
        issues.push(ValidationIssue::options(
            "fps",
            "fps must be one of the supported frame rates",
            &["24", "25", "30", "60"],
        ));
    }
    limit(&mut issues, "scenes", project.scenes.len(), MAX_SCENES);
    text(&mut issues, "title", &project.title);
    dimension(&mut issues, "format.width", project.format.width);
    dimension(&mut issues, "format.height", project.format.height);

    let mut scene_ids = HashSet::new();
    let mut shot_ids = HashSet::new();
    let mut take_ids = HashSet::new();
    let mut panel_ids = HashSet::new();
    let mut layer_ids = HashSet::new();
    let mut cut_ids = HashSet::new();
    let mut clip_ids = HashSet::new();
    let mut sticky_ids = HashSet::new();
    let mut shots_by_id = HashMap::new();

    for (scene_index, scene) in project.scenes.iter().enumerate() {
        let path = format!("scenes[{scene_index}]");
        id(
            &mut issues,
            &mut scene_ids,
            &format!("{path}.id"),
            &scene.id,
        );
        text(&mut issues, &format!("{path}.title"), &scene.title);
        for (shot_index, shot) in scene.shots.iter().enumerate() {
            let path = format!("{path}.shots[{shot_index}]");
            shots_by_id.entry(shot.id.as_str()).or_insert(shot);
            id(&mut issues, &mut shot_ids, &format!("{path}.id"), &shot.id);
            text(&mut issues, &format!("{path}.title"), &shot.title);
            text(&mut issues, &format!("{path}.action"), &shot.action);
            text(&mut issues, &format!("{path}.dialogue"), &shot.dialogue);
            text(&mut issues, &format!("{path}.speaker"), &shot.speaker);
            text(&mut issues, &format!("{path}.camera"), &shot.camera);
            text(&mut issues, &format!("{path}.notes"), &shot.notes);
            if !shot.duration.is_finite() || !(0.0..=3600.0).contains(&shot.duration) {
                issues.push(ValidationIssue::new(
                    format!("{path}.duration"),
                    "duration must be from 0 to 3600 seconds",
                ));
            }
            if !["draft", "review", "approved", "changes"].contains(&shot.status.as_str()) {
                issues.push(ValidationIssue::options(
                    format!("{path}.status"),
                    "invalid shot status",
                    &["draft", "review", "approved", "changes"],
                ));
            }
            limit(&mut issues, format!("{path}.takes"), shot.takes.len(), 100);
            paths(&mut issues, &format!("{path}.refs"), &shot.refs);
            for (take_index, take) in shot.takes.iter().enumerate() {
                let path = format!("{path}.takes[{take_index}]");
                id(&mut issues, &mut take_ids, &format!("{path}.id"), &take.id);
                text(&mut issues, &format!("{path}.name"), &take.name);
                paths(&mut issues, &format!("{path}.refs"), &take.refs);
                for (panel_index, panel) in take.panels.iter().enumerate() {
                    let path = format!("{path}.panels[{panel_index}]");
                    id(
                        &mut issues,
                        &mut panel_ids,
                        &format!("{path}.id"),
                        &panel.id,
                    );
                    text(&mut issues, &format!("{path}.title"), &panel.title);
                    text(&mut issues, &format!("{path}.direction"), &panel.direction);
                    color(
                        &mut issues,
                        &format!("{path}.background"),
                        &panel.background,
                    );
                    if !panel.time.is_finite() || panel.time < 0.0 || panel.time >= shot.duration {
                        issues.push(ValidationIssue::new(
                            format!("{path}.time"),
                            "panel time must be within the shot duration",
                        ));
                    }
                    for (layer_index, layer) in panel.layers.iter().enumerate() {
                        let path = format!("{path}.layers[{layer_index}]");
                        id(
                            &mut issues,
                            &mut layer_ids,
                            &format!("{path}.id"),
                            &layer.id,
                        );
                        validate_layer(&mut issues, &path, layer);
                    }
                    for (stroke_index, stroke) in panel.strokes.iter().enumerate() {
                        let path = format!("{path}.strokes[{stroke_index}]");
                        required_color(&mut issues, &format!("{path}.color"), &stroke.color);
                        geometry(&mut issues, &format!("{path}.width"), stroke.width);
                        for (point_index, point) in stroke.points.iter().enumerate() {
                            geometry(
                                &mut issues,
                                &format!("{path}.points[{point_index}][0]"),
                                point[0],
                            );
                            geometry(
                                &mut issues,
                                &format!("{path}.points[{point_index}][1]"),
                                point[1],
                            );
                        }
                    }
                }
            }
        }
    }

    limit(
        &mut issues,
        "shots",
        project.scenes.iter().map(|scene| scene.shots.len()).sum(),
        MAX_SHOTS,
    );
    for (scene_index, scene) in project.scenes.iter().enumerate() {
        for (shot_index, shot) in scene.shots.iter().enumerate() {
            if shot.selected_take.is_empty()
                || !shot.takes.iter().any(|take| take.id == shot.selected_take)
            {
                let valid: HashSet<String> =
                    shot.takes.iter().map(|take| take.id.clone()).collect();
                reference(
                    &mut issues,
                    format!("scenes[{scene_index}].shots[{shot_index}].selected_take"),
                    "selected take must be a non-empty take belonging to this shot",
                    &valid,
                );
            }
        }
    }

    limit(&mut issues, "cuts", project.cuts.len(), MAX_CUTS);
    let cuts_by_id: HashMap<_, _> = project
        .cuts
        .iter()
        .map(|cut| (cut.id.as_str(), cut))
        .collect();
    for (cut_index, cut) in project.cuts.iter().enumerate() {
        let path = format!("cuts[{cut_index}]");
        id(&mut issues, &mut cut_ids, &format!("{path}.id"), &cut.id);
        text(&mut issues, &format!("{path}.name"), &cut.name);
        limit(
            &mut issues,
            format!("{path}.clips"),
            cut.clips.len(),
            MAX_CLIPS_PER_CUT,
        );
        for (clip_index, clip) in cut.clips.iter().enumerate() {
            let path = format!("{path}.clips[{clip_index}]");
            id(&mut issues, &mut clip_ids, &format!("{path}.id"), &clip.id);
            if let Some(out_frame) = clip.out_frame
                && clip.in_frame > out_frame
            {
                issues.push(ValidationIssue::new(
                    format!("{path}.in"),
                    "in must be less than or equal to out",
                ));
            }
            if clip.slot.is_some_and(|slot| slot == 0) {
                issues.push(ValidationIssue::new(
                    format!("{path}.slot"),
                    "slot must be greater than zero",
                ));
            }
            match &clip.source {
                ClipSource::Shot(source) => {
                    if !shot_ids.contains(&source.shot) {
                        reference(
                            &mut issues,
                            format!("{path}.source.shot"),
                            "unknown shot",
                            &shot_ids,
                        );
                    }
                    if let Some(take) = &source.take {
                        let belongs = shots_by_id.get(source.shot.as_str()).is_some_and(|shot| {
                            shot.takes.iter().any(|candidate| candidate.id == *take)
                        });
                        if !belongs {
                            let valid: HashSet<String> = shots_by_id
                                .get(source.shot.as_str())
                                .map(|shot| shot.takes.iter().map(|take| take.id.clone()).collect())
                                .unwrap_or_default();
                            reference(
                                &mut issues,
                                format!("{path}.source.shot.take"),
                                "take does not belong to shot",
                                &valid,
                            );
                        }
                    }
                }
                ClipSource::Cut(source) if !cuts_by_id.contains_key(source.cut.as_str()) => {
                    reference(
                        &mut issues,
                        format!("{path}.source.cut"),
                        "unknown cut",
                        &project.cuts.iter().map(|cut| cut.id.clone()).collect(),
                    );
                }
                ClipSource::Cut(_) => {}
            }
        }
    }
    if let Some(active_cut) = &project.active_cut
        && !cuts_by_id.contains_key(active_cut.as_str())
    {
        reference(
            &mut issues,
            "active_cut",
            "unknown cut",
            &project.cuts.iter().map(|cut| cut.id.clone()).collect(),
        );
    }

    for (sticky_index, sticky) in project.stickies.iter().enumerate() {
        let path = format!("stickies[{sticky_index}]");
        id(
            &mut issues,
            &mut sticky_ids,
            &format!("{path}.id"),
            &sticky.id,
        );
        text(&mut issues, &format!("{path}.text"), &sticky.text);
        if !["note", "todo", "question"].contains(&sticky.kind.as_str()) {
            issues.push(ValidationIssue::options(
                format!("{path}.kind"),
                "invalid sticky kind",
                &["note", "todo", "question"],
            ));
        }
        if let Author::Agent { agent, .. } = &sticky.author
            && agent.name.len() > 200
        {
            issues.push(ValidationIssue::new(
                format!("{path}.author.agent.name"),
                "author name may not exceed 200 bytes",
            ));
        }
        if OffsetDateTime::parse(&sticky.created, &Rfc3339).is_err()
            || !(sticky.created.ends_with('Z') || sticky.created.ends_with("+00:00"))
        {
            issues.push(ValidationIssue::new(
                format!("{path}.created"),
                "created must be an RFC 3339 UTC timestamp",
            ));
        }
        validate_pin(
            &mut issues,
            &format!("{path}.pin"),
            &sticky.pin,
            &scene_ids,
            &shot_ids,
            &cut_ids,
            &shots_by_id,
            &cuts_by_id,
        );
        if let Some(reply_to) = &sticky.reply_to
            && !project.stickies.iter().any(|sticky| &sticky.id == reply_to)
        {
            reference(
                &mut issues,
                format!("{path}.reply_to"),
                "unknown sticky",
                &project
                    .stickies
                    .iter()
                    .map(|sticky| sticky.id.clone())
                    .collect(),
            );
        }
    }
    for (id, position) in &project.layout.positions {
        let valid_key = id
            .split_once(':')
            .is_some_and(|(kind, id)| matches!(kind, "shot" | "sticky" | "cut") && is_valid_id(id));
        if !valid_key {
            issues.push(ValidationIssue::new(
                format!("layout.positions.{id}"),
                "position key must be shot:ID, sticky:ID, or cut:ID",
            ));
        }
        position_coordinate(&mut issues, &format!("layout.positions.{id}.x"), position.x);
        position_coordinate(&mut issues, &format!("layout.positions.{id}.y"), position.y);
    }
    validate_compile_limits(project, &mut issues);
    issues
}

fn validate_compile_limits(project: &Project, issues: &mut Vec<ValidationIssue>) {
    if check_count_limits(project).is_err() {
        return;
    }
    let mut issue_keys: HashSet<(String, String)> = issues
        .iter()
        .map(|issue| (issue.field.clone(), issue.message.clone()))
        .collect();
    compile_limit_issue(issues, &mut issue_keys, "story", preflight_story(project));
    let index = ProjectIndex::new(project);
    let analysis = analyze_cuts_with_index(project, &index);
    for (cut_index, error) in analysis.validation_errors() {
        compile_limit_issue(
            issues,
            &mut issue_keys,
            &format!("cuts[{cut_index}]"),
            Err::<(), _>(error),
        );
    }
}

fn compile_limit_issue<T>(
    issues: &mut Vec<ValidationIssue>,
    issue_keys: &mut HashSet<(String, String)>,
    field: &str,
    result: Result<T, CompileError>,
) {
    if let Err(
        error @ (CompileError::Limit { .. }
        | CompileError::InvalidDuration { .. }
        | CompileError::Cycle { .. }),
    ) = result
    {
        let field = format!("{field}.compile");
        let message = error.to_string();
        if issue_keys.insert((field.clone(), message.clone())) {
            issues.push(ValidationIssue::new(field, message));
        }
    }
}

fn limit(
    issues: &mut Vec<ValidationIssue>,
    field: impl Into<String>,
    length: usize,
    maximum: usize,
) {
    if length > maximum {
        issues.push(ValidationIssue::new(
            field,
            format!("at most {maximum} items are allowed"),
        ));
    }
}
fn dimension(issues: &mut Vec<ValidationIssue>, field: &str, value: u32) {
    if !(1..=8192).contains(&value) {
        issues.push(ValidationIssue::new(
            field,
            "format dimensions must be from 1 to 8192",
        ));
    }
}
fn id(issues: &mut Vec<ValidationIssue>, seen: &mut HashSet<String>, field: &str, value: &str) {
    if !is_valid_id(value) {
        issues.push(ValidationIssue::new(
            field,
            "id must be 1 to 160 ASCII letters, digits, '_', '.' or '-' and not reserved",
        ));
    }
    if !seen.insert(value.into()) {
        issues.push(ValidationIssue::new(
            field,
            "id must be unique within its kind",
        ));
    }
}
fn text(issues: &mut Vec<ValidationIssue>, field: &str, value: &str) {
    if value.chars().count() > 20_000 {
        issues.push(ValidationIssue::new(
            field,
            "text may not exceed 20000 characters",
        ));
    }
}
fn paths(issues: &mut Vec<ValidationIssue>, field: &str, values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        if !valid_path(value) {
            issues.push(ValidationIssue::new(format!("{field}[{index}]"), "path must be relative, 1 to 1000 characters, without ':', '\\', controls, or '..' segments"));
        }
    }
}
fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 1000
        && !value.starts_with('/')
        && !value.contains(':')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && !value.split('/').any(|segment| segment == "..")
}
fn geometry(issues: &mut Vec<ValidationIssue>, field: &str, value: f64) {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        issues.push(ValidationIssue::new(
            field,
            "geometry must be finite and within 1000000",
        ));
    }
}
fn position_coordinate(issues: &mut Vec<ValidationIssue>, field: &str, value: f64) {
    if !value.is_finite() || value.abs() > 100_000.0 {
        issues.push(ValidationIssue::new(
            field,
            "position must be finite and within 100000",
        ));
    }
}
fn color(issues: &mut Vec<ValidationIssue>, field: &str, value: &str) {
    if !value.is_empty() && !is_css_color(value) {
        issues.push(ValidationIssue::new(
            field,
            "color must parse as a CSS color",
        ));
    }
}
fn required_color(issues: &mut Vec<ValidationIssue>, field: &str, value: &str) {
    if value.is_empty() || !is_css_color(value) {
        issues.push(ValidationIssue::new(
            field,
            "color must parse as a CSS color",
        ));
    }
}
fn is_css_color(value: &str) -> bool {
    parse_color(value).is_some()
}
fn validate_layer(issues: &mut Vec<ValidationIssue>, path: &str, layer: &Layer) {
    if !["image", "text", "solid", "box"].contains(&layer.layer_type.as_str()) {
        issues.push(ValidationIssue::options(
            format!("{path}.type"),
            "invalid layer type",
            &["image", "text", "solid", "box"],
        ));
    }
    for (name, value) in [("x", layer.x), ("y", layer.y)] {
        geometry(issues, &format!("{path}.{name}"), value);
    }
    for (name, value) in [("width", layer.width), ("height", layer.height)] {
        if !value.is_finite() || !(0.0..=1_000_000.0).contains(&value) {
            issues.push(ValidationIssue::new(
                format!("{path}.{name}"),
                "size must be finite and from 0 to 1000000",
            ));
        }
    }
    if !layer.opacity.is_finite() || !(0.0..=1.0).contains(&layer.opacity) {
        issues.push(ValidationIssue::new(
            format!("{path}.opacity"),
            "opacity must be from 0 to 1",
        ));
    }
    text(issues, &format!("{path}.label"), &layer.label);
    if layer.layer_type == "image" && layer.src.as_deref().is_none_or(str::is_empty) {
        issues.push(ValidationIssue::new(
            format!("{path}.src"),
            "image layers require src",
        ));
    } else if let Some(src) = &layer.src
        && !valid_path(src)
    {
        issues.push(ValidationIssue::new(
            format!("{path}.src"),
            "invalid source path",
        ));
    }
    if let Some(fit) = &layer.fit
        && !["contain", "cover", "fill"].contains(&fit.as_str())
    {
        issues.push(ValidationIssue::options(
            format!("{path}.fit"),
            "invalid image fit",
            &["contain", "cover", "fill"],
        ));
    }
    if let Some(text_value) = &layer.text {
        text(issues, &format!("{path}.text"), text_value);
    }
    if let Some(size) = layer.font_size
        && (!size.is_finite() || !(0.0..=4096.0).contains(&size))
    {
        issues.push(ValidationIssue::new(
            format!("{path}.font_size"),
            "font size must be from 0 to 4096",
        ));
    }
    if let Some(weight) = &layer.weight
        && !["regular", "bold"].contains(&weight.as_str())
    {
        issues.push(ValidationIssue::options(
            format!("{path}.weight"),
            "invalid text weight",
            &["regular", "bold"],
        ));
    }
    if let Some(align) = &layer.align
        && !["left", "center", "right"].contains(&align.as_str())
    {
        issues.push(ValidationIssue::options(
            format!("{path}.align"),
            "invalid text alignment",
            &["left", "center", "right"],
        ));
    }
    match layer.layer_type.as_str() {
        "text" if layer.color.as_deref().is_none_or(str::is_empty) => issues.push(
            ValidationIssue::new(format!("{path}.color"), "text layers require a CSS color"),
        ),
        "solid" if layer.fill.as_deref().is_none_or(str::is_empty) => issues.push(
            ValidationIssue::new(format!("{path}.fill"), "solid layers require a CSS color"),
        ),
        _ => {}
    }
    if let Some(color_value) = &layer.color {
        color(issues, &format!("{path}.color"), color_value);
    }
    if let Some(fill) = &layer.fill {
        color(issues, &format!("{path}.fill"), fill);
    }
    if let Some(crop) = &layer.crop {
        for (name, value) in [
            ("x", crop.x),
            ("y", crop.y),
            ("width", crop.width),
            ("height", crop.height),
        ] {
            geometry(issues, &format!("{path}.crop.{name}"), value);
        }
        if crop.width < 0.0 || crop.height < 0.0 {
            issues.push(ValidationIssue::new(
                format!("{path}.crop"),
                "crop width and height must be non-negative",
            ));
        }
    }
}

fn reference(
    issues: &mut Vec<ValidationIssue>,
    field: impl Into<String>,
    message: impl Into<String>,
    values: &HashSet<String>,
) {
    let mut valid: Vec<_> = values.iter().cloned().collect();
    valid.sort();
    issues.push(ValidationIssue {
        field: field.into(),
        message: message.into(),
        valid,
    });
}

#[allow(clippy::too_many_arguments)]
fn validate_pin(
    issues: &mut Vec<ValidationIssue>,
    field: &str,
    pin: &Pin,
    scene_ids: &HashSet<String>,
    shot_ids: &HashSet<String>,
    cut_ids: &HashSet<String>,
    shots_by_id: &HashMap<&str, &crate::Shot>,
    cuts_by_id: &HashMap<&str, &crate::Cut>,
) {
    let mut valid_for = |values: &HashSet<String>| {
        reference(issues, field, "pin does not resolve", values);
    };
    match pin {
        Pin::None => {}
        Pin::Scene(scene) if !scene_ids.contains(scene) => valid_for(scene_ids),
        Pin::Shot(shot) if !shot_ids.contains(shot) => valid_for(shot_ids),
        Pin::Take { shot, take } => {
            if let Some(shot) = shots_by_id.get(shot.as_str()) {
                if !shot.takes.iter().any(|candidate| candidate.id == *take) {
                    let valid = shot.takes.iter().map(|take| take.id.clone()).collect();
                    valid_for(&valid);
                }
            } else {
                valid_for(shot_ids);
            }
        }
        Pin::Panel { shot, take, panel } => {
            let Some(shot) = shots_by_id.get(shot.as_str()) else {
                return valid_for(shot_ids);
            };
            let Some(take) = shot.takes.iter().find(|candidate| candidate.id == *take) else {
                let valid = shot.takes.iter().map(|take| take.id.clone()).collect();
                return valid_for(&valid);
            };
            if !take.panels.iter().any(|candidate| candidate.id == *panel) {
                let valid = take.panels.iter().map(|panel| panel.id.clone()).collect();
                valid_for(&valid);
            }
        }
        Pin::Layer {
            shot,
            take,
            panel,
            layer,
        } => {
            let Some(shot) = shots_by_id.get(shot.as_str()) else {
                return valid_for(shot_ids);
            };
            let Some(take) = shot.takes.iter().find(|candidate| candidate.id == *take) else {
                let valid = shot.takes.iter().map(|take| take.id.clone()).collect();
                return valid_for(&valid);
            };
            let Some(panel) = take.panels.iter().find(|candidate| candidate.id == *panel) else {
                let valid = take.panels.iter().map(|panel| panel.id.clone()).collect();
                return valid_for(&valid);
            };
            if !panel.layers.iter().any(|candidate| candidate.id == *layer) {
                let valid = panel.layers.iter().map(|layer| layer.id.clone()).collect();
                valid_for(&valid);
            }
        }
        Pin::Clip { cut, clip } => {
            let Some(cut) = cuts_by_id.get(cut.as_str()) else {
                return valid_for(cut_ids);
            };
            if !cut.clips.iter().any(|candidate| candidate.id == *clip) {
                let valid = cut.clips.iter().map(|clip| clip.id.clone()).collect();
                valid_for(&valid);
            }
        }
        _ => {}
    }
}
