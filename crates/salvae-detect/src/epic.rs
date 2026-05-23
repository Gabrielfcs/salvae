//! Epic launcher enumeration.

use std::path::Path;

use serde::Deserialize;

use crate::game::{InstalledGame, Launcher};
use crate::DetectError;

#[derive(Deserialize)]
struct EpicManifest {
    #[serde(rename = "AppName")]
    app_name: String,
    #[serde(rename = "DisplayName")]
    display_name: Option<String>,
    #[serde(rename = "InstallLocation")]
    install_location: String,
}

/// Enumerate Epic games by parsing every `*.item` manifest in `manifests_dir`
/// (typically `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`). Malformed
/// manifests are skipped.
pub fn enumerate(manifests_dir: &Path) -> Result<Vec<InstalledGame>, DetectError> {
    let mut games = Vec::new();
    let entries = match std::fs::read_dir(manifests_dir) {
        Ok(e) => e,
        Err(_) => return Ok(games),
    };
    for entry in entries {
        let entry = entry.map_err(|e| DetectError::Io(e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| DetectError::Io(e.to_string()))?;
        let manifest: EpicManifest = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue, // skip malformed manifests
        };
        let name = manifest
            .display_name
            .unwrap_or_else(|| manifest.app_name.clone());
        games.push(InstalledGame {
            id: format!("epic:{}", manifest.app_name),
            name,
            launcher: Launcher::Epic,
            install_dir: std::path::PathBuf::from(manifest.install_location),
        });
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(dir: &Path, file: &str, app_name: &str, display: &str, location: &str) {
        let json = format!(
            "{{ \"AppName\": \"{app_name}\", \"DisplayName\": \"{display}\", \"InstallLocation\": {location} }}"
        );
        std::fs::write(dir.join(file), json).unwrap();
    }

    #[test]
    fn parses_one_manifest_into_a_game() {
        let dir = tempfile::tempdir().unwrap();
        // InstallLocation must be a JSON string; build it from a real temp path.
        let loc = dir.path().join("Fortnite");
        let loc_json = serde_json::to_string(&loc).unwrap();
        make_item(dir.path(), "abc.item", "Fortnite", "Fortnite", &loc_json);

        let games = enumerate(dir.path()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, "epic:Fortnite");
        assert_eq!(games[0].name, "Fortnite");
        assert_eq!(games[0].launcher, Launcher::Epic);
        assert_eq!(games[0].install_dir, loc);
    }

    #[test]
    fn ignores_non_item_files_and_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignore me").unwrap();
        std::fs::write(dir.path().join("broken.item"), "{ not json").unwrap();
        let loc_json = serde_json::to_string(&dir.path().join("Game")).unwrap();
        make_item(dir.path(), "ok.item", "Game", "Game", &loc_json);

        let games = enumerate(dir.path()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, "epic:Game");
    }

    #[test]
    fn missing_dir_yields_no_games() {
        let dir = tempfile::tempdir().unwrap();
        assert!(enumerate(&dir.path().join("does-not-exist"))
            .unwrap()
            .is_empty());
    }
}
