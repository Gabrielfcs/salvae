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
    games.iter().find(|g| exe_path.starts_with(&g.install_dir))
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
}
