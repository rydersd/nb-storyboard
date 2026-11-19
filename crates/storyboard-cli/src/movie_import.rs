//! Import a movie as editable shots while retaining ranges into the original source.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{Map, Value, json};
use storyboard_core::{Author, ClipSource, Op, Project, ShotSource, compile_cut};
use storyboard_store::{import_source, open};
use uuid::Uuid;

use crate::ops_api::{self, ApiError};

pub(crate) const MAX_MOVIE_BYTES: u64 = 1024 * 1024 * 1024;

fn tool_output(program: &str, args: &[&str]) -> Result<Vec<u8>, ApiError> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        ops_api::validation(format!("{program} is required for movie import: {error}"))
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(ops_api::validation(format!(
            "{program} failed: {}",
            detail.trim()
        )));
    }
    Ok(output.stdout)
}

pub fn probe(path: &Path) -> Result<Value, ApiError> {
    let path = path
        .to_str()
        .ok_or_else(|| ops_api::validation("movie path is not UTF-8"))?;
    let output = tool_output(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,codec_name,width,height,r_frame_rate",
            "-of",
            "json",
            path,
        ],
    )?;
    let raw: Value = serde_json::from_slice(&output)
        .map_err(|error| ops_api::validation(format!("ffprobe response: {error}")))?;
    let streams = raw["streams"]
        .as_array()
        .ok_or_else(|| ops_api::validation("movie has no streams"))?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .ok_or_else(|| ops_api::validation("movie has no video stream"))?;
    if video["codec_name"] != "h264" {
        return Err(ops_api::validation(format!(
            "MP4 video codec {} cannot play in this workbench; use H.264 video",
            video["codec_name"].as_str().unwrap_or("unknown")
        )));
    }
    let audio = streams
        .iter()
        .find(|stream| stream["codec_type"] == "audio");
    if let Some(stream) = audio
        && stream["codec_name"] != "aac"
    {
        return Err(ops_api::validation(format!(
            "MP4 audio codec {} cannot play in this workbench; use AAC audio",
            stream["codec_name"].as_str().unwrap_or("unknown")
        )));
    }
    let duration: f64 = raw["format"]["duration"]
        .as_str()
        .ok_or_else(|| ops_api::validation("movie duration is unavailable"))?
        .parse()
        .map_err(|_| ops_api::validation("movie duration is invalid"))?;
    if !duration.is_finite() || duration <= 0.0 || duration > 3600.0 {
        return Err(ops_api::validation(
            "movie duration must be above 0 and at most one hour",
        ));
    }
    Ok(json!({
        "duration": duration,
        "codec": video["codec_name"],
        "width": video["width"],
        "height": video["height"],
        "frame_rate": video["r_frame_rate"],
        "audio": audio.is_some(),
    }))
}

pub fn probe_audio(path: &Path) -> Result<Value, ApiError> {
    let path = path
        .to_str()
        .ok_or_else(|| ops_api::validation("audio path is not UTF-8"))?;
    let output = tool_output(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,codec_name",
            "-of",
            "json",
            path,
        ],
    )?;
    let raw: Value = serde_json::from_slice(&output)
        .map_err(|error| ops_api::validation(format!("ffprobe response: {error}")))?;
    let audio = raw["streams"]
        .as_array()
        .and_then(|streams| {
            streams
                .iter()
                .find(|stream| stream["codec_type"] == "audio")
        })
        .ok_or_else(|| ops_api::validation("file has no audio stream"))?;
    let duration: f64 = raw["format"]["duration"]
        .as_str()
        .ok_or_else(|| ops_api::validation("audio duration is unavailable"))?
        .parse()
        .map_err(|_| ops_api::validation("audio duration is invalid"))?;
    if !duration.is_finite() || duration <= 0.0 || duration > 3600.0 {
        return Err(ops_api::validation(
            "audio duration must be above 0 and at most one hour",
        ));
    }
    Ok(json!({"duration": duration, "codec": audio["codec_name"]}))
}

