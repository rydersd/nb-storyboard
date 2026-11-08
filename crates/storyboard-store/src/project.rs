use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use storyboard_core::{
    AgentAuthor, Author, Format, Layout, Op, OpError, Project, ValidationIssue, apply,
    footprint::disjoint, validate,
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
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum CommitError {
    #[error("empty commit")]
    EmptyCommit,
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
    let bytes = serialize_project(&project)?;
    write_atomic(dir, &bytes)?;
    Ok(Loaded {
        project,
        version: version_of(&bytes),
        dir: dir.to_path_buf(),
    })
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
    let _lock = ProjectLock::acquire(dir).map_err(StoreError::from)?;
    repair_log_tail_for_dir(dir).map_err(CommitError::Store)?;
    recover_pending_commit(dir).map_err(CommitError::Store)?;
    commit_locked(dir, base_version, ops, author, note, None, None)
}

fn commit_locked(
    dir: &Path,
    base_version: &str,
    ops: Vec<Op>,
    author: Author,
    note: Option<String>,
    forced_footprints: Option<Vec<String>>,
    undoes: Option<String>,
) -> Result<CommitRecord, CommitError> {
    let (project, old_bytes) = read_project(dir).map_err(CommitError::Store)?;
    let current_version = version_of(&old_bytes);
    let records = synchronize_external_edit(dir, &project, &old_bytes, &current_version)
        .map_err(CommitError::Store)?;

    // Applying before the rebase decision deliberately surfaces a real bad op before a stale-base conflict.
    let mut candidate = project.clone();
    let mut inverses = Vec::new();
    let mut ours = Vec::new();
    let mut created = Vec::new();
    for op in &ops {
        let applied = apply(&mut candidate, op).map_err(CommitError::Op)?;
        created.extend(applied.created_ids);
        inverses.splice(0..0, applied.inverse);
        ours.extend(applied.footprints);
    }
    let mut ours = forced_footprints.unwrap_or(ours);
    ours.sort();
    ours.dedup();
    let append_only_scenes = ops
        .iter()
        .all(|op| matches!(op, Op::AddScene { index: None, .. }));
    let stale = base_version != current_version;
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
        .unwrap_or_else(|| summary_for(&ops, &intervening));
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
        undoes,
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
    _base_version: &str,
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
    let already_undone: BTreeSet<_> = records
        .iter()
        .filter_map(|record| record.undoes.clone())
        .collect();
    let selected: Vec<_> = records
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, record)| {
            !record.external
                && record.undoes.is_none()
                && !already_undone.contains(&record.version)
                && record.author == author
        })
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
    let selected_positions: BTreeSet<_> = selected.iter().copied().collect();
    for &position in &selected {
        let mut later_positions: Vec<_> = (position + 1..records.len())
            .filter(|index| !selected_positions.contains(index))
            .collect();
        let mut cancelled = BTreeSet::new();
        for &undo_index in &later_positions {
            let Some(undone_version) = records[undo_index].undoes.as_deref() else {
                continue;
            };
            if let Some(original_index) = later_positions.iter().rev().copied().find(|index| {
                *index < undo_index
                    && records[*index].undoes.is_none()
                    && !records[*index].external
                    && records[*index].version == undone_version
            }) {
                cancelled.insert(original_index);
                cancelled.insert(undo_index);
            }
        }
        later_positions.retain(|index| !cancelled.contains(index));

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
            Some(target.footprints.clone()),
            Some(target.version.clone()),
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
        author: recovered_author(),
        ops: Vec::new(),
        inverses: Vec::new(),
        footprints,
        rebased: false,
        note: None,
        summary: "external edit".into(),
        external: true,
        undoes: None,
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

fn recovered_author() -> Author {
    Author::Agent {
        agent: AgentAuthor {
            name: "recovered".into(),
            extra: Map::new(),
        },
        extra: Map::new(),
    }
}
fn summary_for(ops: &[Op], intervening: &[CommitRecord]) -> String {
    let prefix = if intervening.is_empty() {
        ""
    } else {
        "rebased "
    };
    let detail = ops.iter().map(op_summary).collect::<Vec<_>>().join("; ");
    format!("{prefix}{detail}")
}

fn op_summary(op: &Op) -> String {
    match op {
        Op::Batch { ops } => format!(
            "batch ({})",
            ops.iter().map(op_summary).collect::<Vec<_>>().join("; ")
        ),
        Op::AddScene { .. } => "add-scene".into(),
        Op::RemoveScene { scene } => format!("remove-scene {scene}"),
        Op::MoveScene { scene, index } => format!("move-scene {scene} to {index}"),
        Op::UpdateScene { scene, .. } => format!("update-scene {scene}"),
        Op::AddShot { scene, .. } => format!("add-shot to {scene}"),
        Op::RemoveShot { shot } => format!("remove-shot {shot}"),
        Op::MoveShot { shot, index } => format!("move-shot {shot} to {index}"),
        Op::UpdateShot { shot, .. } => format!("update-shot {shot}"),
        Op::AddTake { shot, .. } => format!("add-take to {shot}"),
        Op::RemoveTake { shot, take } => format!("remove-take {shot}/{take}"),
        Op::MoveTake { shot, take, index } => format!("move-take {shot}/{take} to {index}"),
        Op::UpdateTake { shot, take, .. } => format!("update-take {shot}/{take}"),
        Op::SelectTake { shot, take } => format!("select-take {shot}/{take}"),
        Op::DuplicateTake { shot, take, .. } => format!("duplicate-take {shot}/{take}"),
        Op::AddPanel { shot, take, .. } => format!("add-panel to {shot}/{take}"),
        Op::RemovePanel { shot, take, panel } => format!("remove-panel {shot}/{take}/{panel}"),
        Op::MovePanel {
            shot,
            take,
            panel,
            index,
        } => {
            format!("move-panel {shot}/{take}/{panel} to {index}")
        }
        Op::UpdatePanel {
            shot, take, panel, ..
        } => format!("update-panel {shot}/{take}/{panel}"),
        Op::AddLayer {
            shot,
            take,
            panel,
            layer_type,
            ..
        } => {
            format!("add-{layer_type} to {shot}/{take}/{panel}")
        }
        Op::RemoveLayer {
            shot,
            take,
            panel,
            layer,
        } => {
            format!("remove-layer {shot}/{take}/{panel}/{layer}")
        }
        Op::MoveLayer {
            shot,
            take,
            panel,
            layer,
            index,
        } => {
            format!("move-layer {shot}/{take}/{panel}/{layer} to {index}")
        }
        Op::UpdateLayer {
            shot,
            take,
            panel,
            layer,
            ..
        } => {
            format!("update-layer {shot}/{take}/{panel}/{layer}")
        }
        Op::AddSticky { .. } => "note".into(),
        Op::RemoveSticky { sticky } => format!("remove-note {sticky}"),
        Op::MoveSticky { sticky, index } => format!("move-note {sticky} to {index}"),
        Op::UpdateSticky { sticky, .. } => format!("update-note {sticky}"),
        Op::AddClip { cut, .. } => format!("add-clip to {cut}"),
        Op::RemoveClip { cut, clip } => format!("remove-clip {clip} from {cut}"),
        Op::ReorderClip { cut, clip, index } => format!("reorder-clip {clip} in {cut} to {index}"),
        Op::UpdateClip { cut, clip, .. } => format!("update-clip {clip} in {cut}"),
        Op::AddCut { .. } => "add-cut".into(),
        Op::UpdateCut { cut, .. } => format!("update-cut {cut}"),
        Op::RemoveCut { cut } => format!("remove-cut {cut}"),
        Op::SetActiveCut { cut } => match cut {
            Some(cut) => format!("set-active-cut {cut}"),
            None => "clear-active-cut".into(),
        },
        Op::MoveShotBetweenScenes { shot, scene, index } => {
            format!("move-shot {shot} to {scene} at {index}")
        }
        Op::AddRef {
            target, reference, ..
        } => format!("add-ref {reference} to {target:?}"),
        Op::RemoveRef { target, reference } => format!("remove-ref {reference} from {target:?}"),
        Op::ResolveSticky { sticky } => format!("resolve {sticky}"),
        Op::ReplySticky { sticky, .. } => format!("reply to {sticky}"),
        Op::MoveNode { node, position } => match position {
            Some(_) => format!("move-node {node}"),
            None => format!("clear-node-position {node}"),
        },
        Op::RestoreChanges { .. } => "undo".into(),
    }
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
