//! Render the compiled cut to a frame-accurate H.264/AAC movie.
//!
//! Every placement becomes one or more FFmpeg concat inputs. This keeps the
//! picture and audio on one timeline, including black slots and held frames.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{Value, json};
use storyboard_core::{Compiled, Project, RenderMode, compile_active, compile_cut, render_panel};
use storyboard_store::Loaded;

use crate::ops_api::{self, ApiError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageMode {
    Original,
    Boards,
}

impl ImageMode {
    pub fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "original" => Ok(Self::Original),
            "boards" => Ok(Self::Boards),
            _ => Err(ops_api::options(
                "export image mode must be original or boards",
                ["original", "boards"],
            )),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Boards => "boards",
        }
    }
}

#[derive(Clone)]
enum Picture {
    Movie {
        path: PathBuf,
        start: f64,
        available: u64,
    },
    HeldMovie {
        path: PathBuf,
        end: f64,
    },
    Panel {
        shot: String,
        take: String,
        panel: String,
    },
    Black,
}

#[derive(Clone)]
struct Piece {
    frames: u64,
    picture: Picture,
    audio: Option<(PathBuf, f64, f64)>,
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn export(dir: &Path, cut: Option<&str>, mode: ImageMode) -> Result<Value, ApiError> {
    let loaded = storyboard_store::open(dir)?;
    export_loaded(&loaded, cut, mode)
}

pub(crate) fn export_loaded(
    loaded: &Loaded,
    cut: Option<&str>,
    mode: ImageMode,
) -> Result<Value, ApiError> {
    let dir = loaded.dir.as_path();
    let project = &loaded.project;
    let sequence = match cut {
        Some(id) => compile_cut(project, id)?,
        None => compile_active(project)?,
    };
    if sequence.frames == 0 {
        return Err(ops_api::validation("nothing to export in this cut"));
    }
    if sequence.frames > 30 * 60 * 60 * u64::from(project.fps) {
        return Err(ops_api::validation("export exceeds 30 hours"));
    }
    let root = dir
        .canonicalize()
        .map_err(|e| ops_api::validation(format!("project path: {e}")))?;
    let export_dir = checked_export_dir(&root)?;
    let cut_name = cut
        .or(project.active_cut.as_deref())
        .unwrap_or("story-order");
    let safe_cut: String = cut_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    // Bump this when rendering changes so an old project-version cache cannot
    // return frames produced by the previous renderer.
    const RENDER_REVISION: &str = "r2";
    let file_name = format!(
        "{}-{}-{}-{}.mp4",
        safe_cut,
        mode.name(),
        &loaded.version[..12],
        RENDER_REVISION
    );
    let output = export_dir.join(&file_name);
    if output
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink() && meta.len() > 0)
    {
        return Ok(
            json!({"path": output, "file": file_name, "version": loaded.version, "frames": sequence.frames, "cached": true}),
        );
    }
    let scratch = Scratch(export_dir.join(format!(".render-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&scratch.0).map_err(|e| ops_api::validation(format!("export scratch: {e}")))?;
    let pieces = plan(project, &sequence, mode, &root)?;
    let pending = scratch.0.join("movie.mp4");
    // FFmpeg keeps every picture input in a chunk open at once. Bound that
    // decoder/filter footprint by both input count and output pixel count.
    const MAX_CHUNK_PIECES: usize = 32;
    const CHUNK_PIXEL_BUDGET: u64 = 8 * 1920 * 1080;
    let pixels = u64::from(project.format.width) * u64::from(project.format.height);
    let chunk_pieces = ((CHUNK_PIXEL_BUDGET / pixels.max(1)).max(1) as usize).min(MAX_CHUNK_PIECES);
    if pieces.len() <= chunk_pieces {
        render(
            project,
            &root,
            &pieces,
            &scratch.0,
            &pending,
            sequence.frames,
            false,
        )?;
    } else {
        let mut chunks = Vec::new();
        for (index, group) in pieces.chunks(chunk_pieces).enumerate() {
            let frames = group.iter().map(|piece| piece.frames).sum();
            let file = format!("chunk-{index:05}.mkv");
            render(
                project,
                &root,
                group,
                &scratch.0,
                &scratch.0.join(&file),
                frames,
                true,
            )?;
            chunks.push((file, frames));
        }
        join_chunks(&scratch.0, &chunks, &pending, sequence.frames, project.fps)?;
    }
    fs::rename(&pending, &output)
        .map_err(|e| ops_api::validation(format!("saving export: {e}")))?;
    Ok(
        json!({"path": output, "file": file_name, "version": loaded.version, "frames": sequence.frames, "cached": false}),
    )
}

fn checked_export_dir(root: &Path) -> Result<PathBuf, ApiError> {
    let mut path = root.to_path_buf();
    for name in [".cache", "exports"] {
        path.push(name);
        match path.symlink_metadata() {
            Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                return Err(ops_api::validation(
                    "export cache path is not a real directory",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&path)
                .map_err(|e| ops_api::validation(format!("creating export cache: {e}")))?,
            Err(e) => return Err(ops_api::validation(format!("export cache: {e}"))),
        }
    }
    Ok(path)
}

fn media_path(root: &Path, source: &str) -> Result<PathBuf, ApiError> {
    let relative = Path::new(source);
    if relative.is_absolute()
        || !source.starts_with("sources/")
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(ops_api::validation(
            "media source must stay under project sources/",
        ));
    }
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        let meta = path
            .symlink_metadata()
            .map_err(|e| ops_api::validation(format!("media source {source}: {e}")))?;
        if meta.file_type().is_symlink() {
            return Err(ops_api::validation(format!(
                "media source {source} is a symlink"
            )));
        }
    }
    if !path.is_file() {
        return Err(ops_api::validation(format!(
            "media source {source} is not a file"
        )));
    }
    Ok(path)
}

fn plan(
    project: &Project,
    sequence: &Compiled,
    mode: ImageMode,
    root: &Path,
) -> Result<Vec<Piece>, ApiError> {
    let mut pieces = Vec::new();
    let mut cursor = 0;
    let fps = f64::from(project.fps);
    for item in &sequence.items {
        if item.start > cursor {
            pieces.push(Piece {
                frames: item.start - cursor,
                picture: Picture::Black,
                audio: None,
            });
        }
        let shot = project
            .scenes
            .iter()
            .flat_map(|s| &s.shots)
            .find(|s| s.id == item.shot)
            .ok_or_else(|| ops_api::validation(format!("missing shot {}", item.shot)))?;
        let take = shot
            .takes
            .iter()
            .find(|t| t.id == item.take)
            .ok_or_else(|| ops_api::validation(format!("missing take {}", item.take)))?;
        let movie = shot.source_movie.as_ref();
        let placement_voice = item.leaf_clip.as_ref().and_then(|clip_id| {
            project
                .cuts
                .iter()
                .flat_map(|cut| &cut.clips)
                .find(|clip| &clip.id == clip_id)
                .and_then(|clip| clip.source_audio.as_ref())
        });
        let voice = placement_voice.or(shot.source_audio.as_ref());
        let audio_source = voice
            .map(|v| (&v.src, v.start, v.end))
            .or_else(|| movie.map(|m| (&m.src, m.start, m.end)));
        let mut start = item.start;
        while start < item.end {
            let local = item.offset + start - item.start;
            let local_seconds = local as f64 / fps;
            let panel = take
                .panels
                .iter()
                .rev()
                .find(|p| p.time <= local_seconds + 1e-9)
                .or_else(|| take.panels.first());
            let picture = if mode == ImageMode::Original {
                if let Some(movie) = movie {
                    let source_time = movie.start + local_seconds;
                    let path = media_path(root, &movie.src)?;
                    if source_time + 1e-9 >= movie.end {
                        Picture::HeldMovie {
                            path,
                            end: movie.end,
                        }
                    } else {
                        let available = ((movie.end - source_time) * fps).ceil() as u64;
                        Picture::Movie {
                            path,
                            start: source_time,
                            available: available.max(1),
                        }
                    }
                } else if let Some(panel) = panel {
                    Picture::Panel {
                        shot: shot.id.clone(),
                        take: take.id.clone(),
                        panel: panel.id.clone(),
                    }
                } else {
                    Picture::Black
                }
            } else if let Some(panel) = panel {
                Picture::Panel {
                    shot: shot.id.clone(),
                    take: take.id.clone(),
                    panel: panel.id.clone(),
                }
            } else {
                Picture::Black
            };
            let end = if matches!(picture, Picture::Movie { .. }) {
                let movie = movie.expect("movie picture has a source range");
                let hold_local = ((movie.end - movie.start) * fps - 1e-9).ceil() as u64;
                (start + hold_local.saturating_sub(local).max(1)).min(item.end)
            } else if matches!(picture, Picture::HeldMovie { .. }) {
                item.end
            } else {
                let mut end = start + 1;
                while end < item.end {
                    let next_local = (item.offset + end - item.start) as f64 / fps;
                    let next_panel = take
                        .panels
                        .iter()
                        .rev()
                        .find(|p| p.time <= next_local + 1e-9)
                        .or_else(|| take.panels.first());
                    if next_panel.map(|p| &p.id) != panel.map(|p| &p.id) {
                        break;
                    }
                    end += 1;
                }
                end
            };
            let frames = end - start;
            let audio = if let Some((src, source_start, source_end)) = audio_source {
                let at = source_start + local_seconds;
                let duration = (source_end - at).max(0.0).min(frames as f64 / fps);
                if duration > 0.0 {
                    Some((media_path(root, src)?, at, duration))
                } else {
                    None
                }
            } else {
                None
            };
            pieces.push(Piece {
                frames,
                picture,
                audio,
            });
            start = end;
        }
        cursor = item.end;
    }
    if cursor < sequence.frames {
        pieces.push(Piece {
            frames: sequence.frames - cursor,
            picture: Picture::Black,
            audio: None,
        });
    }
    Ok(pieces)
}

fn extract_last_frame(path: &Path, end: f64, output: &Path) -> Result<(), ApiError> {
    // Decode at most the last second of the selected source range. Reversing
    // that bounded window selects its actual final decoded frame, independent
    // of the project's output frame rate.
    let start = (end - 1.0).max(0.0);
    let result = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-ss",
        ])
        .arg(format!("{start:.9}"))
        .arg("-t")
        .arg(format!("{:.9}", end - start))
        .arg("-i")
        .arg(path)
        .args(["-vf", "reverse", "-frames:v", "1"])
        .arg(output)
        .output()
        .map_err(|e| ops_api::validation(format!("extracting held movie frame: {e}")))?;
    if !result.status.success()
        || !output.is_file()
        || fs::metadata(output).is_ok_and(|m| m.len() == 0)
    {
        return Err(ops_api::validation(format!(
            "extracting held movie frame failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}

fn has_audio(path: &Path) -> Result<bool, ApiError> {
    let result = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|e| ops_api::validation(format!("ffprobe is required: {e}")))?;
    if !result.status.success() {
        return Err(ops_api::validation(format!(
            "could not probe audio in {}",
            path.display()
        )));
    }
    Ok(!result.stdout.is_empty())
}

fn render(
    project: &Project,
    root: &Path,
    pieces: &[Piece],
    scratch: &Path,
    pending: &Path,
    total_frames: u64,
    intermediate: bool,
) -> Result<(), ApiError> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);
    let mut filters = Vec::new();
    let mut join = String::new();
    let mut input = 0;
    let mut panels: HashMap<(String, String, String), PathBuf> = HashMap::new();
    let mut held_frames: HashMap<(PathBuf, u64), PathBuf> = HashMap::new();
    let mut audio_available = HashMap::new();
    let fps = project.fps;
    let width = project.format.width;
    let height = project.format.height;
    for (number, piece) in pieces.iter().enumerate() {
        let duration = piece.frames as f64 / f64::from(fps);
        let (video_input, source_frames) = match &piece.picture {
            Picture::Movie {
                path,
                start,
                available,
            } => {
                cmd.args(["-ss", &format!("{start:.9}")])
                    .arg("-i")
                    .arg(path);
                let index = input;
                input += 1;
                (index, (*available).min(piece.frames))
            }
            Picture::HeldMovie { path, end } => {
                let key = (path.clone(), end.to_bits());
                let still = if let Some(still) = held_frames.get(&key) {
                    still.clone()
                } else {
                    let still = scratch.join(format!("held-{}.png", held_frames.len()));
                    extract_last_frame(path, *end, &still)?;
                    held_frames.insert(key, still.clone());
                    still
                };
                cmd.args(["-loop", "1", "-framerate", &fps.to_string(), "-i"])
                    .arg(still);
                let index = input;
                input += 1;
                (index, piece.frames)
            }
            Picture::Panel { shot, take, panel } => {
                let key = (shot.clone(), take.clone(), panel.clone());
                let path = if let Some(path) = panels.get(&key) {
                    path.clone()
                } else {
                    let path = scratch.join(format!("panel-{}.png", panels.len()));
                    let rendered =
                        render_panel(project, root, shot, take, panel, width, RenderMode::Stage)?;
                    fs::write(&path, rendered.png)
                        .map_err(|e| ops_api::validation(format!("rendering panel: {e}")))?;
                    panels.insert(key, path.clone());
                    path
                };
                cmd.args(["-loop", "1", "-framerate", &fps.to_string(), "-i"])
                    .arg(path);
                let index = input;
                input += 1;
                (index, piece.frames)
            }
            Picture::Black => {
                cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("color=c=black:s={width}x{height}:r={fps}"),
                ]);
                let index = input;
                input += 1;
                (index, piece.frames)
            }
        };
        let audio_input = if let Some((path, start, _)) = &piece.audio {
            let available = if let Some(value) = audio_available.get(path) {
                *value
            } else {
                let value = has_audio(path)?;
                audio_available.insert(path.clone(), value);
                value
            };
            if available {
                cmd.args(["-ss", &format!("{start:.9}")])
                    .arg("-i")
                    .arg(path);
            } else {
                cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=channel_layout=stereo:sample_rate=48000",
                ]);
            }
            let index = input;
            input += 1;
            index
        } else {
            cmd.args([
                "-f",
                "lavfi",
                "-i",
                "anullsrc=channel_layout=stereo:sample_rate=48000",
            ]);
            let index = input;
            input += 1;
            index
        };
        let video = format!(
            "[{video_input}:v]setpts=PTS-STARTPTS,fps={fps},trim=end_frame={source_frames},tpad=stop={}:stop_mode=clone,trim=end_frame={},setpts=PTS-STARTPTS,scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,setsar=1[v{number}]",
            piece.frames, piece.frames
        );
        filters.push(video);
        let source_audio_duration = piece.audio.as_ref().map_or(duration, |(_, _, span)| *span);
        let audio = format!(
            "[{audio_input}:a]atrim=duration={source_audio_duration:.9},asetpts=PTS-STARTPTS,aresample=48000,apad,atrim=duration={duration:.9},asetpts=PTS-STARTPTS[a{number}]"
        );
        filters.push(audio);
        join.push_str(&format!("[v{number}][a{number}]"));
    }
    filters.push(format!("{join}concat=n={}:v=1:a=1[v][a]", pieces.len()));
    let cmd = cmd.arg("-filter_complex").arg(filters.join(";")).args([
        "-map",
        "[v]",
        "-map",
        "[a]",
        "-frames:v",
        &total_frames.to_string(),
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "18",
        "-pix_fmt",
        "yuv420p",
    ]);
    if intermediate {
        cmd.args(["-c:a", "pcm_s16le"]);
    } else {
        cmd.args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]);
    }
    let output = cmd
        .arg(pending)
        .output()
        .map_err(|e| ops_api::validation(format!("ffmpeg is required for export: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ops_api::validation(format!(
            "movie export failed: {}",
            stderr.chars().take(1200).collect::<String>()
        )));
    }
    if !pending.metadata().is_ok_and(|m| m.len() > 0) {
        return Err(ops_api::validation("ffmpeg produced an empty movie"));
    }
    Ok(())
}