/// Suggest hard-cut boundaries without changing the project. The caller presents these marks
/// for review; scene detection cannot infer editorial intent from a continuous shot.
pub fn suggest_cuts(
    path: &Path,
    fps: u32,
    duration: f64,
    threshold: f64,
) -> Result<Vec<f64>, ApiError> {
    if !threshold.is_finite() || !(0.1..=50.0).contains(&threshold) {
        return Err(ops_api::validation(
            "cut sensitivity threshold must be 0.1 to 50 percent",
        ));
    }
    let path = path
        .to_str()
        .ok_or_else(|| ops_api::validation("movie path is not UTF-8"))?;
    let filter =
        format!("scdet=threshold={threshold},metadata=mode=print:key=lavfi.scd.time:file=-");
    let output = tool_output(
        "ffmpeg",
        &[
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            path,
            "-vf",
            &filter,
            "-an",
            "-f",
            "null",
            "-",
        ],
    )?;
    if output.len() > 2 * 1024 * 1024 {
        return Err(ops_api::validation(
            "movie produced too many scene-change candidates; use manual marks or interval sampling",
        ));
    }
    let mut frames = BTreeSet::new();
    for line in String::from_utf8_lossy(&output).lines() {
        let Some(value) = line.strip_prefix("lavfi.scd.time=") else {
            continue;
        };
        let Ok(seconds) = value.parse::<f64>() else {
            continue;
        };
        if !seconds.is_finite() {
            continue;
        }
        let frame = (seconds * f64::from(fps)).round() as i64;
        if frame > 0 && (frame as f64 + 1.0) / f64::from(fps) <= duration + 1e-9 {
            frames.insert(frame);
        }
    }
    let frames = consolidate_suggested_frames(frames, fps);
    if frames.len() >= 200 {
        return Err(ops_api::validation(
            "movie has over 199 suggested cuts; raise the threshold or mark cuts manually",
        ));
    }
    Ok(frames
        .into_iter()
        .map(|frame| frame as f64 / f64::from(fps))
        .collect())
}

fn consolidate_suggested_frames(frames: BTreeSet<i64>, fps: u32) -> Vec<i64> {
    // A dissolve can cross the detector threshold on adjacent frames. Keep its first
    // candidate so the suggestion does not create a two-frame storyboard shot.
    let min_spacing = (f64::from(fps) * 0.25).round() as i64;
    let mut kept = Vec::new();
    for frame in frames {
        if kept.last().is_none_or(|last| frame - last > min_spacing) {
            kept.push(frame);
        }
    }
    kept
}

/// Bring an agent-produced movie back into the project without bypassing source containment or undo.
pub fn replace_movie(
    dir: &Path,
    base_version: &str,
    shot_id: &str,
    source: &Path,
    author: Author,
    note: Option<String>,
) -> Result<Value, ApiError> {
    if source
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("mp4"))
    {
        return Err(ops_api::validation("replacement must be an MP4 file"));
    }
    let metadata = source
        .symlink_metadata()
        .map_err(storyboard_store::StoreError::Io)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_MOVIE_BYTES
    {
        return Err(ops_api::validation(
            "replacement must be a nonempty regular MP4 through 1 GB",
        ));
    }
    let loaded = open(dir)?;
    let shot = loaded
        .project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .find(|shot| shot.id == shot_id)
        .ok_or_else(|| ops_api::validation(format!("no shot named {shot_id:?}")))?;
    let media = probe(source)?;
    let movie_duration = media["duration"]
        .as_f64()
        .expect("validated movie duration");
    let audio_duration = shot
        .source_audio
        .as_ref()
        .map_or(0.0, |audio| audio.end - audio.start);
    let last_panel = shot
        .takes
        .iter()
        .flat_map(|take| &take.panels)
        .map(|panel| panel.time)
        .fold(0.0_f64, f64::max);
    let duration = movie_duration
        .max(audio_duration)
        .max(last_panel + 1.0 / loaded.project.fps as f64);
    let destination = PathBuf::from(format!(
        "sources/imported-movies/{}/original.mp4",
        Uuid::new_v4()
    ));
    let copied = import_source(dir, source, &destination)?;
    let path = copied.to_string_lossy().replace('\\', "/");
    let patch = serde_json::from_value(json!({
        "source_movie": {"src": path, "start": 0.0, "end": movie_duration},
        "duration": duration,
    }))
    .expect("replacement patch object");
    let mut result = ops_api::commit_ops(
        dir,
        base_version,
        vec![Op::UpdateShot {
            shot: shot_id.into(),
            patch,
        }],
        author,
        note,
    )?;
    result["path"] = json!(path);
    result["media"] = media;
    Ok(result)
}

