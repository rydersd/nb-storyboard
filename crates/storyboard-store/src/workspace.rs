use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[cfg(feature = "render")]
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage, imageops};

use crate::history::history;
use crate::project::{
    PROJECT_FILE, StoreError, ensure_owned_directory, ensure_real_directory,
    read_project_unvalidated, reject_symlink,
};

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub projects: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub path: PathBuf,
    pub title: String,
    pub scene_count: usize,
    pub shot_count: usize,
    pub duration: u64,
    pub unresolved_stickies: usize,
    pub last_author: Option<String>,
    pub aspect: String,
    pub modified: String,
    pub thumbnail: Option<PathBuf>,
    /// Why the project could not be read; the other fields are then placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedIndex {
    size: u64,
    modified: String,
    entry: IndexEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thumbnail_inputs: Option<ThumbnailInputs>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ThumbnailInputs {
    panel_hash: String,
    sources: Vec<String>,
    key: String,
}

pub fn open_workspace(root: &Path) -> Result<Workspace, StoreError> {
    let mut projects = Vec::new();
    for item in fs::read_dir(root)? {
        let item = item?;
        let path = item.path();
        if item.file_type()?.is_dir() && path.join(PROJECT_FILE).is_file() {
            projects.push(path);
        }
    }
    projects.sort();
    Ok(Workspace {
        root: root.to_path_buf(),
        projects,
    })
}

pub fn index(workspace: &Workspace) -> Result<Vec<IndexEntry>, StoreError> {
    let mut entries = Vec::new();
    for path in &workspace.projects {
        match index_project(&workspace.root, path) {
            Ok(entry) => entries.push(entry),
            // Skip only a project whose document vanished mid-scan; any other failure is listed.
            Err(StoreError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound
                    && !path.join(PROJECT_FILE).exists() => {}
            Err(error) => entries.push(broken_entry(&workspace.root, path, &error)),
        }
    }
    Ok(entries)
}

/// One unreadable project is listed with its problem, so it never hides the rest.
fn broken_entry(root: &Path, dir: &Path, error: &StoreError) -> IndexEntry {
    let path = dir.strip_prefix(root).unwrap_or(dir).to_path_buf();
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified = dir
        .join(PROJECT_FILE)
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(format_system_time)
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
    IndexEntry {
        path,
        title,
        scene_count: 0,
        shot_count: 0,
        duration: 0,
        unresolved_stickies: 0,
        last_author: None,
        aspect: String::new(),
        modified,
        thumbnail: None,
        problem: Some(format!("{}: {error}", PROJECT_FILE)),
    }
}

fn index_project(root: &Path, dir: &Path) -> Result<IndexEntry, StoreError> {
    let document = dir.join(PROJECT_FILE);
    let metadata = document.metadata()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(format_system_time)
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
    let path = dir
        .strip_prefix(root)
        .map_err(|_| StoreError::Io(std::io::Error::other("project is outside workspace")))?
        .to_path_buf();
    if let Some(mut entry) = read_cached(dir, metadata.len(), &modified)? {
        entry.path = path;
        return Ok(entry);
    }
    let (project, _) = read_project_unvalidated(dir)?;
    let shot_count = project.scenes.iter().map(|scene| scene.shots.len()).sum();
    let duration = project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .map(|shot| (shot.duration * f64::from(project.fps)).round() as u64)
        .sum();
    let last_author = history(dir, 1)?.first().map(|record| match &record.author {
        storyboard_core::Author::Human => "human".into(),
        storyboard_core::Author::Agent { agent, .. } => agent.name.clone(),
    });
    let (thumbnail, thumbnail_inputs) = thumbnail_for(dir, &project)?;
    let entry = IndexEntry {
        path,
        title: project.title,
        scene_count: project.scenes.len(),
        shot_count,
        duration,
        unresolved_stickies: project
            .stickies
            .iter()
            .filter(|sticky| !sticky.resolved)
            .count(),
        last_author,
        aspect: format_aspect(project.format.width, project.format.height),
        modified: modified.clone(),
        thumbnail,
        problem: None,
    };
    write_cached(dir, metadata.len(), &modified, &entry, thumbnail_inputs)?;
    Ok(entry)
}

fn read_cached(dir: &Path, size: u64, modified: &str) -> Result<Option<IndexEntry>, StoreError> {
    let cache = dir.join(".cache");
    match cache.symlink_metadata() {
        // A fresh clone has no .cache (it is git-ignored): a cold miss, not a vanished project.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => ensure_real_directory(&cache)?,
    }
    let path = dir.join(".cache/index.json");
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "refusing symlinked workspace index",
            )));
        }
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::Io(error)),
    }
    let cached: CachedIndex = match serde_json::from_slice(&fs::read(&path)?) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if cached.size != size || cached.modified != modified {
        return Ok(None);
    }
    if let Some(inputs) = &cached.thumbnail_inputs
        && thumbnail_key(dir, &inputs.panel_hash, &inputs.sources) != inputs.key
    {
        return Ok(None);
    }
    if let Some(thumbnail) = &cached.entry.thumbnail {
        let thumbnail = dir.join(thumbnail);
        match thumbnail.symlink_metadata() {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::Io(error)),
        }
    }
    Ok(Some(cached.entry))
}
fn write_cached(
    dir: &Path,
    size: u64,
    modified: &str,
    entry: &IndexEntry,
    thumbnail_inputs: Option<ThumbnailInputs>,
) -> Result<(), StoreError> {
    let cache = dir.join(".cache");
    ensure_owned_directory(&cache)?;
    let path = cache.join("index.json");
    reject_symlink(&path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)?;
    serde_json::to_writer(
        &mut file,
        &CachedIndex {
            size,
            modified: modified.into(),
            entry: entry.clone(),
            thumbnail_inputs,
        },
    )
    .map_err(|error| StoreError::InvalidJson {
        path,
        message: error.to_string(),
    })?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
fn format_aspect(width: u32, height: u32) -> String {
    let divisor = gcd(width, height);
    format!("{}:{}", width / divisor, height / divisor)
}
fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}
#[cfg(feature = "render")]
fn first_non_empty_panel(project: &storyboard_core::Project) -> Option<&storyboard_core::Panel> {
    for shot in project.scenes.iter().flat_map(|scene| &scene.shots) {
        let take = shot
            .takes
            .iter()
            .find(|take| take.id == shot.selected_take)?;
        if let Some(panel) = take.panels.iter().find(|panel| {
            !panel.layers.is_empty()
                || !panel.strokes.is_empty()
                || !panel.title.is_empty()
                || !panel.direction.is_empty()
        }) {
            return Some(panel);
        }
    }
    None
}

