//! Folder-backed storage for storyboard projects.

pub mod cache;
pub mod history;
pub mod lock;
pub mod project;
pub mod sources;
pub mod version;
pub mod watch;
pub mod workspace;

pub use cache::{cache_put, prune_cache};
pub use history::{CommitRecord, CommitSummary, history, records_since};
pub use project::{CommitError, Loaded, StoreError, commit, create, open, undo};
pub use sources::{
    MissingRef, SourceDimensions, SourceEntry, SourceUse, check_refs, import_source, scan_sources,
};
pub use version::version_of;
pub use watch::{Change, watch};
pub use workspace::{IndexEntry, Workspace, index, open_workspace};
