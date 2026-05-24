//! Installed-game model and process matching.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Which launcher a game was installed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Launcher {
    Steam,
    Epic,
}

/// A game discovered on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledGame {
    /// Stable cross-member id, e.g. `steam:892970` or `epic:Valheim`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Which launcher installed it.
    pub launcher: Launcher,
    /// Install directory on this machine.
    pub install_dir: PathBuf,
}

/// Return the first game whose `install_dir` is a prefix of `exe_path`
/// (i.e., the running executable belongs to that game).
pub fn match_process_to_game<'a>(
    games: &'a [InstalledGame],
    exe_path: &Path,
) -> Option<&'a InstalledGame> {
    games
        .iter()
        .find(|g| path_starts_with_ci(exe_path, &g.install_dir))
}

/// Case-insensitive, component-wise prefix test: is `prefix` a leading run of
/// `path`'s components? Windows paths are case-insensitive, and the same folder
/// can be reported with different casing (the registry's Steam path often has a
/// lowercase drive, e.g. `c:\...`, while the OS reports a running process as
/// `C:\...`). A plain `Path::starts_with` is case-sensitive and would miss the
/// match. Comparing per component (not as raw strings) keeps the `/` boundary,
/// so `.../Valheim` never matches `.../Valheim2/...`.
fn path_starts_with_ci(path: &Path, prefix: &Path) -> bool {
    let mut path_components = path.components();
    for want in prefix.components() {
        match path_components.next() {
            Some(have) if have.as_os_str().eq_ignore_ascii_case(want.as_os_str()) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, dir: &str) -> InstalledGame {
        InstalledGame {
            id: id.into(),
            name: id.into(),
            launcher: Launcher::Steam,
            install_dir: PathBuf::from(dir),
        }
    }

    #[test]
    fn matches_process_under_install_dir() {
        let games = vec![
            game("steam:1", "C:/Steam/common/Valheim"),
            game("steam:2", "C:/Steam/common/Terraria"),
        ];
        let m = match_process_to_game(&games, Path::new("C:/Steam/common/Valheim/valheim.exe"));
        assert_eq!(m.unwrap().id, "steam:1");
    }

    #[test]
    fn no_match_outside_any_install_dir() {
        let games = vec![game("steam:1", "C:/Steam/common/Valheim")];
        assert!(match_process_to_game(&games, Path::new("C:/Windows/notepad.exe")).is_none());
    }

    #[test]
    fn match_is_case_insensitive_for_drive_and_folders() {
        // install_dir as the registry might report it (lowercase drive + folder),
        // process path as the OS reports it (real on-disk casing).
        let games = vec![game("steam:1", "c:/steam/common/valheim")];
        let m = match_process_to_game(&games, Path::new("C:/Steam/Common/Valheim/valheim.exe"));
        assert_eq!(m.unwrap().id, "steam:1");
    }

    #[test]
    fn prefix_must_align_on_component_boundaries() {
        // `Valheim` install dir must not match a sibling `Valheim2` process.
        let games = vec![game("steam:1", "C:/Steam/common/Valheim")];
        assert!(
            match_process_to_game(&games, Path::new("C:/Steam/common/Valheim2/game.exe")).is_none()
        );
    }
}
