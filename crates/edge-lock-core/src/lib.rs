//! Local asset catalog and immutable file snapshots for Edge Lock.
mod domain;
mod files;
mod library;

pub mod blender;
pub mod glb;
pub mod preview;
pub mod sync;

pub use domain::*;
pub use library::{
    ArchiveDeletionAsset, ArchiveDeletionPlan, ArchiveDeletionReport, FileSelectionKind,
    FileSelections, Library, StorageAction, VariantDeletionPlan, WorkspaceCheck,
    WorkspaceDiscardReport,
};
