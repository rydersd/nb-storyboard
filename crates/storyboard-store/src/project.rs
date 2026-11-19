use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use storyboard_core::{
    Author, Format, Layout, Op, OpError, Project, ValidationIssue, apply, footprint::disjoint,
    validate,
};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::history::{
    CommitRecord, CommitSummary, append_record, read_records, records_since_from,
    repair_log_tail_for_dir,
};
use crate::lock::ProjectLock;
use crate::version::version_of;

pub(crate) const PROJECT_FILE: &str = "project.json";

#[derive(Debug)]
pub struct Loaded {
    pub project: Project,
    pub version: String,
    pub dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not a project: {0}")]
    NotAProject(PathBuf),
    #[error("invalid JSON in {path}: {message}")]
    InvalidJson { path: PathBuf, message: String },
    #[error("invalid project: {0:?}")]
    Invalid(Vec<ValidationIssue>),
    #[error("source destination already exists: {0}")]
    SourceCollision(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum CommitError {
    #[error("empty commit")]
    EmptyCommit,
    #[error("dishonest sticky authorship: {message}")]
    Authorship { message: String },
    #[error("commit conflict: {reason}")]
    Conflict {
        current_version: String,
        reason: String,
        intervening: Vec<CommitSummary>,
    },
    #[error(transparent)]
    Op(#[from] OpError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("project landed but its history record did not: {error}")]
    LandedWithoutLog {
        record: Box<CommitRecord>,
        error: StoreError,
    },
}
pub fn open(dir: &Path) -> Result<Loaded, StoreError> {
    let (project, bytes) = read_project(dir)?;
    Ok(Loaded {
        project,
        version: version_of(&bytes),
        dir: dir.to_path_buf(),
    })
}

pub fn create(dir: &Path, title: &str, fps: u32, format: Format) -> Result<Loaded, StoreError> {
    let project = Project {
        schema: "nb-storyboard/v1".into(),
        title: title.into(),
        fps,
        format,
        scenes: Vec::new(),
        cuts: Vec::new(),
        active_cut: None,
        stickies: Vec::new(),
        layout: Layout {
            positions: BTreeMap::new(),
            extra: Map::new(),
        },
        extra: Map::new(),
    };
    let issues = validate(&project);
    if !issues.is_empty() {
        return Err(StoreError::Invalid(issues));
    }
    let dir_existed = dir.exists();
    let tracked = ["sources", "history", ".cache", ".gitignore"]
        .map(|name| (dir.join(name), dir.join(name).exists()));
    let result = (|| {
        fs::create_dir_all(dir)?;
        let _lock = ProjectLock::acquire(dir)?;
        let path = project_path(dir);
        if path.exists() {
            return Err(StoreError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "project already exists",
            )));
        }
        ensure_project_directories(dir)?;
        write_gitignore(dir)?;
        let bytes = serialize_project(&project)?;
        write_atomic(dir, &bytes)?;
        Ok(Loaded {
            project,
            version: version_of(&bytes),
            dir: dir.to_path_buf(),
        })
    })();
    if result.is_err() {
        if !dir_existed {
            let _ = fs::remove_dir_all(dir);
        } else {
            for (path, existed) in tracked.into_iter().rev() {
                if !existed {
                    let _ = if path.is_dir() {
                        fs::remove_dir(&path)
                    } else {
                        fs::remove_file(&path)
                    };
                }
            }
        }
    }
    result
}

pub fn commit(
    dir: &Path,
    base_version: &str,
    ops: Vec<Op>,
    author: Author,
    note: Option<String>,
) -> Result<CommitRecord, CommitError> {
    if ops.is_empty() {
        return Err(CommitError::EmptyCommit);
    }
    for op in &ops {
        op.check_public()?;
    }
    if matches!(author, Author::Agent { .. })
        && ops
            .iter()
            .flat_map(Op::sticky_authors)
            .any(|sticky_author| sticky_author != &author)
    {
        return Err(CommitError::Authorship {
            message: "an agent may only land stickies under its own author".into(),
        });
    }
    let _lock = ProjectLock::acquire(dir).map_err(StoreError::from)?;
    repair_log_tail_for_dir(dir).map_err(CommitError::Store)?;
    recover_pending_commit(dir).map_err(CommitError::Store)?;
    commit_locked(
        dir,
        base_version,
        ops,
        author,
        note,
        CommitMetadata::default(),
    )
}

#[derive(Default)]
struct CommitMetadata {
    forced_footprints: Option<Vec<String>>,
    undoes: Option<String>,
    redoes: Option<String>,
}

fn commit_locked(
    dir: &Path,
    base_version: &str,
    ops: Vec<Op>,
    author: Author,
    note: Option<String>,
    metadata: CommitMetadata,
) -> Result<CommitRecord, CommitError> {
    let (project, old_bytes) = read_project(dir).map_err(CommitError::Store)?;
    let current_version = version_of(&old_bytes);
    let records = synchronize_external_edit(dir, &project, &old_bytes, &current_version)
        .map_err(CommitError::Store)?;

    let stale = base_version != current_version;
    // Applying before the rebase decision deliberately surfaces a real bad op before a stale-base conflict.
    let mut candidate = project.clone();
    let mut inverses = Vec::new();
    let mut ours = Vec::new();
    let mut created = Vec::new();
    for op in &ops {
        let applied = match apply(&mut candidate, op) {
            Ok(applied) => applied,
            Err(error) => {
                if stale && ops_apply_to_version(dir, base_version, &ops) {
                    let intervening = records_since_from(&records, base_version)
                        .unwrap_or_default()
                        .iter()
                        .map(CommitSummary::from)
                        .collect();
                    return Err(CommitError::Conflict {
                        current_version,
                        reason: "operation was valid at its base but cannot rebase onto the current project".into(),
                        intervening,
                    });
                }
                return Err(CommitError::Op(error));
            }
        };
        created.extend(applied.created_ids);
        inverses.splice(0..0, applied.inverse);
        ours.extend(applied.footprints);
    }
    let mut ours = metadata.forced_footprints.unwrap_or(ours);
    ours.sort();
    ours.dedup();
    let append_only_scenes = ops
        .iter()
        .all(|op| matches!(op, Op::AddScene { index: None, .. }));
    let intervening = if stale {
        let Some(records) = records_since_from(&records, base_version) else {
            return Err(CommitError::Conflict {
                current_version,
                reason: "base version is not in local history".into(),
                intervening: Vec::new(),
            });
        };
        if records
            .last()
            .is_none_or(|record| record.version != current_version)
        {
            return Err(CommitError::Conflict {
                current_version,
                reason: "history does not reach the on-disk project".into(),
                intervening: records.iter().map(CommitSummary::from).collect(),
            });
        }
        let theirs: Vec<_> = records
            .iter()
            .flat_map(|record| record.footprints.iter().cloned())
            .collect();
        let conflict_ours: Vec<_> = ours
            .iter()
            .filter(|footprint| {
                !append_only_scenes || !matches!(footprint.as_str(), "scene/order" | "ids/scene")
            })
            .cloned()
            .collect();
        if !disjoint(&conflict_ours, &theirs) {
            return Err(CommitError::Conflict {
                current_version,
                reason: "operation footprints overlap an intervening commit".into(),
                intervening: records.iter().map(CommitSummary::from).collect(),
            });
        }
        records
    } else {
        Vec::new()
    };
    let bytes = serialize_project(&candidate).map_err(CommitError::Store)?;
    let summary = note
        .clone()
        .unwrap_or_else(|| summary_for(&ops, &intervening, &project, &candidate));
    let record = CommitRecord {
        version: version_of(&bytes),
        parent: current_version,
        at: now(),
        author,
        ops,
        inverses,
        footprints: ours,
        rebased: stale,
        note,
        summary,
        external: false,
        undoes: metadata.undoes,
        redoes: metadata.redoes,
    };
    match persist_commit(dir, &old_bytes, &record, &bytes) {
        Ok(()) => {}
        Err(PersistError::BeforeLanding(error)) => return Err(CommitError::Store(error)),
        Err(PersistError::Landed(error)) => {
            return Err(CommitError::LandedWithoutLog {
                record: Box::new(record),
                error,
            });
        }
    }
    Ok(record)
}

pub fn undo(
    dir: &Path,
    base_version: &str,
    author: Author,
    steps: usize,
) -> Result<CommitRecord, CommitError> {
    if steps == 0 {
        return Err(CommitError::EmptyCommit);
    }
    let _lock = ProjectLock::acquire(dir).map_err(StoreError::from)?;
    repair_log_tail_for_dir(dir).map_err(CommitError::Store)?;
    recover_pending_commit(dir).map_err(CommitError::Store)?;
    let (project, project_bytes) = read_project(dir).map_err(CommitError::Store)?;
    let mut current_version = version_of(&project_bytes);
    let records = synchronize_external_edit(dir, &project, &project_bytes, &current_version)
        .map_err(CommitError::Store)?;
    if base_version != current_version {
        let intervening = records_since_from(&records, base_version)
            .unwrap_or_default()
            .iter()
            .map(CommitSummary::from)
            .collect();
        return Err(CommitError::Conflict {
            current_version,
            reason: "undo base version is stale".into(),
            intervening,
        });
    }
    let state = history_state(&records);
    let mut active: Vec<_> = state
        .active
        .iter()
        .filter(|(index, _)| records[**index].author == author)
        .map(|(index, activation)| (*index, *activation))
        .collect();
    active.sort_by_key(|(_, activation)| std::cmp::Reverse(*activation));
    let selected: Vec<_> = active
        .into_iter()
        .map(|(index, _)| index)
        .take(steps)
        .collect();
    if selected.len() != steps {
        return Err(CommitError::Conflict {
            current_version: version_of(&read_project(dir).map_err(CommitError::Store)?.1),
            reason: "not enough commits by this author to undo".into(),
            intervening: Vec::new(),
        });
    }
    for &position in &selected {
        let activation = state.active[&position];
        let later_positions = effective_later_positions(&records, &state, activation, &selected);
        let target = &records[position];
        let later_footprints: Vec<_> = later_positions
            .iter()
            .map(|index| &records[*index])
            .flat_map(|record| record.footprints.iter().cloned())
            .collect();
        if !disjoint(&target.footprints, &later_footprints) {
            return Err(CommitError::Conflict {
                current_version: version_of(&read_project(dir).map_err(CommitError::Store)?.1),
                reason: "undo footprint overlaps a later commit".into(),
                intervening: later_positions
                    .iter()
                    .map(|index| &records[*index])
                    .map(CommitSummary::from)
                    .collect(),
            });
        }
    }

    let mut result = None;
    for (index, &position) in selected.iter().enumerate() {
        let target = &records[position];
        let record = commit_locked(
            dir,
            &current_version,
            target.inverses.clone(),
            author.clone(),
            Some(format!("undo {} of {steps} commit(s)", index + 1)),
            CommitMetadata {
                forced_footprints: Some(target.footprints.clone()),
                undoes: Some(target.version.clone()),
                redoes: None,
            },
        )?;
        current_version = record.version.clone();
        result = Some(record);
    }
    Ok(result.expect("steps is nonzero and selected has that many records"))
}

#[derive(Clone)]
struct UndoneOccurrence {
    target: usize,
    undo_record: usize,
}

#[derive(Default)]
struct AuthorRedo {
    author: Option<Author>,
    undone: Vec<UndoneOccurrence>,
}

#[derive(Default)]
struct HistoryState {
    /// Active regular commit occurrence -> its most recent application record position.
    active: BTreeMap<usize, usize>,
    redo: Vec<AuthorRedo>,
}

fn author_redo_mut<'a>(state: &'a mut HistoryState, author: &Author) -> &'a mut AuthorRedo {
    if let Some(index) = state
        .redo
        .iter()
        .position(|entry| entry.author.as_ref() == Some(author))
    {
        return &mut state.redo[index];
    }
    state.redo.push(AuthorRedo {
        author: Some(author.clone()),
        undone: Vec::new(),
    });
    state.redo.last_mut().expect("just pushed")
}

