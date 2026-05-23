//! Steam launcher enumeration.

use std::path::Path;

use crate::game::{InstalledGame, Launcher};
use crate::vdf;
use crate::DetectError;

/// Parse every `appmanifest_*.acf` in `library/steamapps` into games. Their
/// install dirs are `library/steamapps/common/<installdir>`.
pub fn games_in_library(library: &Path) -> Result<Vec<InstalledGame>, DetectError> {
    let apps = library.join("steamapps");
    let mut games = Vec::new();
    let entries = match std::fs::read_dir(&apps) {
        Ok(e) => e,
        Err(_) => return Ok(games), // no steamapps dir => no games
    };
    for entry in entries {
        let entry = entry.map_err(|e| DetectError::Io(e.to_string()))?;
        let path = entry.path();
        let is_acf = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
            .unwrap_or(false);
        if !is_acf {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| DetectError::Io(e.to_string()))?;
        if let Some(game) = game_from_acf(&text, &apps)? {
            games.push(game);
        }
    }
    Ok(games)
}

fn game_from_acf(text: &str, apps: &Path) -> Result<Option<InstalledGame>, DetectError> {
    let doc = vdf::parse(text)?;
    let app = match doc
        .as_obj()
        .and_then(|m| m.get("AppState"))
        .and_then(|v| v.as_obj())
    {
        Some(a) => a,
        None => return Ok(None),
    };
    let appid = app.get("appid").and_then(|v| v.as_str());
    let name = app.get("name").and_then(|v| v.as_str());
    let installdir = app.get("installdir").and_then(|v| v.as_str());
    match (appid, name, installdir) {
        (Some(appid), Some(name), Some(installdir)) => Ok(Some(InstalledGame {
            id: format!("steam:{appid}"),
            name: name.to_string(),
            launcher: Launcher::Steam,
            install_dir: apps.join("common").join(installdir),
        })),
        _ => Ok(None),
    }
}

/// Read the library paths listed in a `libraryfolders.vdf`.
pub fn library_paths(library_folders_vdf: &Path) -> Result<Vec<std::path::PathBuf>, DetectError> {
    let text =
        std::fs::read_to_string(library_folders_vdf).map_err(|e| DetectError::Io(e.to_string()))?;
    let doc = vdf::parse(&text)?;
    let mut paths = Vec::new();
    if let Some(lf) = doc
        .as_obj()
        .and_then(|m| m.get("libraryfolders"))
        .and_then(|v| v.as_obj())
    {
        for entry in lf.values() {
            if let Some(path) = entry
                .as_obj()
                .and_then(|m| m.get("path"))
                .and_then(|v| v.as_str())
            {
                paths.push(std::path::PathBuf::from(path));
            }
        }
    }
    Ok(paths)
}

/// Enumerate all Steam games: the main library at `steam_root` plus any extra
/// libraries listed in `steam_root/steamapps/libraryfolders.vdf`.
pub fn enumerate(steam_root: &Path) -> Result<Vec<InstalledGame>, DetectError> {
    let mut libraries = vec![steam_root.to_path_buf()];
    let lf = steam_root.join("steamapps").join("libraryfolders.vdf");
    if lf.exists() {
        for extra in library_paths(&lf)? {
            if !libraries.contains(&extra) {
                libraries.push(extra);
            }
        }
    }
    let mut games = Vec::new();
    for lib in &libraries {
        games.extend(games_in_library(lib)?);
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_acf(dir: &Path, file: &str, appid: &str, name: &str, installdir: &str) {
        let content = format!(
            "\"AppState\"\n{{\n  \"appid\" \"{appid}\"\n  \"name\" \"{name}\"\n  \"installdir\" \"{installdir}\"\n}}\n"
        );
        std::fs::write(dir.join(file), content).unwrap();
    }

    #[test]
    fn parses_one_acf_into_a_game() {
        let lib = tempfile::tempdir().unwrap();
        let apps = lib.path().join("steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        make_acf(
            &apps,
            "appmanifest_892970.acf",
            "892970",
            "Valheim",
            "Valheim",
        );

        let games = games_in_library(lib.path()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, "steam:892970");
        assert_eq!(games[0].name, "Valheim");
        assert_eq!(games[0].launcher, Launcher::Steam);
        assert_eq!(games[0].install_dir, apps.join("common").join("Valheim"));
    }

    #[test]
    fn library_folders_lists_extra_libraries() {
        let root = tempfile::tempdir().unwrap();
        let lib2 = root.path().join("lib2");
        std::fs::create_dir_all(lib2.join("steamapps")).unwrap();
        let escaped = lib2.to_string_lossy().replace('\\', "\\\\");
        let content = format!("\"libraryfolders\"\n{{\n  \"0\" {{ \"path\" \"{escaped}\" }}\n}}\n");
        let lf = root.path().join("libraryfolders.vdf");
        std::fs::write(&lf, content).unwrap();

        let libs = library_paths(&lf).unwrap();
        assert!(libs.iter().any(|p| p == &lib2));
    }

    #[test]
    fn enumerate_scans_all_libraries() {
        // A steam root whose steamapps has its own game + a second library.
        let steam = tempfile::tempdir().unwrap();
        let main_apps = steam.path().join("steamapps");
        std::fs::create_dir_all(&main_apps).unwrap();
        make_acf(&main_apps, "appmanifest_1.acf", "1", "GameOne", "GameOne");

        let lib2 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(lib2.path().join("steamapps")).unwrap();
        make_acf(
            &lib2.path().join("steamapps"),
            "appmanifest_2.acf",
            "2",
            "GameTwo",
            "GameTwo",
        );

        let escaped = lib2.path().to_string_lossy().replace('\\', "\\\\");
        std::fs::write(
            main_apps.join("libraryfolders.vdf"),
            format!("\"libraryfolders\"\n{{\n  \"0\" {{ \"path\" \"{escaped}\" }}\n}}\n"),
        )
        .unwrap();

        let mut games = enumerate(steam.path()).unwrap();
        games.sort_by(|a, b| a.id.cmp(&b.id));
        let ids: Vec<&str> = games.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["steam:1", "steam:2"]);
    }
}
