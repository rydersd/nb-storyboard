use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use storyboard_core::{Author, Op};

use crate::project::{StoreError, ensure_real_directory, reject_symlink};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitRecord {
    pub version: String,
    pub parent: String,
    pub at: String,
    pub author: Author,
    pub ops: Vec<Op>,
    pub inverses: Vec<Op>,
    pub footprints: Vec<String>,
    pub rebased: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitSummary {
    pub version: String,
    pub parent: String,
    pub at: String,
    pub author: Author,
    pub summary: String,
    pub external: bool,
}

impl From<&CommitRecord> for CommitSummary {
    fn from(value: &CommitRecord) -> Self {
        Self {
            version: value.version.clone(),
            parent: value.parent.clone(),
            at: value.at.clone(),
            author: value.author.clone(),
            summary: value.summary.clone(),
            external: value.external,
        }
    }
}

pub fn history(dir: &Path, limit: usize) -> Result<Vec<CommitRecord>, StoreError> {
    let mut records = read_records(dir)?;
    records.reverse();
    records.truncate(limit);
    Ok(records)
}

pub fn records_since(dir: &Path, version: &str) -> Result<Option<Vec<CommitRecord>>, StoreError> {
    Ok(records_since_from(&read_records(dir)?, version))
}

pub(crate) fn records_since_from(
    records: &[CommitRecord],
    version: &str,
) -> Option<Vec<CommitRecord>> {
    if records
        .first()
        .is_some_and(|record| record.parent == version)
    {
        return Some(records.to_vec());
    }
    records
        .iter()
        .position(|record| record.version == version)
        .map(|index| records[index + 1..].to_vec())
}

pub(crate) fn read_records(dir: &Path) -> Result<Vec<CommitRecord>, StoreError> {
    let history_dir = dir.join("history");
    match history_dir.symlink_metadata() {
        Ok(_) => ensure_real_directory(&history_dir)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StoreError::Io(error)),
    }
    let path = history_dir.join("log.jsonl");
    match path.symlink_metadata() {
        Ok(_) => reject_symlink(&path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StoreError::Io(error)),
    }
    let bytes = fs::read(&path)?;
    let lines: Vec<_> = bytes.split(|byte| *byte == b'\n').collect();
    let mut records = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        if index + 1 == lines.len() && !bytes.ends_with(b"\n") {
            break;
        }
        match serde_json::from_slice(line) {
            Ok(record) => records.push(record),
            Err(error) => {
                return Err(StoreError::InvalidJson {
                    path: path.clone(),
                    message: error.to_string(),
                });
            }
        }
    }
    Ok(records)
}

pub(crate) fn append_record(dir: &Path, record: &CommitRecord) -> Result<(), StoreError> {
    let history_dir = dir.join("history");
    ensure_real_directory(&history_dir)?;
    let path = history_dir.join("log.jsonl");
    reject_symlink(&path)?;
    let mut bytes = serde_json::to_vec(record).map_err(|error| StoreError::InvalidJson {
        path: path.clone(),
        message: error.to_string(),
    })?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn repair_log_tail_for_dir(dir: &Path) -> Result<(), StoreError> {
    let path = dir.join("history/log.jsonl");
    match path.symlink_metadata() {
        Ok(_) => {
            reject_symlink(&path)?;
            repair_log_tail(&path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Io(error)),
    }
}

fn repair_log_tail(path: &Path) -> Result<(), StoreError> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(StoreError::Io(error)),
    };
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.read_to_end(&mut bytes)?;
    if bytes.ends_with(b"\n") {
        return Ok(());
    }
    let start = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if serde_json::from_slice::<CommitRecord>(&bytes[start..]).is_ok() {
        file.seek(SeekFrom::End(0))?;
        file.write_all(b"\n")?;
    } else {
        file.set_len(start as u64)?;
    }
    file.sync_all()?;
    Ok(())
}
