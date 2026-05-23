//! End-to-end: enumerate a fixture Steam library, then use snapshot/diff/rank
//! to locate a game's save folder after it "writes" one.

use std::path::Path;

use salvae_detect::{candidate, game, snapshot, steam};

fn write(dir: &Path, rel: &str, content: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn enumerate_then_discover_save_folder() {
    // --- A fixture Steam install with one game. ---
    let steam = tempfile::tempdir().unwrap();
    let apps = steam.path().join("steamapps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(
        apps.join("appmanifest_892970.acf"),
        "\"AppState\"\n{\n  \"appid\" \"892970\"\n  \"name\" \"Valheim\"\n  \"installdir\" \"Valheim\"\n}\n",
    )
    .unwrap();

    let games = steam::enumerate(steam.path()).unwrap();
    assert_eq!(games.len(), 1);
    let valheim = &games[0];
    assert_eq!(valheim.id, "steam:892970");

    // A running process under the install dir is matched to the game.
    let exe = apps.join("common").join("Valheim").join("valheim.exe");
    assert_eq!(
        game::match_process_to_game(&games, &exe).map(|g| g.id.as_str()),
        Some("steam:892970")
    );

    // --- Discover the save folder via snapshot/diff/rank. ---
    let appdata = tempfile::tempdir().unwrap(); // stands in for %LocalAppData%
    write(appdata.path(), "Unrelated/cache.tmp", b"junk");
    let before = snapshot::capture(appdata.path(), 4).unwrap();

    // The game "plays": it writes a save under a name-matching folder.
    write(appdata.path(), "Valheim/worlds/Midgard.db", b"world data");
    write(appdata.path(), "Valheim/worlds/Midgard.fwl", b"meta");
    let after = snapshot::capture(appdata.path(), 4).unwrap();

    let changed = snapshot::diff(&before, &after);
    let ranked = candidate::rank(&changed, &valheim.name);
    assert_eq!(ranked[0].folder, "Valheim");
    assert_eq!(ranked[0].changed_files, 2);
}