fn redo_selection(state: &HistoryState, author: &Author, steps: usize) -> Vec<UndoneOccurrence> {
    state
        .redo
        .iter()
        .find(|entry| entry.author.as_ref() == Some(author))
        .map(|entry| entry.undone.iter().rev().take(steps).cloned().collect())
        .unwrap_or_default()
}

fn history_state(records: &[CommitRecord]) -> HistoryState {
    let mut state = HistoryState::default();
    for (record_index, record) in records.iter().enumerate() {
        if record.external {
            continue;
        }
        if let Some(version) = record.undoes.as_deref() {
            let target = state
                .active
                .iter()
                .filter(|(index, _)| {
                    let candidate = &records[**index];
                    candidate.author == record.author && candidate.version == version
                })
                .max_by_key(|(_, activation)| **activation)
                .map(|(index, _)| *index);
            if let Some(target) = target {
                state.active.remove(&target);
                author_redo_mut(&mut state, &record.author)
                    .undone
                    .push(UndoneOccurrence {
                        target,
                        undo_record: record_index,
                    });
            }
            continue;
        }
        if let Some(version) = record.redoes.as_deref() {
            let redo = author_redo_mut(&mut state, &record.author);
            if let Some(position) = redo
                .undone
                .iter()
                .rposition(|entry| records[entry.target].version == version)
            {
                let occurrence = redo.undone.remove(position);
                state.active.insert(occurrence.target, record_index);
            }
            continue;
        }
        state.active.insert(record_index, record_index);
        author_redo_mut(&mut state, &record.author).undone.clear();
    }
    state
}

