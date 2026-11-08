use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::project::{StoreError, ensure_real_directory};

/// Stores a derived value under its content hash, so identical render inputs share a cache entry.
pub fn cache_put(dir: &Path, bytes: &[u8]) -> Result<PathBuf, StoreError> {
    let key = hex(Sha256::digest(bytes));
    let cache_root = dir.join(".cache");
    ensure_cache_directory(&cache_root)?;
    let cache = cache_root.join("content");
    ensure_cache_directory(&cache)?;
    let path = cache.join(&key);
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cache entry must be a regular file",
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Err(error) => return Err(StoreError::Io(error)),
    }
    Ok(PathBuf::from(".cache/content").join(key))
}

/// Removes oldest derived entries until at most `keep` remain. Project data and source files are untouched.
pub fn prune_cache(dir: &Path, keep: usize) -> Result<usize, StoreError> {
    let cache_root = dir.join(".cache");
    if !real_directory_or_missing(&cache_root)? {
        return Ok(0);
    }
    let cache = cache_root.join("content");
    if !real_directory_or_missing(&cache)? {
        return Ok(0);
    }
    let mut files: Vec<_> = fs::read_dir(&cache)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .path()
                .symlink_metadata()
                .ok()
                .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
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

fn ensure_cache_directory(path: &Path) -> Result<(), StoreError> {
    match path.symlink_metadata() {
        Ok(_) => ensure_real_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            Ok(())
        }
        Err(error) => Err(StoreError::Io(error)),
    }
}

fn real_directory_or_missing(path: &Path) -> Result<bool, StoreError> {
    match path.symlink_metadata() {
        Ok(_) => {
            ensure_real_directory(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(StoreError::Io(error)),
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
