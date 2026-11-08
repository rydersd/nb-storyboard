use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use crate::project::{StoreError, reject_symlink};

pub(crate) struct ProjectLock(File);

impl ProjectLock {
    pub(crate) fn acquire(dir: &Path) -> io::Result<Self> {
        let path = dir.join(".lock");
        reject_symlink(&path).map_err(store_error_to_io)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.lock()?;
        Ok(Self(file))
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn store_error_to_io(error: StoreError) -> io::Error {
    match error {
        StoreError::Io(error) => error,
        other => io::Error::other(other.to_string()),
    }
}