fn effective_later_positions(
    records: &[CommitRecord],
    state: &HistoryState,
    after: usize,
    excluded: &[usize],
) -> Vec<usize> {
    let mut positions: Vec<_> = state
        .active
        .iter()
        .filter(|(target, activation)| **activation > after && !excluded.contains(target))
        .map(|(_, activation)| *activation)
        .chain(
            records
                .iter()
                .enumerate()
                .filter(|(index, record)| record.external && *index > after)
                .map(|(index, _)| index),
        )
        .collect();
    positions.sort_unstable();
    positions.dedup();
    positions
}

pub fn redo(
    dir: &Path,
    base_version: &str,
    author: Author,
    steps: usize,
) -> Result<CommitRecord, CommitError> {
    if steps == 0 {
        return Err(CommitError::EmptyCommit);
    }
    let _lock = ProjectLock::acquire(dir).map_err(StoreError::from)?;
    repair_log_tail_for_dir(dir).map_err(CommitError::Store)?;
    recover_pending_commit(dir).map_err(CommitError::Store)?;
    let (project, project_bytes) = read_project(dir).map_err(CommitError::Store)?;
    let mut current_version = version_of(&project_bytes);
    let records = synchronize_external_edit(dir, &project, &project_bytes, &current_version)
        .map_err(CommitError::Store)?;
    let state = history_state(&records);
    let selected = redo_selection(&state, &author, steps);
    if base_version != current_version {
        let Some(intervening) = records_since_from(&records, base_version) else {
            return Err(CommitError::Conflict {
                current_version,
                reason: "base version is not in local history".into(),
                intervening: Vec::new(),
            });
        };
        if intervening
            .last()
            .is_none_or(|record| record.version != current_version)
        {
            return Err(CommitError::Conflict {
                current_version,
                reason: "history does not reach the on-disk project".into(),
                intervening: intervening.iter().map(CommitSummary::from).collect(),
            });
        }
        let base_state = history_state(&records[..records.len() - intervening.len()]);
        let requested = redo_selection(&base_state, &author, steps);
        if requested
            .iter()
            .map(|entry| entry.target)
            .ne(selected.iter().map(|entry| entry.target))
        {
            return Err(CommitError::Conflict {
                current_version,
                reason: "redo target changed since base version".into(),
                intervening: intervening.iter().map(CommitSummary::from).collect(),
            });
        }
    }
    if selected.len() != steps {
        return Err(CommitError::Conflict {
            current_version,
            reason: "not enough undone commits by this author to redo".into(),
            intervening: Vec::new(),
        });
    }
    for occurrence in &selected {
        let target = &records[occurrence.target];
        let later_positions =
            effective_later_positions(&records, &state, occurrence.undo_record, &[]);
        let later_footprints: Vec<_> = later_positions
            .iter()
            .flat_map(|index| records[*index].footprints.iter().cloned())
            .collect();
        if !disjoint(&target.footprints, &later_footprints) {
            return Err(CommitError::Conflict {
                current_version,
                reason: "redo footprint overlaps a later commit".into(),
                intervening: later_positions
                    .iter()
                    .map(|index| CommitSummary::from(&records[*index]))
                    .collect(),
            });
        }
    }
    let mut result = None;
    for (index, occurrence) in selected.iter().enumerate() {
        let target = &records[occurrence.target];
        let undo_record = &records[occurrence.undo_record];
        let record = commit_locked(
            dir,
            &current_version,
            undo_record.inverses.clone(),
            author.clone(),
            Some(format!("redo {} of {steps} commit(s)", index + 1)),
            CommitMetadata {
                forced_footprints: Some(target.footprints.clone()),
                undoes: None,
                redoes: Some(target.version.clone()),
            },
        )?;
        current_version = record.version.clone();
        result = Some(record);
    }
    Ok(result.expect("steps is nonzero and selected has that many records"))
}

