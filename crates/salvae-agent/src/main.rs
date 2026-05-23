//! Salvaê background agent: load config, build a sync engine per group, and
//! poll for game open/close, pulling on open and pushing on close.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use salvae_agent::agent::{Agent, GroupRuntime};
use salvae_config::dpapi::DpapiSecretStore;
use salvae_config::store::ConfigStore;
use salvae_detect::game::InstalledGame;
use salvae_detect::{epic, steam};
use salvae_discord::discord::DiscordChannel;
use salvae_sync::engine::SyncEngine;
use salvae_sync::state::SyncState;
use salvae_watch::detector::Detector;
use salvae_watch::process::Watcher;
use salvae_watch::system::SystemProcessLister;

/// How often to poll the process list.
const POLL_INTERVAL: Duration = Duration::from_secs(4);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Per-user Salvaê app directory (`%AppData%\salvae`).
fn app_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("salvae")
}

/// Enumerate installed games from the standard Steam and Epic locations.
fn enumerate_games() -> Vec<InstalledGame> {
    let mut games = Vec::new();

    // Steam: default install path (best effort).
    let steam_root = PathBuf::from(r"C:\Program Files (x86)\Steam");
    if steam_root.exists() {
        if let Ok(s) = steam::enumerate(&steam_root) {
            games.extend(s);
        }
    }

    // Epic manifests under %ProgramData%.
    let program_data =
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    let epic_manifests = PathBuf::from(program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    if let Ok(e) = epic::enumerate(&epic_manifests) {
        games.extend(e);
    }

    games
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = app_dir();
    let secrets = DpapiSecretStore::new(dir.join("secrets.dat"));
    let store = ConfigStore::load_or_default(dir.join("config.toml"), secrets)?;

    if store.groups().is_empty() {
        eprintln!(
            "salvae-agent: no groups configured (see crates/salvae-discord/INTEGRATION.md and \
             create or join a group first). Nothing to do."
        );
        return Ok(());
    }

    let device_id = store.device_id().to_string();
    let backups_dir = dir.join("backups");

    let mut groups = Vec::new();
    for group in store.groups() {
        let secret = store.group_secret(&group.id)?;
        let channel = DiscordChannel::new(secret.token, group.channel_id);
        let state_path = dir.join("state").join(format!("{}.json", group.id));
        let state = SyncState::load(&state_path)?;
        let engine = SyncEngine::new(
            channel,
            secret.key,
            device_id.clone(),
            device_id.clone(),
            group.max_versions,
            backups_dir.clone(),
        )
        .with_state(state);
        groups.push(GroupRuntime::new(group.clone(), engine, state_path));
    }

    let games = enumerate_games();
    eprintln!(
        "salvae-agent: watching {} group(s), {} installed game(s).",
        store.groups().len(),
        games.len()
    );

    let watcher = Watcher::new(SystemProcessLister);
    let detector = Detector::new(games);
    let mut agent = Agent::new(watcher, detector, groups);

    loop {
        match agent.tick(now_ms()) {
            Ok(results) => {
                for (event, outcome) in results {
                    eprintln!("salvae-agent: {event:?} -> {outcome:?}");
                }
            }
            Err(e) => eprintln!("salvae-agent: tick error: {e}"),
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}
