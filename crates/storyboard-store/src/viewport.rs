use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use storyboard_core::Project;

use crate::project::{
    StoreError, ensure_owned_directory, ensure_real_directory, read_project_unvalidated,
    reject_symlink,
};

pub const MAX_VIEWPORT_BYTES: usize = 4 * 1024;
const MAX_PAN: f64 = 1_000_000.0;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub pan_x: f64,
    pub pan_y: f64,
    pub zoom: f64,
    pub viewed_cut: Option<String>,
}

pub fn validate_viewport(viewport: &Viewport, project: &Project) -> Result<(), String> {
    if !viewport.pan_x.is_finite()
        || !viewport.pan_y.is_finite()
        || viewport.pan_x.abs() > MAX_PAN
        || viewport.pan_y.abs() > MAX_PAN
    {
        return Err(
            "viewport pan_x and pan_y must be finite and between -1000000 and 1000000".into(),
        );
    }
    if !viewport.zoom.is_finite() || !(0.1..=4.0).contains(&viewport.zoom) {
        return Err("viewport zoom must be a finite number from 0.1 to 4".into());
    }
    if let Some(cut) = &viewport.viewed_cut
        && !project.cuts.iter().any(|candidate| candidate.id == *cut)
    {
        return Err(format!("viewport viewed_cut {cut:?} is not a known cut"));
    }
    let bytes = serde_json::to_vec(viewport).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_VIEWPORT_BYTES {
        return Err(format!(
            "viewport JSON must be at most {MAX_VIEWPORT_BYTES} bytes"
        ));
    }
    Ok(())
}

pub fn read_viewport(dir: &Path) -> Result<Option<Viewport>, StoreError> {
    let cache = dir.join(".cache");
    match cache.symlink_metadata() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        _ => ensure_real_directory(&cache)?,
    }
    let path = cache.join("viewport.json");
    reject_symlink(&path)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) if bytes.len() <= MAX_VIEWPORT_BYTES => bytes,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let viewport: Viewport = match serde_json::from_slice(&bytes) {
        Ok(viewport) => viewport,
        Err(_) => return Ok(None),
    };
    let (project, _) = read_project_unvalidated(dir)?;
    if validate_viewport(&viewport, &project).is_err() {
        return Ok(None);
    }
    Ok(Some(viewport))
}

pub fn write_viewport(dir: &Path, viewport: &Viewport) -> Result<(), StoreError> {
    let (project, _) = read_project_unvalidated(dir)?;
    validate_viewport(viewport, &project)
        .map_err(|message| StoreError::Io(io::Error::new(io::ErrorKind::InvalidInput, message)))?;
    let mut bytes =
        serde_json::to_vec_pretty(viewport).map_err(|error| StoreError::InvalidJson {
            path: dir.join(".cache/viewport.json"),
            message: error.to_string(),
        })?;
    bytes.push(b'\n');

    let cache = dir.join(".cache");
    ensure_owned_directory(&cache)?;
    let path = cache.join("viewport.json");
    reject_symlink(&path)?;
    let temporary = cache.join(format!(
        "viewport.json.tmp-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    reject_symlink(&temporary)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        fs::File::open(&cache)?.sync_all()?;
        Ok::<(), StoreError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