fn synchronize_external_edit(
    dir: &Path,
    actual: &Project,
    actual_bytes: &[u8],
    current_version: &str,
) -> Result<Vec<CommitRecord>, StoreError> {
    let mut records = read_records(dir)?;
    let Some(last) = records.last() else {
        return Ok(records);
    };
    if last.version == current_version {
        return Ok(records);
    }
    let footprints = expected_after_last_record(dir, last)
        .map(|expected| external_footprints(&expected, actual))
        .unwrap_or_else(|| vec!["project".into()]);
    let record = CommitRecord {
        version: current_version.into(),
        parent: last.version.clone(),
        at: now(),
        author: Author::Human,
        ops: Vec::new(),
        inverses: Vec::new(),
        footprints,
        rebased: false,
        note: None,
        summary: "external edit".into(),
        external: true,
        undoes: None,
        redoes: None,
    };
    let baseline = dir
        .join("history")
        .join(format!("archive-{current_version}.json"));
    write_archive(&baseline, actual_bytes, current_version)?;
    append_record(dir, &record)?;
    records.push(record);
    Ok(records)
}

enum PersistError {
    BeforeLanding(StoreError),
    Landed(StoreError),
}

#[derive(Serialize, Deserialize)]
struct PendingCommit {
    record: CommitRecord,
}

fn persist_commit(
    dir: &Path,
    old_bytes: &[u8],
    record: &CommitRecord,
    new_bytes: &[u8],
) -> Result<(), PersistError> {
    ensure_project_directories(dir).map_err(PersistError::BeforeLanding)?;
    let history = dir.join("history");
    let archive = history.join(format!("archive-{}.json", record.parent));
    write_archive(&archive, old_bytes, &record.parent).map_err(PersistError::BeforeLanding)?;
    File::open(&history)
        .and_then(|file| file.sync_all())
        .map_err(StoreError::Io)
        .map_err(PersistError::BeforeLanding)?;
    maybe_crash("after_archive");
    write_pending_commit(dir, record).map_err(PersistError::BeforeLanding)?;
    stage_project(dir, new_bytes).map_err(PersistError::BeforeLanding)?;
    maybe_crash("after_tmp");
    land_staged_project(dir).map_err(PersistError::BeforeLanding)?;
    maybe_crash("after_rename");
    if cfg!(debug_assertions) && std::env::var("NB_STORYBOARD_TEST_FAIL_LOG").as_deref() == Ok("1")
    {
        return Err(PersistError::Landed(StoreError::Io(io::Error::other(
            "injected history append failure",
        ))));
    }
    append_record(dir, record).map_err(PersistError::Landed)?;
    maybe_crash("after_log");
    clear_pending_commit(dir).map_err(PersistError::Landed)?;
    let count = read_records(dir).map_err(PersistError::Landed)?.len();
    if count % 50 == 0 {
        let snapshot = history.join(format!("snapshot-{count}.json"));
        if !snapshot.exists() {
            write_new_file_sync(&snapshot, new_bytes).map_err(PersistError::Landed)?;
        }
    }
    Ok(())
}

fn write_atomic(dir: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    stage_project(dir, bytes)?;
    land_staged_project(dir)
}

fn stage_project(dir: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = dir.join("project.json.tmp");
    match tmp.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(&tmp)?
        }
        Ok(_) => {
            return Err(StoreError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid project temporary file",
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StoreError::Io(error)),
    }
    write_new_file_sync(&tmp, bytes)?;
    Ok(())
}

fn land_staged_project(dir: &Path) -> Result<(), StoreError> {
    let tmp = dir.join("project.json.tmp");
    fs::rename(&tmp, project_path(dir))?;
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn write_archive(path: &Path, bytes: &[u8], expected_version: &str) -> Result<(), StoreError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(existing) if version_of(&existing) == expected_version => return Ok(()),
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StoreError::Io(error)),
    }
    write_new_file_sync(path, bytes)
}

fn pending_path(dir: &Path) -> PathBuf {
    dir.join(".cache/pending-commit.json")
}

fn write_pending_commit(dir: &Path, record: &CommitRecord) -> Result<(), StoreError> {
    let path = pending_path(dir);
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(&path)?;
        }
        Ok(_) => {
            return Err(StoreError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid pending commit file",
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StoreError::Io(error)),
    }
    let bytes = serde_json::to_vec(&PendingCommit {
        record: record.clone(),
    })
    .map_err(|error| StoreError::InvalidJson {
        path: path.clone(),
        message: error.to_string(),
    })?;
    write_new_file_sync(&path, &bytes)?;
    File::open(dir.join(".cache"))?.sync_all()?;
    Ok(())
}

fn clear_pending_commit(dir: &Path) -> Result<(), StoreError> {
    let path = pending_path(dir);
    match fs::remove_file(&path) {
        Ok(()) => File::open(dir.join(".cache"))?.sync_all()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StoreError::Io(error)),
    }
    Ok(())
}

fn recover_pending_commit(dir: &Path) -> Result<(), StoreError> {
    let path = pending_path(dir);
    reject_symlink(&path)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(StoreError::Io(error)),
    };
    let pending: PendingCommit =
        serde_json::from_slice(&bytes).map_err(|error| StoreError::InvalidJson {
            path: path.clone(),
            message: error.to_string(),
        })?;
    let disk_version = version_of(&fs::read(project_path(dir))?);
    if disk_version == pending.record.version {
        let records = read_records(dir)?;
        if records
            .last()
            .is_none_or(|record| record.version != pending.record.version)
        {
            append_record(dir, &pending.record)?;
        }
    }
    clear_pending_commit(dir)?;
    Ok(())
}

