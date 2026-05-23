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

/// Rank candidate save folders from changed relative paths + the game name.
/// Returns candidates sorted by descending score.
pub fn rank(changed: &[String], game_name: &str) -> Vec<Candidate> {
    let needle = normalize(game_name);

    let mut by_folder: BTreeMap<String, usize> = BTreeMap::new();
    for path in changed {
        let folder = path.split('/').next().filter(|_| path.contains('/')).unwrap_or(".");
        *by_folder.entry(folder.to_string()).or_insert(0) += 1;
    }

    let mut candidates: Vec<Candidate> = by_folder
        .into_iter()
        .map(|(folder, changed_files)| {
            let name_bonus = if folder != "." && name_matches(&needle, &normalize(&folder)) {
                100
            } else {
                0
            };
            Candidate {
                folder,
                changed_files,
                score: name_bonus + changed_files as i64,
            }
        })
        .collect();

    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.folder.cmp(&b.folder)));
    candidates
}

/// Lowercase and strip non-alphanumeric characters for fuzzy name comparison.
fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
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
}
