//! Automatically resolve a game's save folder — no user snapshot/diff needed.
//!
//! Order: the curated Ludusavi-derived manifest (resolved to an existing path)
//! first, then a bounded heuristic search of the standard roots (folders that
//! match the game name, are named `save`/`saves`, or hold save-like files).

use std::path::{Path, PathBuf};

use crate::manifest::{normalize, Manifest, Placeholders};

/// Common save-file extensions (lowercase, without the dot).
const SAVE_EXTS: &[&str] = &["sav", "save", "sv", "es3", "slot", "profile"];
/// Folder names that commonly hold saves.
const SAVE_DIR_NAMES: &[&str] = &["save", "saves", "savegame", "savegames", "savedata"];

/// Best-effort automatic resolution of a game's save folder.
pub fn find_save_dir(
    game_name: &str,
    steam_id: Option<u64>,
    manifest: &Manifest,
    placeholders: &Placeholders,
    roots: &[PathBuf],
) -> Option<PathBuf> {
    // 1. Curated manifest — resolve templates and take the first that exists.
    for template in manifest.paths_for(steam_id, game_name) {
        if let Some(path) = placeholders.resolve(&template) {
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    // 2. Heuristic filesystem search.
    heuristic(game_name, roots)
}

fn heuristic(game_name: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let needle = normalize(game_name);
    let mut best: Option<(i64, PathBuf)> = None;
    for root in roots {
        walk(root, &needle, 0, 3, &mut best);
    }
    best.map(|(_, path)| path)
}

fn walk(
    dir: &Path,
    needle: &str,
    depth: usize,
    max_depth: usize,
    best: &mut Option<(i64, PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let score = score_dir(&path, &name, needle);
        if score > 0 {
            match best {
                Some((s, _)) if *s >= score => {}
                _ => *best = Some((score, path.clone())),
            }
        }
        if depth + 1 < max_depth {
            walk(&path, needle, depth + 1, max_depth, best);
        }
    }
}

/// Score how likely `path` (named `name`) is a save folder for `needle`.
fn score_dir(path: &Path, name: &str, needle: &str) -> i64 {
    let norm = normalize(name);
    let mut score = 0i64;
    if name_matches(needle, &norm) {
        score += 100;
    }
    if SAVE_DIR_NAMES.contains(&norm.as_str()) {
        score += 40;
        // Strong signal: a `save` folder directly under the game's folder.
        if let Some(parent) = path.parent().and_then(Path::file_name) {
            if name_matches(needle, &normalize(&parent.to_string_lossy())) {
                score += 60;
            }
        }
    }
    let save_files = count_save_files(path);
    if save_files > 0 {
        score += 10 + save_files.min(20) as i64;
    }
    score
}

fn count_save_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| SAVE_EXTS.contains(&x.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .count()
}

/// Whether two normalized names plausibly refer to the same game.
fn name_matches(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    short.len() >= 3 && long.contains(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn manifest_hit_wins_when_path_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("DDTNL/Supermarket Together");
        std::fs::create_dir_all(&game_dir).unwrap();
        let manifest = Manifest::from_json(
            r#"[{"name":"Supermarket Together","steam_id":2709570,"paths":["<home>/DDTNL/Supermarket Together"]}]"#,
        )
        .unwrap();
        let ph = Placeholders {
            home: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let found = find_save_dir("Supermarket Together", Some(2709570), &manifest, &ph, &[]);
        assert_eq!(found, Some(game_dir));
    }

    #[test]
    fn heuristic_finds_game_named_folder() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("IronGate/Valheim");
        write(&game.join("worlds/Midgard.fwl"));
        let found = find_save_dir(
            "Valheim",
            None,
            &Manifest::default(),
            &Placeholders::default(),
            &[root.path().to_path_buf()],
        );
        assert_eq!(found, Some(game));
    }

    #[test]
    fn heuristic_finds_save_subfolder_by_files() {
        let root = tempfile::tempdir().unwrap();
        let saves = root.path().join("SomeStudio/Saves");
        write(&saves.join("slot1.sav"));
        let found = find_save_dir(
            "Totally Unrelated Title",
            None,
            &Manifest::default(),
            &Placeholders::default(),
            &[root.path().to_path_buf()],
        );
        assert_eq!(found, Some(saves));
    }

    #[test]
    fn nothing_relevant_yields_none() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("Random/notes.txt"));
        let found = find_save_dir(
            "Valheim",
            None,
            &Manifest::default(),
            &Placeholders::default(),
            &[root.path().to_path_buf()],
        );
        assert_eq!(found, None);
    }
}
