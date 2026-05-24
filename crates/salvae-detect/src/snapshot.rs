//! Folder metadata snapshot + diff.
//!
//! Captures file metadata (relative path -> size + mtime), NOT contents, under
//! a bounded depth — cheap enough to run around a game launch. Diffing two
//! snapshots reveals which files a game wrote.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::DetectError;

/// File metadata used to detect changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub size: u64,
    /// Modification time in milliseconds since the Unix epoch (0 if unknown).
    pub mtime_ms: u64,
}

/// A snapshot of file metadata under one root, keyed by `/`-separated relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: BTreeMap<String, Entry>,
}

/// Capture file metadata under `root`, including files whose relative path is
/// at most `max_depth` components deep (so a file directly in `root` is depth
/// 1). Missing roots yield an empty snapshot. Inaccessible subtrees are skipped
/// (the scan never fails on permission errors).
pub fn capture(root: &Path, max_depth: usize) -> Result<Snapshot, DetectError> {
    let mut snap = Snapshot::default();
    if root.is_dir() {
        walk(root, root, 0, max_depth, &mut snap);
    }
    Ok(snap)
}

/// `current_depth` is the number of directory levels below `root` (0 at root).
/// A file in this directory therefore sits at `current_depth + 1` components.
///
/// Best-effort: directories and entries that can't be read (permission denied,
/// junctions, etc. — common under `%LocalAppData%`) are skipped rather than
/// failing the whole scan.
fn walk(root: &Path, dir: &Path, current_depth: usize, max_depth: usize, snap: &mut Snapshot) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return, // unreadable directory — skip it
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue, // skip unreadable entries
        };
        if meta.is_dir() {
            // Only descend if files inside the subdir could still be in range.
            if current_depth + 1 < max_depth {
                walk(root, &path, current_depth + 1, max_depth, snap);
            }
        } else if meta.is_file() && current_depth < max_depth {
            let Ok(stripped) = path.strip_prefix(root) else {
                continue;
            };
            let rel = stripped.to_string_lossy().replace('\\', "/");
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            snap.entries.insert(
                rel,
                Entry {
                    size: meta.len(),
                    mtime_ms,
                },
            );
        }
    }
}

/// Return the relative paths that are new in `after` or differ from `before`
/// (by size or mtime).
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut changed = Vec::new();
    for (path, entry) in &after.entries {
        match before.entries.get(path) {
            Some(old) if old == entry => {}
            _ => changed.push(path.clone()),
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn capture_records_files_within_depth() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "a.txt", b"a");
        write(root.path(), "sub/b.txt", b"bb");
        write(root.path(), "sub/deep/c.txt", b"ccc");

        let snap = capture(root.path(), 2).unwrap();
        assert!(snap.entries.contains_key("a.txt"));
        assert!(snap.entries.contains_key("sub/b.txt"));
        // depth 2 = root(0) -> sub(1) -> files in sub(2); "sub/deep/c.txt" is too deep.
        assert!(!snap.entries.contains_key("sub/deep/c.txt"));
        assert_eq!(snap.entries["a.txt"].size, 1);
    }

    #[test]
    fn diff_reports_added_and_changed_paths() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "keep.txt", b"same");
        write(root.path(), "change.txt", b"old");
        let before = capture(root.path(), 4).unwrap();

        write(root.path(), "change.txt", b"new longer content");
        write(root.path(), "added.txt", b"x");
        let after = capture(root.path(), 4).unwrap();

        let mut changed = diff(&before, &after);
        changed.sort();
        assert_eq!(
            changed,
            vec!["added.txt".to_string(), "change.txt".to_string()]
        );
    }
}