pub struct ImportTiming<'a> {
    pub interval: f64,
    pub cut_times: Option<&'a [f64]>,
}

pub fn import_ops(
    dir: &Path,
    project: &Project,
    folder: &str,
    scene: Option<&str>,
    title: &str,
    duration: f64,
    timing: ImportTiming<'_>,
) -> Result<Vec<Op>, ApiError> {
    let interval = timing.interval;
    let cut_times = timing.cut_times;
    if !interval.is_finite() || !(0.25..=60.0).contains(&interval) {
        return Err(ops_api::validation(
            "sample interval must be 0.25 to 60 seconds",
        ));
    }
    let ranges = if let Some(marks) = cut_times.filter(|marks| !marks.is_empty()) {
        if marks.len() >= 200 {
            return Err(ops_api::validation(
                "movie import must create at most 200 shots",
            ));
        }
        let fps = project.fps as f64;
        let mut previous = 0.0;
        let mut previous_frame = 0i64;
        let mut ranges = Vec::with_capacity(marks.len() + 1);
        for &mark in marks {
            if !mark.is_finite() {
                return Err(ops_api::validation("cut times must be finite seconds"));
            }
            let frame = (mark * fps).round() as i64;
            let time = frame as f64 / fps;
            if frame <= previous_frame || time >= duration || duration - time + 1e-9 < 1.0 / fps {
                return Err(ops_api::validation(
                    "cut times must increase, stay inside the movie, and leave at least one project frame per shot",
                ));
            }
            ranges.push((previous, time));
            previous = time;
            previous_frame = frame;
        }
        ranges.push((previous, duration));
        ranges
    } else {
        let fps = f64::from(project.fps);
        let total_frames = (duration * fps).round() as i64;
        if total_frames < 1 {
            return Err(ops_api::validation(
                "movie is shorter than one project frame",
            ));
        }
        let mut ranges = Vec::new();
        let mut previous_frame = 0i64;
        for number in 1..=(duration / interval).ceil() as usize {
            let frame = (number as f64 * interval * fps).round() as i64;
            if frame >= total_frames {
                break;
            }
            if frame > previous_frame {
                ranges.push((previous_frame as f64 / fps, frame as f64 / fps));
                previous_frame = frame;
            }
        }
        ranges.push((previous_frame as f64 / fps, duration));
        ranges
    };
    let count = ranges.len();
    if count == 0 || count > 200 {
        return Err(ops_api::validation(
            "movie import must create 1 to 200 shots; increase the interval",
        ));
    }
    let prefix = format!("movie-{}", folder.replace('-', ""));
    let source = format!("sources/imported-movies/{folder}/original.mp4");
    let scene_id = match scene {
        Some(id) => {
            if !project.scenes.iter().any(|item| item.id == id) {
                return Err(ops_api::validation(format!("unknown scene {id}")));
            }
            id.to_owned()
        }
        None => format!("{prefix}-scene"),
    };
    let mut ops = Vec::with_capacity(count * 3 + 1);
    let mut draft = project.clone();
    if scene.is_none() {
        let add = Op::AddScene {
            id: Some(scene_id.clone()),
            title: title.to_owned(),
            index: None,
        };
        storyboard_core::apply(&mut draft, &add)
            .map_err(|error| ops_api::validation(error.to_string()))?;
        ops.push(add);
    }
    let movie_path = dir.join(&source);
    for (number, (start, end)) in ranges.into_iter().enumerate() {
        let shot = format!("{prefix}-shot-{}", number + 1);
        let layer = format!("{prefix}-layer-{}", number + 1);
        let frame = format!(
            "sources/imported-movies/{folder}/frame-{:03}.png",
            number + 1
        );
        let frame_path = dir.join(&frame);
        let middle = (start + end) / 2.0;
        let position = format!("{middle:.6}");
        let movie_arg = movie_path
            .to_str()
            .ok_or_else(|| ops_api::validation("movie path is not UTF-8"))?;
        let frame_arg = frame_path
            .to_str()
            .ok_or_else(|| ops_api::validation("frame path is not UTF-8"))?;
        tool_output(
            "ffmpeg",
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &position,
                "-i",
                movie_arg,
                "-frames:v",
                "1",
                "-f",
                "image2",
                frame_arg,
            ],
        )?;
        if !fs::metadata(&frame_path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        {
            return Err(ops_api::validation(format!(
                "could not extract frame at {middle:.2} seconds"
            )));
        }
        let add = Op::AddShot {
            scene: scene_id.clone(),
            id: Some(shot.clone()),
            title: format!("{title} {:02}", number + 1),
            index: None,
        };
        storyboard_core::apply(&mut draft, &add)
            .map_err(|error| ops_api::validation(error.to_string()))?;
        ops.push(add);
        let created = draft
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .find(|item| item.id == shot)
            .expect("shot just added");
        let take = created.selected_take.clone();
        let panel = created.takes[0].panels[0].id.clone();
        let mut patch = Map::new();
        patch.insert("duration".into(), json!(end - start));
        patch.insert(
            "source_movie".into(),
            json!({"src":source,"start":start,"end":end}),
        );
        ops.push(Op::UpdateShot {
            shot: shot.clone(),
            patch,
        });
        let mut fields = Map::new();
        fields.insert("src".into(), json!(frame));
        fields.insert("fit".into(), json!("contain"));
        ops.push(Op::AddLayer {
            shot,
            take,
            panel,
            id: Some(layer),
            layer_type: "image".into(),
            fields,
            index: None,
        });
    }
    Ok(vec![Op::Batch { ops }])
}

