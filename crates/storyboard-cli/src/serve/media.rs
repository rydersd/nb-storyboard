//! Stream an imported MP4 with byte ranges for browser playback and seeking.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::Command,
};

use serde_json::{Value, json};
use tiny_http::{Request, Response, StatusCode};

use super::http::{header, header_value, text_response};
use super::{State, project_dir};
use crate::ops_api::{self, ApiError};

pub(super) fn movie(state: &State, query: &HashMap<String, String>, request: Request) {
    stream(
        state,
        query,
        request,
        "imported-movies",
        "original.mp4",
        "video/mp4",
    );
}

pub(super) fn audio(state: &State, query: &HashMap<String, String>, request: Request) {
    let ext = query.get("ext").map(String::as_str).unwrap_or("");
    let kind = match ext {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        _ => {
            let _ = request.respond(text_response(404, "invalid audio type"));
            return;
        }
    };
    stream(
        state,
        query,
        request,
        "imported-audio",
        &format!("original.{ext}"),
        kind,
    );
}

pub(super) fn exported_movie(state: &State, query: &HashMap<String, String>, request: Request) {
    let result = (|| {
        let dir = project_dir(state, query).map_err(|_| "project not found".to_owned())?;
        let name = query.get("file").ok_or("export file is required")?;
        if name.len() > 128
            || !name.ends_with(".mp4")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("invalid export file".to_owned());
        }
        let path = dir.join(".cache/exports").join(name);
        for parent in [dir.join(".cache"), dir.join(".cache/exports")] {
            let meta = parent.symlink_metadata().map_err(|_| "export not found")?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("export folder is invalid".to_owned());
            }
        }
        let meta = path.symlink_metadata().map_err(|_| "export not found")?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("export is invalid".to_owned());
        }
        Ok((path, name.clone()))
    })();
    match result {
        Ok((path, name)) => stream_file(request, path, "video/mp4", Some(&name)),
        Err(error) => {
            let _ = request.respond(text_response(404, &error));
        }
    }
}

/// A compact peak envelope for a visible timeline segment. The browser never downloads or
/// decodes an entire movie merely to draw a small audio waveform.
pub(super) fn waveform(state: &State, query: &HashMap<String, String>) -> Result<Value, ApiError> {
    let (collection, name) = match query.get("kind").map(String::as_str) {
        Some("movie") => ("imported-movies", "original.mp4".to_owned()),
        Some("audio") => {
            let ext = query.get("ext").map(String::as_str).unwrap_or("");
            if !["mp3", "wav", "m4a"].contains(&ext) {
                return Err(ops_api::validation("invalid audio type"));
            }
            ("imported-audio", format!("original.{ext}"))
        }
        _ => return Err(ops_api::validation("invalid waveform source")),
    };
    let path = source_path(state, query, collection, &name).map_err(ops_api::validation)?;
    let time = |key: &str| -> Result<f64, ApiError> {
        query
            .get(key)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 3600.0)
            .ok_or_else(|| ops_api::validation(format!("invalid waveform {key}")))
    };
    let start = time("start")?;
    let end = time("end")?;
    if end <= start || end - start > 300.0 {
        return Err(ops_api::validation(
            "waveform range must be above 0 and at most five minutes",
        ));
    }
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &start.to_string(),
            "-i",
        ])
        .arg(&path)
        .args([
            "-t",
            &(end - start).to_string(),
            "-map",
            "0:a:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "4000",
            "-f",
            "s16le",
            "pipe:1",
        ])
        .output()
        .map_err(|error| {
            ops_api::validation(format!("ffmpeg is required for waveforms: {error}"))
        })?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(ops_api::validation(
            "source has no decodable audio in this range",
        ));
    }
    let samples: Vec<i16> = output
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes))
        .collect();
    let mut peaks = vec![0u16; 128];
    for (index, sample) in samples.iter().enumerate() {
        let bin = (index * peaks.len() / samples.len()).min(peaks.len() - 1);
        peaks[bin] = peaks[bin].max(sample.unsigned_abs());
    }
    Ok(json!({"peaks": peaks}))
}

fn source_path(
    state: &State,
    query: &HashMap<String, String>,
    collection: &str,
    name: &str,
) -> Result<PathBuf, String> {
    let dir = project_dir(state, query).map_err(|_| "project not found".to_owned())?;
    let folder = query.get("folder").ok_or("folder is required")?;
    let uuid_folder = folder.len() == 36
        && folder
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-');
    let legacy_agent_folder = collection == "imported-movies"
        && folder.strip_prefix("agent-").is_some_and(|rest| {
            rest.len() <= 48
                && rest.contains('-')
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-')
        });
    if !uuid_folder && !legacy_agent_folder {
        return Err("invalid source folder".to_owned());
    }
    let relative = format!("sources/{collection}/{folder}");
    let collection_dir = format!("sources/{collection}");
    for item in ["sources", collection_dir.as_str(), relative.as_str()] {
        let meta = dir
            .join(item)
            .symlink_metadata()
            .map_err(|_| "media source not found")?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("media source folder is invalid".to_owned());
        }
    }
    let path = dir.join(relative).join(name);
    let meta = path
        .symlink_metadata()
        .map_err(|_| "media source not found")?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("media source is invalid".to_owned());
    }
    Ok(path)
}

fn stream(
    state: &State,
    query: &HashMap<String, String>,
    request: Request,
    collection: &str,
    name: &str,
    kind: &str,
) {
    match source_path(state, query, collection, name) {
        Ok(path) => stream_file(request, path, kind, None),
        Err(error) => {
            let _ = request.respond(text_response(404, &error));
        }
    }
}

fn stream_file(request: Request, path: PathBuf, kind: &str, download: Option<&str>) {
    let result = (|| {
        let meta = path.metadata().map_err(|error| error.to_string())?;
        let mut file = File::open(path).map_err(|error| error.to_string())?;
        let size = meta.len();
        let range = header_value(&request, "Range").and_then(|value| value.strip_prefix("bytes="));
        let (start, end, partial) = if let Some(range) = range {
            let (first, last) = range.split_once('-').ok_or("invalid range")?;
            let start: u64 = first.parse().map_err(|_| "invalid range")?;
            let end: u64 = if last.is_empty() {
                size.saturating_sub(1)
            } else {
                last.parse().map_err(|_| "invalid range")?
            };
            if start >= size || end < start || end >= size {
                return Err("range outside movie".to_owned());
            }
            (start, end, true)
        } else {
            (0, size.saturating_sub(1), false)
        };
        file.seek(SeekFrom::Start(start))
            .map_err(|error| error.to_string())?;
        let length = if size == 0 { 0 } else { end - start + 1 };
        let mut headers = vec![
            header("Content-Type", kind),
            header("Accept-Ranges", "bytes"),
            header("Cache-Control", "no-store"),
        ];
        if let Some(name) = download {
            headers.push(header(
                "Content-Disposition",
                &format!("attachment; filename=\"{name}\""),
            ));
        }
        if partial {
            headers.push(header(
                "Content-Range",
                &format!("bytes {start}-{end}/{size}"),
            ));
        }
        let response = Response::new(
            StatusCode(if partial { 206 } else { 200 }),
            headers,
            file.take(length),
            usize::try_from(length).ok(),
            None,
        );
        Ok(response)
    })();
    match result {
        Ok(response) => {
            let _ = request.respond(response);
        }
        Err(error) => {
            let _ = request.respond(text_response(404, &error));
        }
    }
}
