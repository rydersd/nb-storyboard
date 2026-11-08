use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use storyboard_core::Project;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::project::{
    StoreError, ensure_owned_directory, ensure_real_directory, read_project_unvalidated,
    reject_symlink,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDimensions {
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceUse {
    pub target: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntry {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub modified: String,
    pub dimensions: Option<SourceDimensions>,
    pub uses: Vec<SourceUse>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingRef {
    pub path: String,
    pub target: String,
}

pub fn scan_sources(dir: &Path) -> Result<Vec<SourceEntry>, StoreError> {
    let sources = dir.join("sources");
    ensure_real_directory(&sources)?;
    let project = read_project_unvalidated(dir)?.0;
    let mut entries = Vec::new();
    scan_dir(dir, &sources, &project, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let cache = dir.join(".cache");
    ensure_owned_directory(&cache)?;
    let path = cache.join("sources.json");
    reject_symlink(&path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, &entries).map_err(|error| StoreError::InvalidJson {
        path: dir.join(".cache/sources.json"),
        message: error.to_string(),
    })?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(entries)
}

pub fn import_source(dir: &Path, source: &Path, destination: &Path) -> Result<PathBuf, StoreError> {
    let relative = checked_source_path(destination)?;
    let target = dir.join(&relative);
    let parent = target.parent().ok_or_else(|| {
        StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source destination has no parent",
        ))
    })?;
    ensure_destination_parent(dir, parent)?;
    let source_metadata = source.symlink_metadata()?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
        return Err(StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source must be a regular file",
        )));
    }
    let mut input = fs::File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        drop(output);
        let _ = fs::remove_file(&target);
        return Err(StoreError::Io(error));
    }
    output.sync_all()?;
    fs::File::open(parent)?.sync_all()?;
    Ok(relative)
}

pub fn check_refs(dir: &Path, project: &Project) -> Result<Vec<MissingRef>, StoreError> {
    let mut missing = Vec::new();
    for scene in &project.scenes {
        for shot in &scene.shots {
            check_target_refs(dir, &shot.refs, format!("shot:{}", shot.id), &mut missing)?;
            for take in &shot.takes {
                check_target_refs(
                    dir,
                    &take.refs,
                    format!("take:{}/{}", shot.id, take.id),
                    &mut missing,
                )?;
            }
        }
    }
    Ok(missing)
}

fn scan_dir(
    root: &Path,
    directory: &Path,
    project: &Project,
    entries: &mut Vec<SourceEntry>,
) -> Result<(), StoreError> {
    for item in fs::read_dir(directory)? {
        let item = item?;
        let path = item.path();
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            scan_dir(root, &path, project, entries)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| StoreError::Io(io::Error::other("source escaped project")))?;
        let relative = relative_to_string(relative)?;
        let dimensions = image_dimensions(&path, &relative);
        entries.push(SourceEntry {
            path: relative.clone(),
            kind: kind_for(&relative).into(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(format_system_time)
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".into()),
            dimensions,
            uses: uses_for(project, &relative),
        });
    }
    Ok(())
}

fn check_target_refs(
    dir: &Path,
    refs: &[String],
    target: String,
    missing: &mut Vec<MissingRef>,
) -> Result<(), StoreError> {
    for reference in refs {
        let Ok(relative) = checked_source_path(Path::new(reference)) else {
            missing.push(MissingRef {
                path: reference.clone(),
                target: target.clone(),
            });
            continue;
        };
        let path = dir.join(&relative);
        let present = path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
        if !present {
            missing.push(MissingRef {
                path: reference.clone(),
                target: target.clone(),
            });
        }
    }
    Ok(())
}

fn uses_for(project: &Project, path: &str) -> Vec<SourceUse> {
    let mut uses = Vec::new();
    for scene in &project.scenes {
        for shot in &scene.shots {
            if shot.refs.iter().any(|reference| reference == path) {
                uses.push(SourceUse {
                    target: format!("shot:{}", shot.id),
                });
            }
            for take in &shot.takes {
                if take.refs.iter().any(|reference| reference == path) {
                    uses.push(SourceUse {
                        target: format!("take:{}/{}", shot.id, take.id),
                    });
                }
            }
        }
    }
    uses
}

fn image_dimensions(path: &Path, relative: &str) -> Option<SourceDimensions> {
    if !matches!(kind_for(relative), "image") {
        return None;
    }
    if Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
    {
        return svg_dimensions(path);
    }
    image::image_dimensions(path)
        .ok()
        .map(|(width, height)| SourceDimensions { width, height })
}
fn svg_dimensions(path: &Path) -> Option<SourceDimensions> {
    let mut file = fs::File::open(path).ok()?;
    let mut bytes = vec![0; 8192];
    let count = file.read(&mut bytes).ok()?;
    let text = std::str::from_utf8(&bytes[..count]).ok()?;
    let svg = text.find("<svg")?;
    let tag = &text[svg..text.find('>').unwrap_or(text.len())];
    let number = |name: &str| -> Option<u32> {
        let value = tag
            .split_once(&format!("{name}=\""))?
            .1
            .split_once('"')?
            .0
            .trim_end_matches("px");
        value.parse().ok()
    };
    Some(SourceDimensions {
        width: number("width")?,
        height: number("height")?,
    })
}
fn kind_for(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "tif" | "tiff" | "svg") => "image",
        _ => "file",
    }
}
fn checked_source_path(value: &Path) -> Result<PathBuf, StoreError> {
    if value.is_absolute()
        || !value.starts_with("sources")
        || value.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path must stay below sources/",
        )));
    }
    Ok(value.to_path_buf())
}

fn ensure_destination_parent(dir: &Path, parent: &Path) -> Result<(), StoreError> {
    let sources = dir.join("sources");
    ensure_real_directory(&sources)?;
    let relative = parent.strip_prefix(&sources).map_err(|_| {
        StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source destination escaped sources/",
        ))
    })?;
    let mut current = sources;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(StoreError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid source destination component",
            )));
        };
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(StoreError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "source destination component is not a real directory: {}",
                        current.display()
                    ),
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(error) => return Err(StoreError::Io(error)),
        }
    }
    Ok(())
}

fn format_system_time(value: std::time::SystemTime) -> Option<String> {
    OffsetDateTime::from(value).format(&Rfc3339).ok()
}
fn relative_to_string(path: &Path) -> Result<String, StoreError> {
    let text = path.to_str().ok_or_else(|| {
        StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "source path is not UTF-8",
        ))
    })?;
    Ok(text.replace(std::path::MAIN_SEPARATOR, "/"))
}