fn maybe_crash(point: &str) {
    if cfg!(debug_assertions)
        && std::env::var("NB_STORYBOARD_TEST_CRASH_AT").as_deref() == Ok(point)
    {
        std::process::abort();
    }
}

pub(crate) fn read_project_unvalidated(dir: &Path) -> Result<(Project, Vec<u8>), StoreError> {
    let path = project_path(dir);
    if !path.exists() {
        return Err(StoreError::NotAProject(path));
    }
    reject_symlink(&path)?;
    let bytes = fs::read(&path)?;
    let project = serde_json::from_slice(&bytes).map_err(|error| StoreError::InvalidJson {
        path,
        message: error.to_string(),
    })?;
    Ok((project, bytes))
}
fn read_project(dir: &Path) -> Result<(Project, Vec<u8>), StoreError> {
    let (project, bytes) = read_project_unvalidated(dir)?;
    let issues = validate(&project);
    if issues.is_empty() {
        Ok((project, bytes))
    } else {
        Err(StoreError::Invalid(issues))
    }
}
fn serialize_project(project: &Project) -> Result<Vec<u8>, StoreError> {
    let mut bytes = project
        .to_pretty_json()
        .map_err(|error| StoreError::InvalidJson {
            path: PathBuf::from(PROJECT_FILE),
            message: error.to_string(),
        })?
        .into_bytes();
    bytes.push(b'\n');
    Ok(bytes)
}
fn write_gitignore(dir: &Path) -> Result<(), StoreError> {
    let path = dir.join(".gitignore");
    if !path.exists() {
        write_new_file_sync(&path, b"history/\n.cache/\n.lock\n")?;
    }
    Ok(())
}
fn ensure_project_directories(dir: &Path) -> Result<(), StoreError> {
    for name in ["sources", "history", ".cache"] {
        let path = dir.join(name);
        if !path.exists() {
            fs::create_dir(&path)?;
        } else {
            ensure_real_directory(&path)?;
        }
    }
    Ok(())
}
pub(crate) fn ensure_real_directory(path: &Path) -> Result<(), StoreError> {
    let metadata = path.symlink_metadata()?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        Err(StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("expected real directory: {}", path.display()),
        )))
    } else {
        Ok(())
    }
}
/// Creates a git-ignored folder such as `.cache` when a fresh clone lacks it, then refuses it
/// unless it is a real directory (never a symlink).
pub(crate) fn ensure_owned_directory(path: &Path) -> Result<(), StoreError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => ensure_real_directory(path),
        Err(error) => Err(StoreError::Io(error)),
    }
}
pub(crate) fn reject_symlink(path: &Path) -> Result<(), StoreError> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing symlink: {}", path.display()),
        ))),
        Ok(_) | Err(_) => Ok(()),
    }
}
fn write_new_file_sync(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn project_path(dir: &Path) -> PathBuf {
    dir.join(PROJECT_FILE)
}
fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("UTC timestamps format as RFC 3339")
}

fn summary_for(
    ops: &[Op],
    intervening: &[CommitRecord],
    before: &Project,
    after: &Project,
) -> String {
    let prefix = if intervening.is_empty() {
        ""
    } else {
        "rebased "
    };
    let mut flattened = Vec::new();
    flatten_ops(ops, &mut flattened);
    let detail = aggregate_summary(&flattened).unwrap_or_else(|| {
        flattened
            .iter()
            .map(|op| op_summary(op, before, after))
            .collect::<Vec<_>>()
            .join("; ")
    });
    one_line_under_120(&format!("{prefix}{detail}"))
}

fn flatten_ops<'a>(ops: &'a [Op], flattened: &mut Vec<&'a Op>) {
    for op in ops {
        match op {
            Op::Batch { ops } => flatten_ops(ops, flattened),
            other => flattened.push(other),
        }
    }
}

fn aggregate_summary(ops: &[&Op]) -> Option<String> {
    if ops.len() < 2 {
        return None;
    }
    if ops.iter().all(|op| matches!(op, Op::MoveScene { .. })) {
        return Some(format!("reordered {} scenes", ops.len()));
    }
    if ops.iter().all(|op| matches!(op, Op::MoveShot { .. })) {
        return Some(format!("reordered {} shots", ops.len()));
    }
    if ops.iter().all(|op| matches!(op, Op::MoveTake { .. })) {
        return Some(format!("reordered {} takes", ops.len()));
    }
    if ops.iter().all(|op| matches!(op, Op::MovePanel { .. })) {
        return Some(format!("reordered {} panels", ops.len()));
    }
    if ops.iter().all(|op| matches!(op, Op::MoveLayer { .. })) {
        return Some(format!("reordered {} layers", ops.len()));
    }
    if ops.iter().all(|op| matches!(op, Op::ReorderClip { .. })) {
        return Some(format!("reordered {} clips", ops.len()));
    }
    None
}

fn one_line_under_120(summary: &str) -> String {
    let clean = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() < 120 {
        return clean;
    }
    let mut shortened: String = clean.chars().take(116).collect();
    shortened.push('…');
    shortened
}

fn quoted(value: &str) -> String {
    format!(
        "\u{201c}{}\u{201d}",
        value.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

fn titled(title: &str, id: &str) -> String {
    quoted(if title.trim().is_empty() { id } else { title })
}

fn scene_name(before: &Project, after: &Project, id: &str) -> String {
    before
        .scenes
        .iter()
        .chain(&after.scenes)
        .find(|scene| scene.id == id)
        .map(|scene| titled(&scene.title, &scene.id))
        .unwrap_or_else(|| quoted(id))
}

fn shot<'a>(project: &'a Project, id: &str) -> Option<&'a storyboard_core::Shot> {
    project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .find(|shot| shot.id == id)
}

