//! Auto-discovery: snapshot a set of roots before a game runs, then diff and
//! rank changed folders after it closes. Absolute-path candidates the user
//! confirms into a `set_game_path`.

use std::path::{Path, PathBuf};

use salvae_detect::candidate::rank;
use salvae_detect::snapshot::{capture, diff, Snapshot};
use salvae_detect::DetectError;

use crate::view::DiscoveredCandidate;

/// How deep to scan each root (root -> folder -> subfolder -> file).
pub const SCAN_DEPTH: usize = 4;

/// The "before" snapshots captured when a scan is armed.
#[derive(Debug, Clone, Default)]
pub struct ArmedScan {
    /// (root, snapshot-before) pairs.
    pub roots: Vec<(PathBuf, Snapshot)>,
}

/// Capture "before" snapshots for every root (missing roots snapshot empty).
pub fn arm(roots: &[PathBuf]) -> Result<ArmedScan, DetectError> {
    let mut out = Vec::new();
    for root in roots {
        let snap = capture(root, SCAN_DEPTH)?;
        out.push((root.clone(), snap));
    }
    Ok(ArmedScan { roots: out })
}

/// Re-snapshot each armed root, diff, rank by `game_name`, and return absolute
/// candidate folders sorted by descending score.
pub fn collect(
    armed: &ArmedScan,
    game_name: &str,
) -> Result<Vec<DiscoveredCandidate>, DetectError> {
    let mut candidates = Vec::new();
    for (root, before) in &armed.roots {
        let after = capture(root, SCAN_DEPTH)?;
        let changed = diff(before, &after);
        for c in rank(&changed, game_name) {
            candidates.push(DiscoveredCandidate {
                folder: abs_folder(root, &c.folder),
                changed_files: c.changed_files,
                score: c.score,
            });
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.folder.cmp(&b.folder)));
    Ok(candidates)
}

/// Turn a candidate's (possibly nested, `/`-separated) folder into an absolute
/// path under `root`. `rank` uses `"."` for files written directly into the
/// root.
fn abs_folder(root: &Path, folder: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    if folder != "." {
        for component in folder.split('/') {
            path.push(component);
        }
    }
    path
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
    fn discovers_changed_folder_as_absolute_candidate() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "Valheim/old.db", b"x");
        let armed = arm(&[root.path().to_path_buf()]).unwrap();

        // The game writes new files into its save folder.
        write(root.path(), "Valheim/worlds/Midgard.db", b"new world");
        write(root.path(), "Valheim/worlds/Midgard.fwl", b"meta");

        let found = collect(&armed, "Valheim").unwrap();
        assert_eq!(found[0].folder, root.path().join("Valheim"));
        assert!(found[0].score > 0);
    }

    #[test]
    fn no_changes_yields_no_candidates() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "Game/save.db", b"x");
        let armed = arm(&[root.path().to_path_buf()]).unwrap();
        // Nothing changes between arm and collect.
        let found = collect(&armed, "Game").unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn higher_scoring_root_ranks_first_across_roots() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let armed = arm(&[a.path().to_path_buf(), b.path().to_path_buf()]).unwrap();

        // Root A: one unrelated changed file. Root B: a name-matching folder.
        write(a.path(), "Misc/tmp.dat", b"1");
        write(b.path(), "Terraria/Players/p1.plr", b"hero");

        let found = collect(&armed, "Terraria").unwrap();
        assert_eq!(found[0].folder, b.path().join("Terraria"));
    }
}
