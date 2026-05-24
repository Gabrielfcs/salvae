//! Save-folder candidate ranking.
//!
//! Given the relative paths a game changed (from a snapshot diff) and the
//! game's name, group changes by their top-level folder and rank those folders
//! as likely save locations: more changed files and a name resembling the game
//! score higher.

use std::collections::BTreeMap;

/// A ranked save-folder candidate (top-level folder name under the scanned root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub folder: String,
    pub changed_files: usize,
    pub score: i64,
}

/// Top-level folders that almost never hold game saves — they show up only
/// because lots of apps write under `%LocalAppData%`. Filtered out (unless the
/// folder name actually matches the game).
const NOISE: &[&str] = &[
    "nvidia",
    "nvidiacorporation",
    "microsoft",
    "temp",
    "tempstate",
    "asus",
    "packages",
    "crashdumps",
    "d3dscache",
    "google",
    "mozilla",
    "comms",
    "connecteddevicesplatform",
    "powertoys",
    "diagnostics",
    "programs",
];

/// Rank candidate save folders from changed relative paths + the game name.
///
/// For each changed file, the candidate folder is the path **up to and
/// including the deepest component that matches the game name** (e.g.
/// `DDTNL/Supermarket Together`), or the top-level folder if nothing matches.
/// Name-matching folders score highest; obvious noise is dropped. Returns
/// candidates sorted by descending score.
pub fn rank(changed: &[String], game_name: &str) -> Vec<Candidate> {
    let needle = normalize(game_name);

    let mut by_folder: BTreeMap<String, usize> = BTreeMap::new();
    for path in changed {
        let comps: Vec<&str> = path.split('/').collect();
        // Directory components (drop the file name).
        let dirs = &comps[..comps.len().saturating_sub(1)];
        *by_folder.entry(candidate_key(dirs, &needle)).or_insert(0) += 1;
    }

    let mut candidates: Vec<Candidate> = by_folder
        .into_iter()
        .filter_map(|(folder, changed_files)| {
            let first = folder.split('/').next().unwrap_or("");
            let last = folder.rsplit('/').next().unwrap_or("");
            let name_match = folder != "." && name_matches(&needle, &normalize(last));
            // Drop obvious noise unless the folder itself matches the game.
            if !name_match && NOISE.contains(&normalize(first).as_str()) {
                return None;
            }
            let score = if name_match { 100 } else { 0 } + changed_files as i64;
            Some(Candidate {
                folder,
                changed_files,
                score,
            })
        })
        .collect();

    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.folder.cmp(&b.folder)));
    candidates
}

/// The candidate folder for one file's directory components: the path up to and
/// including the deepest component that matches the game name, else the
/// top-level folder (`"."` for a file directly under the root).
fn candidate_key(dirs: &[&str], needle: &str) -> String {
    if dirs.is_empty() {
        return ".".to_string();
    }
    for (i, comp) in dirs.iter().enumerate() {
        if name_matches(needle, &normalize(comp)) {
            return dirs[..=i].join("/");
        }
    }
    dirs[0].to_string()
}

/// Lowercase and strip non-alphanumeric characters for fuzzy name comparison.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Whether two normalized names plausibly refer to the same game (one contains
/// the other), ignoring trivially short matches.
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

    #[test]
    fn ranks_name_matching_folder_highest() {
        let changed = vec![
            "Valheim/worlds/Midgard.db".to_string(),
            "Valheim/worlds/Midgard.fwl".to_string(),
            "SomeOtherApp/cache/x.tmp".to_string(),
        ];
        let ranked = rank(&changed, "Valheim");
        assert_eq!(ranked[0].folder, "Valheim");
        assert_eq!(ranked[0].changed_files, 2);
        // The name-matching folder outranks the unrelated one.
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn loose_files_grouped_under_root_marker() {
        let changed = vec!["loose.sav".to_string()];
        let ranked = rank(&changed, "Whatever");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].folder, ".");
    }

    #[test]
    fn empty_diff_yields_no_candidates() {
        assert!(rank(&[], "Game").is_empty());
    }

    #[test]
    fn drills_into_studio_game_subfolder_and_drops_noise() {
        let changed = vec![
            "DDTNL/Supermarket Together/save.es3".to_string(),
            "DDTNL/Supermarket Together/settings.cfg".to_string(),
            "NVIDIA/GLCache/abc.bin".to_string(),
            "Microsoft/Edge/cache.dat".to_string(),
        ];
        let ranked = rank(&changed, "Supermarket Together");
        // Best candidate is the studio/game folder, not just "DDTNL".
        assert_eq!(ranked[0].folder, "DDTNL/Supermarket Together");
        assert_eq!(ranked[0].changed_files, 2);
        // Noise top-level folders are filtered out.
        assert!(ranked
            .iter()
            .all(|c| c.folder != "NVIDIA" && c.folder != "Microsoft"));
    }
}
