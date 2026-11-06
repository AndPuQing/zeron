//! Directory classification for host folder browsing and name search.

use std::fs::FileType;
use std::path::Path;

/// Follow a link only to classify its target; callers keep the link's path.
/// This does not authorize workspace reads or enable recursive link traversal.
/// Missing, cyclic or inaccessible targets are not browsable directories.
pub(crate) fn is_directory(path: &Path, kind: Option<FileType>) -> bool {
    kind.is_some_and(|kind| kind.is_dir() || (kind.is_symlink() && path.is_dir()))
}