fn format_system_time(value: std::time::SystemTime) -> Option<String> {
    OffsetDateTime::from(value).format(&Rfc3339).ok()
}

fn thumbnail_key(dir: &Path, panel_hash: &str, sources: &[String]) -> String {
    let mut hash = Sha256::new();
    hash.update(panel_hash.as_bytes());
    for source in sources {
        hash.update(source.as_bytes());
        match safe_source_metadata(dir, source) {
            Ok(metadata) => {
                hash.update(metadata.len().to_le_bytes());
                if let Ok(modified) = metadata.modified()
                    && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
                {
                    hash.update(duration.as_nanos().to_le_bytes());
                }
            }
            Err(_) => hash.update(b"missing"),
        }
    }
    hex(hash.finalize())
}

#[cfg(not(feature = "render"))]
fn thumbnail_for(
    dir: &Path,
    _project: &storyboard_core::Project,
) -> Result<(Option<PathBuf>, Option<ThumbnailInputs>), StoreError> {
    prune_render_cache(dir, None)?;
    Ok((None, None))
}

#[cfg(feature = "render")]
fn thumbnail_for(
    dir: &Path,
    project: &storyboard_core::Project,
) -> Result<(Option<PathBuf>, Option<ThumbnailInputs>), StoreError> {
    let Some(panel) = first_non_empty_panel(project) else {
        prune_render_cache(dir, None)?;
        return Ok((None, None));
    };
    let panel_bytes = serde_json::to_vec(panel).map_err(|error| StoreError::InvalidJson {
        path: dir.join("project.json"),
        message: error.to_string(),
    })?;
    let panel_hash = hex(Sha256::digest(panel_bytes));
    let mut sources: Vec<_> = panel
        .layers
        .iter()
        .filter_map(|layer| layer.src.clone())
        .collect();
    sources.sort();
    sources.dedup();
    let key = thumbnail_key(dir, &panel_hash, &sources);
    let relative = PathBuf::from(".cache/render").join(format!("{key}.png"));
    let absolute = dir.join(&relative);
    let render_cache = absolute.parent().expect("render cache has a parent");
    match render_cache.symlink_metadata() {
        Ok(_) => ensure_real_directory(render_cache)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(render_cache)?,
        Err(error) => return Err(StoreError::Io(error)),
    }
    reject_symlink(&absolute)?;
    if !absolute.is_file() {
        let bytes = render_thumbnail(project, dir, panel)?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&absolute)
        {
            Ok(mut file) => {
                file.write_all(&bytes)?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(StoreError::Io(error)),
        }
    }
    prune_render_cache(dir, Some(&key))?;
    Ok((
        Some(relative),
        Some(ThumbnailInputs {
            panel_hash,
            sources,
            key,
        }),
    ))
}