fn shot_name(before: &Project, after: &Project, id: &str) -> String {
    shot(before, id)
        .or_else(|| shot(after, id))
        .map(|shot| titled(&shot.title, &shot.id))
        .unwrap_or_else(|| quoted(id))
}

fn take_name(before: &Project, after: &Project, shot_id: &str, take_id: &str) -> String {
    shot(before, shot_id)
        .or_else(|| shot(after, shot_id))
        .and_then(|shot| shot.takes.iter().find(|take| take.id == take_id))
        .map(|take| titled(&take.name, &take.id))
        .unwrap_or_else(|| quoted(take_id))
}

fn panel_name(
    before: &Project,
    after: &Project,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
) -> String {
    shot(before, shot_id)
        .or_else(|| shot(after, shot_id))
        .and_then(|shot| shot.takes.iter().find(|take| take.id == take_id))
        .and_then(|take| take.panels.iter().find(|panel| panel.id == panel_id))
        .map(|panel| titled(&panel.title, &panel.id))
        .unwrap_or_else(|| quoted(panel_id))
}

fn layer_name(
    before: &Project,
    after: &Project,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
    layer_id: &str,
) -> String {
    shot(before, shot_id)
        .or_else(|| shot(after, shot_id))
        .and_then(|shot| shot.takes.iter().find(|take| take.id == take_id))
        .and_then(|take| take.panels.iter().find(|panel| panel.id == panel_id))
        .and_then(|panel| panel.layers.iter().find(|layer| layer.id == layer_id))
        .map(|layer| titled(&layer.label, &layer.id))
        .unwrap_or_else(|| quoted(layer_id))
}

fn cut_name(before: &Project, after: &Project, id: &str) -> String {
    before
        .cuts
        .iter()
        .chain(&after.cuts)
        .find(|cut| cut.id == id)
        .map(|cut| titled(&cut.name, &cut.id))
        .unwrap_or_else(|| quoted(id))
}

fn target_name(before: &Project, after: &Project, target: &storyboard_core::RefTarget) -> String {
    match target {
        storyboard_core::RefTarget::Shot { shot: shot_id } => {
            match shot(before, shot_id).or_else(|| shot(after, shot_id)) {
                Some(found) if found.title != found.id => {
                    format!("shot {} ({})", found.id, quoted(&found.title))
                }
                _ => format!("shot {shot_id}"),
            }
        }
        storyboard_core::RefTarget::Take {
            shot: shot_id,
            take: take_id,
        } => {
            let target = format!("take {shot_id}/{take_id}");
            match (
                shot(before, shot_id).or_else(|| shot(after, shot_id)),
                shot(before, shot_id)
                    .or_else(|| shot(after, shot_id))
                    .and_then(|found| {
                        found
                            .takes
                            .iter()
                            .find(|candidate| candidate.id == *take_id)
                    }),
            ) {
                (Some(found_shot), Some(found_take))
                    if found_shot.title != found_shot.id || found_take.name != found_take.id =>
                {
                    format!(
                        "{target} ({} / {})",
                        quoted(&found_shot.title),
                        quoted(&found_take.name)
                    )
                }
                _ => target,
            }
        }
    }
}

fn node_name(before: &Project, after: &Project, node: &str) -> String {
    match node.split_once(':') {
        Some(("scene", id)) => scene_name(before, after, id),
        Some(("shot", id)) => shot_name(before, after, id),
        Some(("cut", id)) => cut_name(before, after, id),
        _ => quoted(node),
    }
}

