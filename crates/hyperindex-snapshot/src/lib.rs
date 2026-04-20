pub mod base;
pub mod manifest;
pub mod overlays;

pub use base::build_base_snapshot;
pub use manifest::{ResolvedFile, ResolvedFrom, SnapshotAssembler};
pub use overlays::{build_buffer_overlays, build_working_tree_overlay};