fn join_chunks(
    scratch: &Path,
    chunks: &[(String, u64)],
    pending: &Path,
    total_frames: u64,
    fps: u32,
) -> Result<(), ApiError> {
    let mut list = String::from("ffconcat version 1.0\n");
    for (file, frames) in chunks {
        list.push_str(&format!(
            "file {file}\nduration {:.9}\n",
            *frames as f64 / f64::from(fps)
        ));
    }
    let manifest = scratch.join("chunks.ffconcat");
    fs::write(&manifest, list)
        .map_err(|e| ops_api::validation(format!("writing chunk list: {e}")))?;
    let output = Command::new("ffmpeg")
        .current_dir(scratch)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-safe",
            "1",
            "-i",
            "chunks.ffconcat",
        ])
        .args([
            "-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
        ])
        .args([
            "-frames:v",
            &total_frames.to_string(),
            "-movflags",
            "+faststart",
        ])
        .arg(pending)
        .output()
        .map_err(|e| ops_api::validation(format!("ffmpeg is required for export: {e}")))?;
    if !output.status.success() {
        return Err(ops_api::validation(format!(
            "joining movie chunks failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1200)
                .collect::<String>()
        )));
    }
    if !pending
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() > 0)
    {
        return Err(ops_api::validation(
            "joining movie chunks produced an empty file",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use storyboard_core::{Author, Format, Op};
    use storyboard_store::{commit, create, open};

    #[test]
    fn export_uses_the_version_that_was_checked_before_a_saved_edit() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("movie");
        let before = create(
            &dir,
            "Movie",
            24,
            Format {
                width: 320,
                height: 180,
                extra: Default::default(),
            },
        )
        .unwrap();
        commit(
            &dir,
            &before.version,
            vec![
                Op::AddScene {
                    id: Some("scene-1".into()),
                    title: "Scene".into(),
                    index: None,
                },
                Op::AddShot {
                    scene: "scene-1".into(),
                    id: Some("shot-1".into()),
                    title: "Shot".into(),
                    index: None,
                },
            ],
            Author::Human,
            None,
        )
        .unwrap();
        let after = open(&dir).unwrap();
        assert_ne!(after.version, before.version);
        assert_eq!(compile_active(&after.project).unwrap().frames, 48);
        assert!(matches!(
            export_loaded(&before, None, ImageMode::Boards),
            Err(ApiError::Validation { message, .. }) if message == "nothing to export in this cut"
        ));
        assert!(!dir.join(".cache/exports").exists());
    }
}
