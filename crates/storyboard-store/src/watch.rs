use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use crate::{StoreError, Workspace, version_of};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    Project { dir: PathBuf, version: String },
    Source { dir: PathBuf, path: PathBuf },
}

#[derive(Clone)]
struct WatchedProject {
    reported_dir: PathBuf,
    canonical_dir: PathBuf,
    project_file: PathBuf,
    sources_dir: PathBuf,
}

/// Watches each document and every nested source directory. Source files do not need commits.
pub fn watch(workspace: &Workspace) -> Result<(RecommendedWatcher, Receiver<Change>), StoreError> {
    let (sender, receiver) = mpsc::channel();
    let projects: Vec<_> = workspace
        .projects
        .iter()
        .map(|dir| {
            let canonical_dir = dir.canonicalize()?;
            Ok(WatchedProject {
                reported_dir: dir.clone(),
                project_file: canonical_dir.join("project.json"),
                sources_dir: canonical_dir.join("sources"),
                canonical_dir,
            })
        })
        .collect::<std::io::Result<_>>()?;
    let callback_projects = projects.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else {
            return;
        };
        for event_path in &event.paths {
            let Some(path) = canonical_event_path(event_path) else {
                continue;
            };
            for project in &callback_projects {
                if path == project.project_file {
                    if let Ok(bytes) = std::fs::read(&project.project_file) {
                        let _ = sender.send(Change::Project {
                            dir: project.reported_dir.clone(),
                            version: version_of(&bytes),
                        });
                    }
                } else if path.starts_with(&project.sources_dir) {
                    let relative = path
                        .strip_prefix(&project.canonical_dir)
                        .unwrap_or(&path)
                        .to_path_buf();
                    let _ = sender.send(Change::Source {
                        dir: project.reported_dir.clone(),
                        path: relative,
                    });
                }
            }
        }
    })
    .map_err(|error| StoreError::Io(std::io::Error::other(error)))?;
    for project in &projects {
        watcher
            .watch(&project.project_file, RecursiveMode::NonRecursive)
            .map_err(|error| StoreError::Io(std::io::Error::other(error)))?;
        watcher
            .watch(&project.sources_dir, RecursiveMode::Recursive)
            .map_err(|error| StoreError::Io(std::io::Error::other(error)))?;
    }
    Ok((watcher, receiver))
}

fn canonical_event_path(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().or_else(|| {
        let parent = path.parent()?.canonicalize().ok()?;
        Some(parent.join(path.file_name()?))
    })
}