fn prune_render_cache(dir: &Path, keep: Option<&str>) -> Result<(), StoreError> {
    let cache = dir.join(".cache/render");
    let entries = match fs::read_dir(&cache) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(StoreError::Io(error)),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let retained = keep.is_some_and(|key| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == format!("{key}.png"))
        });
        if !retained && entry.file_type()?.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(feature = "render")]
fn render_thumbnail(
    project: &storyboard_core::Project,
    dir: &Path,
    panel: &storyboard_core::Panel,
) -> Result<Vec<u8>, StoreError> {
    let width = 320;
    let height = (width * project.format.height / project.format.width).max(1);
    let background = parse_hex(&panel.background).unwrap_or(Rgba([240, 240, 240, 255]));
    let mut canvas = RgbaImage::from_pixel(width, height, background);
    let sx = f64::from(width) / f64::from(project.format.width);
    let sy = f64::from(height) / f64::from(project.format.height);
    for layer in &panel.layers {
        if layer.hidden {
            continue;
        }
        let x = (layer.x * sx).max(0.0) as u32;
        let y = (layer.y * sy).max(0.0) as u32;
        let w = (layer.width * sx).max(1.0) as u32;
        let h = (layer.height * sy).max(1.0) as u32;
        if layer.layer_type == "image"
            && let Some(source) = &layer.src
            && safe_source_metadata(dir, source).is_ok()
            && let Ok(image) = image::open(dir.join(source))
        {
            let image = image
                .resize_exact(w, h, imageops::FilterType::Triangle)
                .to_rgba8();
            imageops::overlay(&mut canvas, &image, i64::from(x), i64::from(y));
            continue;
        }
        let color = layer
            .fill
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Rgba([96, 96, 96, 255]));
        fill_rect(&mut canvas, x, y, w, h, color);
    }
    for stroke in &panel.strokes {
        let color = parse_hex(&stroke.color).unwrap_or(Rgba([0, 0, 0, 255]));
        for points in stroke.points.windows(2) {
            draw_line(
                &mut canvas,
                (points[0][0] * sx) as i32,
                (points[0][1] * sy) as i32,
                (points[1][0] * sx) as i32,
                (points[1][1] * sy) as i32,
                color,
            );
        }
    }
    let mut cursor = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|error| StoreError::Io(std::io::Error::other(error)))?;
    Ok(cursor.into_inner())
}

#[cfg(feature = "render")]
fn parse_hex(value: &str) -> Option<Rgba<u8>> {
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    Some(Rgba([
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
        255,
    ]))
}

#[cfg(feature = "render")]
fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for py in y..y.saturating_add(height).min(image.height()) {
        for px in x..x.saturating_add(width).min(image.width()) {
            image.put_pixel(px, py, color);
        }
    }
}

#[cfg(feature = "render")]
fn draw_line(image: &mut RgbaImage, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: Rgba<u8>) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        if x0 >= 0 && y0 >= 0 && x0 < image.width() as i32 && y0 < image.height() as i32 {
            image.put_pixel(x0 as u32, y0 as u32, color);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn safe_source_metadata(dir: &Path, source: &str) -> std::io::Result<fs::Metadata> {
    let relative = Path::new(source);
    if relative.is_absolute()
        || !relative.starts_with("sources")
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "thumbnail source is outside sources/",
        ));
    }
    let mut current = dir.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        current.push(component);
        let metadata = current.symlink_metadata()?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "thumbnail source contains a symlink",
            ));
        }
    }
    current.metadata()
}
