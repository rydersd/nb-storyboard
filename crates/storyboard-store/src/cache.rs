use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::project::StoreError;

/// Stores a derived value under its content hash, so identical render inputs share a cache entry.
pub fn cache_put(dir: &Path, bytes: &[u8]) -> Result<PathBuf, StoreError> {
    let key = hex(Sha256::digest(bytes));
    let cache = dir.join(".cache/content");
    fs::create_dir_all(&cache)?;
    let path = cache.join(&key);
    if !path.exists() {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    Ok(PathBuf::from(".cache/content").join(key))
}

/// Removes oldest derived entries until at most `keep` remain. Project data and source files are untouched.
pub fn prune_cache(dir: &Path, keep: usize) -> Result<usize, StoreError> {
    let cache = dir.join(".cache/content");
    if !cache.is_dir() {
        return Ok(0);
    }
    let mut files: Vec<_> = fs::read_dir(&cache)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .metadata()
                .ok()
                .filter(|metadata| metadata.is_file())
                .map(|metadata| (metadata.modified().ok(), entry.path()))
        })
        .collect();
    files.sort_by_key(|(modified, _)| *modified);
    let remove = files.len().saturating_sub(keep);
    for (_, path) in files.into_iter().take(remove) {
        fs::remove_file(path)?;
    }
    Ok(remove)
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