fn op_summary(op: &Op, before: &Project, after: &Project) -> String {
    match op {
        Op::Batch { ops } => summary_for(ops, &[], before, after),
        Op::AddScene { title, .. } => format!("added scene {}", titled(title, "scene")),
        Op::RemoveScene { scene } => format!("removed scene {}", scene_name(before, after, scene)),
        Op::MoveScene { scene, .. } => {
            format!("reordered scene {}", scene_name(before, after, scene))
        }
        Op::UpdateScene { scene, .. } => {
            format!("updated scene {}", scene_name(before, after, scene))
        }
        Op::AddShot { scene, title, .. } => format!(
            "added shot {} to {}",
            titled(title, "shot"),
            scene_name(before, after, scene)
        ),
        Op::DuplicateShot { shot, .. } => {
            format!("duplicated shot {}", shot_name(before, after, shot))
        }
        Op::RemoveShot { shot } => format!("removed shot {}", shot_name(before, after, shot)),
        Op::MoveShot { shot, .. } => format!("reordered shot {}", shot_name(before, after, shot)),
        Op::UpdateShot { shot, .. } => format!("updated shot {}", shot_name(before, after, shot)),
        Op::AddTake { shot, name, .. } => format!(
            "added take {} to {}",
            titled(name, "take"),
            shot_name(before, after, shot)
        ),
        Op::RemoveTake { shot, take } => format!(
            "removed take {} from {}",
            take_name(before, after, shot, take),
            shot_name(before, after, shot)
        ),
        Op::MoveTake { shot, take, .. } => format!(
            "reordered take {} in {}",
            take_name(before, after, shot, take),
            shot_name(before, after, shot)
        ),
        Op::UpdateTake { shot, take, .. } => format!(
            "updated take {} in {}",
            take_name(before, after, shot, take),
            shot_name(before, after, shot)
        ),
        Op::SelectTake { shot, take } => format!(
            "selected take {} for {}",
            take_name(before, after, shot, take),
            shot_name(before, after, shot)
        ),
        Op::DuplicateTake { shot, take, .. } => format!(
            "duplicated take {} in {}",
            take_name(before, after, shot, take),
            shot_name(before, after, shot)
        ),
        Op::AddPanel { shot, take, .. } => format!(
            "added panel to take {} of {}",
            take_name(before, after, shot, take),
            shot_name(before, after, shot)
        ),
        Op::RemovePanel { shot, take, panel } => format!(
            "removed panel {} from {}",
            panel_name(before, after, shot, take, panel),
            shot_name(before, after, shot)
        ),
        Op::MovePanel {
            shot, take, panel, ..
        } => format!(
            "reordered panel {} in {}",
            panel_name(before, after, shot, take, panel),
            shot_name(before, after, shot)
        ),
        Op::UpdatePanel {
            shot, take, panel, ..
        } => format!(
            "updated panel {} in {}",
            panel_name(before, after, shot, take, panel),
            shot_name(before, after, shot)
        ),
        Op::AddLayer {
            shot, layer_type, ..
        } => format!(
            "added {layer_type} layer to {}",
            shot_name(before, after, shot)
        ),
        Op::RemoveLayer {
            shot,
            take,
            panel,
            layer,
        } => format!(
            "removed layer {} from {}",
            layer_name(before, after, shot, take, panel, layer),
            shot_name(before, after, shot)
        ),
        Op::MoveLayer {
            shot,
            take,
            panel,
            layer,
            ..
        } => format!(
            "reordered layer {} in {}",
            layer_name(before, after, shot, take, panel, layer),
            shot_name(before, after, shot)
        ),
        Op::UpdateLayer {
            shot,
            take,
            panel,
            layer,
            ..
        } => format!(
            "updated layer {} in {}",
            layer_name(before, after, shot, take, panel, layer),
            shot_name(before, after, shot)
        ),
        Op::AddSticky { .. } => "added note".into(),
        Op::RemoveSticky { .. } => "removed note".into(),
        Op::MoveSticky { .. } => "reordered note".into(),
        Op::UpdateSticky { .. } => "updated note".into(),
        Op::AddClip { cut, .. } => format!("added clip to cut {}", cut_name(before, after, cut)),
        Op::RemoveClip { cut, .. } => {
            format!("removed clip from cut {}", cut_name(before, after, cut))
        }
        Op::ReorderClip { cut, .. } => {
            format!("reordered clip in cut {}", cut_name(before, after, cut))
        }
        Op::UpdateClip { cut, .. } => {
            format!("updated clip in cut {}", cut_name(before, after, cut))
        }
        Op::MergeClips { cut, .. } => {
            format!("merged clips in cut {}", cut_name(before, after, cut))
        }
        Op::GroupClips { cut, name, .. } => format!(
            "grouped clips as {} in cut {}",
            titled(name, "group"),
            cut_name(before, after, cut)
        ),
        Op::UngroupClips { cut, .. } => {
            format!("ungrouped clip in cut {}", cut_name(before, after, cut))
        }
        Op::AddCut { name, .. } => format!("added cut {}", titled(name, "cut")),
        Op::UpdateCut { cut, .. } => format!("updated cut {}", cut_name(before, after, cut)),
        Op::RemoveCut { cut } => format!("removed cut {}", cut_name(before, after, cut)),
        Op::SetActiveCut { cut } => match cut {
            Some(cut) => format!("viewed cut {}", cut_name(before, after, cut)),
            None => "viewed the story sequence".into(),
        },
        Op::MoveShotBetweenScenes { shot, scene, .. } => format!(
            "moved {} to {}",
            shot_name(before, after, shot),
            scene_name(before, after, scene)
        ),
        Op::AddRef {
            target, reference, ..
        } => format!(
            "attached {} to {}",
            quoted(reference),
            target_name(before, after, target)
        ),
        Op::RemoveRef { target, reference } => format!(
            "detached {} from {}",
            quoted(reference),
            target_name(before, after, target)
        ),
        Op::ResolveSticky { .. } => "resolved note".into(),
        Op::ReplySticky { sticky, .. }
            if before
                .stickies
                .iter()
                .any(|note| note.id == *sticky && note.resolved) =>
        {
            "replied to resolved note; reopened thread".into()
        }
        Op::ReplySticky { .. } => "replied to note".into(),
        Op::MoveNode { node, position } => match position {
            Some(_) => format!("moved {} in the graph", node_name(before, after, node)),
            None => format!("reset {} in the graph", node_name(before, after, node)),
        },
        Op::RestoreChanges { .. } => "restored changes".into(),
    }
}

fn ops_apply_to_version(dir: &Path, version: &str, ops: &[Op]) -> bool {
    let path = dir.join("history").join(format!("archive-{version}.json"));
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let Ok(mut project) = serde_json::from_slice::<Project>(&bytes) else {
        return false;
    };
    ops.iter().all(|op| apply(&mut project, op).is_ok())
}

fn expected_after_last_record(dir: &Path, record: &CommitRecord) -> Option<Project> {
    if record.external {
        let bytes = fs::read(
            dir.join("history")
                .join(format!("archive-{}.json", record.version)),
        )
        .ok()?;
        return serde_json::from_slice(&bytes).ok();
    }
    let bytes = fs::read(
        dir.join("history")
            .join(format!("archive-{}.json", record.parent)),
    )
    .ok()?;
    let mut project: Project = serde_json::from_slice(&bytes).ok()?;
    for op in &record.ops {
        apply(&mut project, op).ok()?;
    }
    Some(project)
}