/// Split one untouched imported movie shot and its sole cut placement into two source-linked
/// shots. Other takes, attached VO, reused shots and manual trims need an explicit editorial
/// decision, so this narrow operation refuses them instead of copying or discarding edits.
pub fn split_shot_ops(
    dir: &Path,
    project: &Project,
    cut_id: &str,
    clip_id: &str,
    frame: u64,
) -> Result<Vec<Op>, ApiError> {
    let cut = project
        .cuts
        .iter()
        .find(|cut| cut.id == cut_id)
        .ok_or_else(|| ops_api::validation("cut not found"))?;
    let clip_index = cut
        .clips
        .iter()
        .position(|clip| clip.id == clip_id)
        .ok_or_else(|| ops_api::validation("clip not found in cut"))?;
    let clip = &cut.clips[clip_index];
    let ClipSource::Shot(source) = &clip.source else {
        return Err(ops_api::validation("split an individual shot placement"));
    };
    if clip.in_frame != 0
        || clip.out_frame.is_some()
        || clip.slot.is_some()
        || source.take.is_some()
    {
        return Err(ops_api::validation(
            "remove placement trims, fixed slot and take override before splitting into shots",
        ));
    }
    let references = project
        .cuts
        .iter()
        .flat_map(|cut| &cut.clips)
        .filter(|clip| matches!(&clip.source, ClipSource::Shot(value) if value.shot == source.shot))
        .count();
    if references != 1 {
        return Err(ops_api::validation(
            "this shot is reused; split a placement instead",
        ));
    }
    let compiled = compile_cut(project, cut_id)
        .map_err(|error| ops_api::validation(format!("cannot compile cut: {error}")))?;
    let item = compiled
        .items
        .iter()
        .find(|item| item.clip.as_deref() == Some(clip_id))
        .ok_or_else(|| ops_api::validation("shot placement is not in the compiled cut"))?;
    if item.offset != 0 || frame <= item.start || frame >= item.end - 1 {
        return Err(ops_api::validation(
            "split at least one frame inside the movie shot",
        ));
    }
    let (scene_index, shot_index, shot) = project
        .scenes
        .iter()
        .enumerate()
        .find_map(|(scene_index, scene)| {
            scene
                .shots
                .iter()
                .enumerate()
                .find(|(_, shot)| shot.id == source.shot)
                .map(|(shot_index, shot)| (scene_index, shot_index, shot))
        })
        .ok_or_else(|| ops_api::validation("source shot not found"))?;
    let movie = shot
        .source_movie
        .as_ref()
        .ok_or_else(|| ops_api::validation("shot has no source movie"))?;
    if shot.source_audio.is_some()
        || shot.takes.len() != 1
        || shot.takes[0].panels.len() != 1
        || shot.takes[0].panels[0].time != 0.0
        || shot.takes[0].panels[0].layers.len() != 1
        || shot.takes[0].panels[0].layers[0].layer_type != "image"
        || (shot.duration - (movie.end - movie.start)).abs() > 1.0 / project.fps as f64
    {
        return Err(ops_api::validation(
            "split into shots supports an untouched, one-panel movie import without VO",
        ));
    }
    let folder = movie
        .src
        .strip_prefix("sources/imported-movies/")
        .and_then(|rest| rest.strip_suffix("/original.mp4"))
        .ok_or_else(|| ops_api::validation("movie source is not an imported MP4"))?;
    if uuid::Uuid::parse_str(folder).is_err() {
        return Err(ops_api::validation("movie source folder is not a UUID"));
    }
    let source_dir = dir.join("sources/imported-movies").join(folder);
    for parent in [dir.join("sources"), dir.join("sources/imported-movies")] {
        if !parent
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        {
            return Err(ops_api::validation("movie source parent folder is invalid"));
        }
    }
    if !source_dir
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
    {
        return Err(ops_api::validation("movie source folder is invalid"));
    }
    let movie_path = source_dir.join("original.mp4");
    if !movie_path
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
    {
        return Err(ops_api::validation("movie source is invalid"));
    }
    let old_layer = &shot.takes[0].panels[0].layers[0];
    if !old_layer
        .src
        .as_deref()
        .is_some_and(|src| src.starts_with(&format!("sources/imported-movies/{folder}/")))
    {
        return Err(ops_api::validation(
            "the panel image has been revised; split its placement instead",
        ));
    }
    let boundary = movie.start + (frame - item.start) as f64 / project.fps as f64;
    if boundary <= movie.start || boundary >= movie.end {
        return Err(ops_api::validation(
            "playhead is outside the source movie range",
        ));
    }
    let token = Uuid::new_v4();
    let left = format!("sources/imported-movies/{folder}/split-{token}-a.png");
    let right = format!("sources/imported-movies/{folder}/split-{token}-b.png");
    let movie_arg = movie_path
        .to_str()
        .ok_or_else(|| ops_api::validation("movie path is not UTF-8"))?;
    for (path, middle) in [
        (&left, (movie.start + boundary) / 2.0),
        (&right, (boundary + movie.end) / 2.0),
    ] {
        let frame_path = dir.join(path);
        let frame_arg = frame_path
            .to_str()
            .ok_or_else(|| ops_api::validation("frame path is not UTF-8"))?;
        tool_output(
            "ffmpeg",
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &format!("{middle:.6}"),
                "-i",
                movie_arg,
                "-frames:v",
                "1",
                "-f",
                "image2",
                frame_arg,
            ],
        )?;
        if !fs::metadata(frame_path).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
            return Err(ops_api::validation(
                "could not extract split storyboard frame",
            ));
        }
    }
    let new_shot = format!("shot-{}", Uuid::new_v4());
    let new_layer = format!("layer-{}", Uuid::new_v4());
    let new_clip = format!("clip-{}", Uuid::new_v4());
    let mut ops = vec![
        Op::UpdateShot {
            shot: shot.id.clone(),
            patch: Map::from_iter([
                ("duration".into(), json!(boundary - movie.start)),
                (
                    "source_movie".into(),
                    json!({"src":movie.src,"start":movie.start,"end":boundary}),
                ),
            ]),
        },
        Op::UpdateLayer {
            shot: shot.id.clone(),
            take: shot.takes[0].id.clone(),
            panel: shot.takes[0].panels[0].id.clone(),
            layer: old_layer.id.clone(),
            patch: Map::from_iter([("src".into(), json!(left))]),
        },
        Op::AddShot {
            scene: project.scenes[scene_index].id.clone(),
            id: Some(new_shot.clone()),
            title: format!(
                "{} part 2",
                shot.title.chars().take(193).collect::<String>()
            ),
            index: Some(shot_index + 1),
        },
    ];
    let mut draft = project.clone();
    for op in &ops {
        storyboard_core::apply(&mut draft, op)
            .map_err(|error| ops_api::validation(error.to_string()))?;
    }
    let created = draft.scenes[scene_index]
        .shots
        .iter()
        .find(|shot| shot.id == new_shot)
        .expect("new shot applied");
    let mut patch = Map::new();
    for (field, value) in [
        ("action", &shot.action),
        ("dialogue", &shot.dialogue),
        ("speaker", &shot.speaker),
        ("camera", &shot.camera),
        ("notes", &shot.notes),
        ("status", &shot.status),
    ] {
        patch.insert(field.into(), json!(value));
    }
    patch.insert("duration".into(), json!(movie.end - boundary));
    patch.insert(
        "source_movie".into(),
        json!({"src":movie.src,"start":boundary,"end":movie.end}),
    );
    ops.push(Op::UpdateShot {
        shot: new_shot.clone(),
        patch,
    });
    let old_fields = serde_json::to_value(old_layer)
        .map_err(|error| ops_api::validation(format!("panel layer: {error}")))?;
    let mut fields = Map::new();
    for key in [
        "x", "y", "width", "height", "opacity", "hidden", "label", "fit", "crop",
    ] {
        if let Some(value) = old_fields.get(key) {
            fields.insert(key.into(), value.clone());
        }
    }
    fields.insert("src".into(), json!(right));
    ops.push(Op::AddLayer {
        shot: new_shot.clone(),
        take: created.selected_take.clone(),
        panel: created.takes[0].panels[0].id.clone(),
        id: Some(new_layer),
        layer_type: "image".into(),
        fields,
        index: None,
    });
    ops.push(Op::AddClip {
        cut: cut_id.into(),
        id: Some(new_clip),
        source: ClipSource::Shot(ShotSource {
            shot: new_shot,
            take: None,
            extra: Default::default(),
            source_extra: Default::default(),
        }),
        index: Some(clip_index + 1),
    });
    Ok(vec![Op::Batch { ops }])
}