fn external_footprints(expected: &Project, actual: &Project) -> Vec<String> {
    let before = serde_json::to_value(expected).unwrap_or(Value::Null);
    let after = serde_json::to_value(actual).unwrap_or(Value::Null);
    let (Value::Object(before), Value::Object(after)) = (before, after) else {
        return vec!["project".into()];
    };
    let mut footprints = Vec::new();
    for key in before.keys().chain(after.keys()) {
        if before.get(key) == after.get(key) {
            continue;
        }
        match key.as_str() {
            "scenes" => diff_entities(
                before.get(key),
                after.get(key),
                "scene",
                EntityKind::Scene,
                &mut footprints,
            ),
            "cuts" => diff_entities(
                before.get(key),
                after.get(key),
                "cut",
                EntityKind::Cut,
                &mut footprints,
            ),
            "stickies" => diff_entities(
                before.get(key),
                after.get(key),
                "sticky",
                EntityKind::Leaf,
                &mut footprints,
            ),
            "layout" => footprints.push("layout".into()),
            field => footprints.push(format!("project/{field}")),
        }
    }
    footprints.sort();
    footprints.dedup();
    if footprints.is_empty() {
        vec!["project".into()]
    } else {
        footprints
    }
}

#[derive(Clone, Copy)]
enum EntityKind {
    Scene,
    Shot,
    Take,
    Panel,
    Cut,
    Clip,
    Leaf,
}

fn diff_entities(
    before: Option<&Value>,
    after: Option<&Value>,
    prefix: &str,
    kind: EntityKind,
    footprints: &mut Vec<String>,
) {
    let before = entities_by_id(before);
    let after = entities_by_id(after);
    for id in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let path = format!("{prefix}/{id}");
        match (before.get(id), after.get(id)) {
            (Some(left), Some(right)) if left != right => {
                diff_entity(left, right, &path, kind, footprints)
            }
            (Some(_), None) | (None, Some(_)) => footprints.push(path),
            _ => {}
        }
    }
}

fn entities_by_id(value: Option<&Value>) -> BTreeMap<String, &Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), item))
        })
        .collect()
}

fn diff_entity(
    before: &Value,
    after: &Value,
    path: &str,
    kind: EntityKind,
    footprints: &mut Vec<String>,
) {
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        footprints.push(path.into());
        return;
    };
    let nested = match kind {
        EntityKind::Scene => Some(("shots", "shot", EntityKind::Shot)),
        EntityKind::Shot => Some(("takes", "take", EntityKind::Take)),
        EntityKind::Take => Some(("panels", "panel", EntityKind::Panel)),
        EntityKind::Panel => Some(("layers", "layer", EntityKind::Leaf)),
        EntityKind::Cut => Some(("clips", "clip", EntityKind::Clip)),
        EntityKind::Clip | EntityKind::Leaf => None,
    };
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        if key.as_str() == "id" || before.get(key) == after.get(key) {
            continue;
        }
        if let Some((field, child, child_kind)) = nested
            && key.as_str() == field
        {
            if entity_order(before.get(key)) != entity_order(after.get(key)) {
                footprints.push(format!("{path}/order"));
            }
            let child_prefix = if matches!(kind, EntityKind::Scene) {
                child.to_owned()
            } else {
                format!("{path}/{child}")
            };
            diff_entities(
                before.get(key),
                after.get(key),
                &child_prefix,
                child_kind,
                footprints,
            );
            continue;
        }
        let footprint = match (kind, key.as_str()) {
            (EntityKind::Shot, "duration") => format!("{path}/timing"),
            (EntityKind::Shot | EntityKind::Take, "refs") => format!("{path}/refs"),
            (EntityKind::Clip, "in" | "out" | "slot") => format!("{path}/trim"),
            _ => format!("{path}/field/{key}"),
        };
        footprints.push(footprint);
    }
}

fn entity_order(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .collect()
}

#[cfg(test)]
mod summary_tests {
    use super::op_summary;
    use storyboard_core::{Author, Format, Layout, Op, Pin, Project, Sticky};

    fn project_with_sticky(resolved: bool) -> Project {
        Project {
            schema: "nb-storyboard/v1".into(),
            title: "Summary test".into(),
            fps: 24,
            format: Format {
                width: 1,
                height: 1,
                extra: Default::default(),
            },
            scenes: Vec::new(),
            cuts: Vec::new(),
            active_cut: None,
            stickies: vec![Sticky {
                id: "sticky-1".into(),
                text: "note".into(),
                kind: "note".into(),
                author: Author::Human,
                pin: Pin::None,
                reply_to: None,
                resolved,
                created: "2026-11-07T00:00:00Z".into(),
                extra: Default::default(),
            }],
            layout: Layout {
                positions: Default::default(),
                extra: Default::default(),
            },
            extra: Default::default(),
        }
    }

    #[test]
    fn summaries_use_the_surface_names_and_report_reopened_threads() {
        let reply = Op::ReplySticky {
            sticky: "sticky-1".into(),
            id: None,
            text: "ok".into(),
            kind: "comment".into(),
            author: Author::Human,
            created: "2026-11-07T00:00:00Z".into(),
        };
        let open = project_with_sticky(false);
        let resolved = project_with_sticky(true);
        assert_eq!(op_summary(&reply, &open, &open), "replied to note");
        assert_eq!(
            op_summary(&reply, &resolved, &resolved),
            "replied to resolved note; reopened thread"
        );
        let across = Op::MoveShotBetweenScenes {
            shot: "shot-1".into(),
            scene: "scene-2".into(),
            index: 0,
        };
        assert_eq!(
            op_summary(&across, &open, &open),
            "moved \u{201c}shot-1\u{201d} to \u{201c}scene-2\u{201d}"
        );
        let within = Op::MoveShot {
            shot: "shot-1".into(),
            index: 2,
        };
        assert_eq!(
            op_summary(&within, &open, &open),
            "reordered shot \u{201c}shot-1\u{201d}"
        );
    }
}