/// Roll the source boundary between two untouched, adjacent imported movie shots.
/// Both source ranges and representative panels move in one undoable edit.
pub fn move_boundary_ops(
    dir: &Path,
    project: &Project,
    cut_id: &str,
    left_clip_id: &str,
    frame: u64,
) -> Result<Vec<Op>, ApiError> {
    let cut = project
        .cuts
        .iter()
        .find(|cut| cut.id == cut_id)
        .ok_or_else(|| ops_api::validation("cut not found"))?;
    let index = cut
        .clips
        .iter()
        .position(|clip| clip.id == left_clip_id)
        .ok_or_else(|| ops_api::validation("left clip not found"))?;
    let left_clip = &cut.clips[index];
    let right_clip = cut
        .clips
        .get(index + 1)
        .ok_or_else(|| ops_api::validation("there is no following clip"))?;
    let (ClipSource::Shot(left_source), ClipSource::Shot(right_source)) =
        (&left_clip.source, &right_clip.source)
    else {
        return Err(ops_api::validation("roll two direct shot placements"));
    };
    for (clip, source) in [(left_clip, left_source), (right_clip, right_source)] {
        if clip.in_frame != 0
            || clip.out_frame.is_some()
            || clip.slot.is_some()
            || source.take.is_some()
        {
            return Err(ops_api::validation(
                "remove placement trims, fixed slots and take overrides before moving the source boundary",
            ));
        }
        let references = project
            .cuts
            .iter()
            .flat_map(|cut| &cut.clips)
            .filter(|candidate| matches!(&candidate.source, ClipSource::Shot(value) if value.shot == source.shot))
            .count();
        if references != 1 {
            return Err(ops_api::validation(
                "a shot is reused; move placement trims instead",
            ));
        }
    }
    let shot = |id: &str| {
        project
            .scenes
            .iter()
            .flat_map(|scene| &scene.shots)
            .find(|shot| shot.id == id)
            .ok_or_else(|| ops_api::validation("source shot not found"))
    };
    let left = shot(&left_source.shot)?;
    let right = shot(&right_source.shot)?;
    let movie = left
        .source_movie
        .as_ref()
        .ok_or_else(|| ops_api::validation("left shot has no imported movie"))?;
    let next_movie = right
        .source_movie
        .as_ref()
        .ok_or_else(|| ops_api::validation("right shot has no imported movie"))?;
    if movie.src != next_movie.src || (movie.end - next_movie.start).abs() > 1e-6 {
        return Err(ops_api::validation(
            "both shots must have adjoining ranges in the same movie",
        ));
    }
    let folder = movie
        .src
        .strip_prefix("sources/imported-movies/")
        .and_then(|rest| rest.strip_suffix("/original.mp4"))
        .ok_or_else(|| ops_api::validation("movie source is not an imported MP4"))?;
    if Uuid::parse_str(folder).is_err() {
        return Err(ops_api::validation("movie source folder is not a UUID"));
    }
    let source_dir = dir.join("sources/imported-movies").join(folder);
    for parent in [
        dir.join("sources"),
        dir.join("sources/imported-movies"),
        source_dir.clone(),
    ] {
        if !parent
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        {
            return Err(ops_api::validation("movie source folder is invalid"));
        }
    }
    let movie_path = source_dir.join("original.mp4");
    if !movie_path
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
    {
        return Err(ops_api::validation("movie source is invalid"));
    }
    let imported_layer = |shot: &storyboard_core::Shot| {
        if shot.source_audio.is_some()
            || shot.takes.len() != 1
            || shot.takes[0].panels.len() != 1
            || shot.takes[0].panels[0].time != 0.0
            || shot.takes[0].panels[0].layers.len() != 1
            || shot.takes[0].panels[0].layers[0].layer_type != "image"
            || (shot.duration
                - (shot
                    .source_movie
                    .as_ref()
                    .expect("checked source movie")
                    .end
                    - shot
                        .source_movie
                        .as_ref()
                        .expect("checked source movie")
                        .start))
                .abs()
                > 1.0 / project.fps as f64
        {
            return Err(ops_api::validation(
                "boundary editing supports untouched one-panel movie shots without VO",
            ));
        }
        let layer = &shot.takes[0].panels[0].layers[0];
        if !layer
            .src
            .as_deref()
            .is_some_and(|src| src.starts_with(&format!("sources/imported-movies/{folder}/")))
        {
            return Err(ops_api::validation("a storyboard panel has been revised"));
        }
        Ok(())
    };
    imported_layer(left)?;
    imported_layer(right)?;
    let left_layer = &left.takes[0].panels[0].layers[0];
    let right_layer = &right.takes[0].panels[0].layers[0];
    let compiled = compile_cut(project, cut_id)
        .map_err(|error| ops_api::validation(format!("cannot compile cut: {error}")))?;
    let left_item = compiled
        .items
        .iter()
        .find(|item| item.clip.as_deref() == Some(left_clip_id))
        .ok_or_else(|| ops_api::validation("left shot is not in the compiled cut"))?;
    let right_item = compiled
        .items
        .iter()
        .find(|item| item.clip.as_deref() == Some(&right_clip.id))
        .ok_or_else(|| ops_api::validation("right shot is not in the compiled cut"))?;
    if left_item.end != right_item.start
        || left_item.offset != 0
        || right_item.offset != 0
        || frame <= left_item.start
        || frame >= right_item.end
    {
        return Err(ops_api::validation(
            "place the boundary at least one frame inside the two adjacent shots",
        ));
    }
    let boundary = movie.start + (frame - left_item.start) as f64 / project.fps as f64;
    if boundary <= movie.start || boundary >= next_movie.end {
        return Err(ops_api::validation("boundary is outside the movie ranges"));
    }
    let movie_arg = movie_path
        .to_str()
        .ok_or_else(|| ops_api::validation("movie path is not UTF-8"))?;
    let token = Uuid::new_v4();
    let paths = [
        format!("sources/imported-movies/{folder}/boundary-{token}-a.png"),
        format!("sources/imported-movies/{folder}/boundary-{token}-b.png"),
    ];
    for (path, middle) in [
        (&paths[0], (movie.start + boundary) / 2.0),
        (&paths[1], (boundary + next_movie.end) / 2.0),
    ] {
        let target = dir.join(path);
        let target_arg = target
            .to_str()
            .ok_or_else(|| ops_api::validation("frame path is not UTF-8"))?;
        tool_output(
            "ffmpeg",
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &format!("{middle:.6}"),
                "-i",
                movie_arg,
                "-frames:v",
                "1",
                "-f",
                "image2",
                target_arg,
            ],
        )?;
        if !fs::metadata(target).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
            return Err(ops_api::validation(
                "could not extract boundary storyboard frame",
            ));
        }
    }
    let mut ops = Vec::new();
    for (shot, layer, start, end, path) in [
        (left, left_layer, movie.start, boundary, &paths[0]),
        (right, right_layer, boundary, next_movie.end, &paths[1]),
    ] {
        ops.push(Op::UpdateShot {
            shot: shot.id.clone(),
            patch: Map::from_iter([
                ("duration".into(), json!(end - start)),
                (
                    "source_movie".into(),
                    json!({"src":movie.src,"start":start,"end":end}),
                ),
            ]),
        });
        ops.push(Op::UpdateLayer {
            shot: shot.id.clone(),
            take: shot.takes[0].id.clone(),
            panel: shot.takes[0].panels[0].id.clone(),
            layer: layer.id.clone(),
            patch: Map::from_iter([("src".into(), json!(path))]),
        });
    }
    Ok(vec![Op::Batch { ops }])
}

#[cfg(test)]
mod suggest_cut_tests {
    use super::consolidate_suggested_frames;
    use std::collections::BTreeSet;

    #[test]
    fn nearby_detector_frames_do_not_make_tiny_shots() {
        let frames = BTreeSet::from([124, 584, 586, 1352, 1354]);
        assert_eq!(consolidate_suggested_frames(frames, 30), [124, 584, 1352]);
    }
}
