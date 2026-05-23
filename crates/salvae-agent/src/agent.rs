//! Agent orchestration: resolve a game to its group, drive sync on open/close.

use std::path::PathBuf;

use salvae_config::group::GroupConfig;
use salvae_sync::engine::SyncEngine;
use salvae_vault::channel::Channel;
use salvae_watch::detector::{Detector, GameEvent};
use salvae_watch::process::{ProcessLister, Watcher};

use crate::outcome::AgentOutcome;
use crate::AgentError;

/// One group's live sync state: its config, engine, and where to persist
/// the engine's [`SyncState`](salvae_sync::state::SyncState).
pub struct GroupRuntime<C: Channel> {
    config: GroupConfig,
    engine: SyncEngine<C>,
    state_path: PathBuf,
}

impl<C: Channel> GroupRuntime<C> {
    pub fn new(config: GroupConfig, engine: SyncEngine<C>, state_path: impl Into<PathBuf>) -> Self {
        Self { config, engine, state_path: state_path.into() }
    }
}

/// Drives save sync from game open/close events across all configured groups.
pub struct Agent<C: Channel, L: ProcessLister> {
    watcher: Watcher<L>,
    detector: Detector,
    groups: Vec<GroupRuntime<C>>,
}

impl<C: Channel, L: ProcessLister> Agent<C, L> {
    pub fn new(watcher: Watcher<L>, detector: Detector, groups: Vec<GroupRuntime<C>>) -> Self {
        Self { watcher, detector, groups }
    }

    /// Find the group + configured save folder for `game_id`, if any.
    fn resolve(&mut self, game_id: &str) -> Option<(&mut GroupRuntime<C>, PathBuf)> {
        for rt in &mut self.groups {
            // Clone the path to an owned value so the immutable borrow of `rt`
            // ends before we hand back the `&mut rt`.
            if let Some(folder) = rt.config.game_paths.get(game_id).map(PathBuf::from) {
                return Some((rt, folder));
            }
        }
        None
    }

    /// Handle a game opening: pull the latest save into its folder and post a
    /// "currently playing" marker. Returns who else is already playing.
    pub fn handle_open(&mut self, game_id: &str, now_ms: u64) -> Result<AgentOutcome, AgentError> {
        let Some((rt, folder)) = self.resolve(game_id) else {
            return Ok(AgentOutcome::NotConfigured);
        };
        let others_playing = rt
            .engine
            .who_is_playing(game_id, now_ms)?
            .into_iter()
            .map(|r| r.member)
            .collect();
        let pull = rt.engine.pull(game_id, &folder, now_ms)?;
        rt.engine.begin_playing(game_id, now_ms)?;
        save_state(rt)?;
        Ok(AgentOutcome::Opened { pull, others_playing })
    }

    /// Handle a game closing: remove our "playing" marker and push the local
    /// save (which may surface a conflict for the UI to resolve).
    pub fn handle_close(&mut self, game_id: &str, now_ms: u64) -> Result<AgentOutcome, AgentError> {
        let Some((rt, folder)) = self.resolve(game_id) else {
            return Ok(AgentOutcome::NotConfigured);
        };
        rt.engine.end_playing(game_id)?;
        if !folder.exists() {
            return Ok(AgentOutcome::NoFolder);
        }
        let push = rt.engine.push(game_id, &folder, now_ms)?;
        save_state(rt)?;
        Ok(AgentOutcome::Closed { push })
    }

    /// Poll processes once, turn detected game open/close events into sync
    /// actions, and return each event with its outcome.
    pub fn tick(&mut self, now_ms: u64) -> Result<Vec<(GameEvent, AgentOutcome)>, AgentError> {
        let process_events = self.watcher.poll()?;
        let game_events = self.detector.process(&process_events);
        let mut results = Vec::new();
        for event in game_events {
            let outcome = match &event {
                GameEvent::Opened { game_id } => self.handle_open(game_id, now_ms)?,
                GameEvent::Closed { game_id } => self.handle_close(game_id, now_ms)?,
            };
            results.push((event, outcome));
        }
        Ok(results)
    }
}

/// Persist a group's current sync state to its state file.
fn save_state<C: Channel>(rt: &GroupRuntime<C>) -> Result<(), AgentError> {
    rt.engine.state().save(&rt.state_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvae_detect::game::{InstalledGame, Launcher};
    use salvae_vault::memory::InMemoryChannel;
    use salvae_vault::vault::Vault;
    use std::collections::BTreeMap;
    use std::path::Path;

    /// A fake lister returning a scripted sequence of process listings.
    pub(super) struct FakeLister {
        pub frames:
            std::cell::RefCell<std::collections::VecDeque<Vec<salvae_watch::process::ProcessInfo>>>,
    }
    impl ProcessLister for FakeLister {
        fn list(
            &self,
        ) -> Result<Vec<salvae_watch::process::ProcessInfo>, salvae_watch::WatchError> {
            Ok(self.frames.borrow_mut().pop_front().unwrap_or_default())
        }
    }

    pub(super) fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub(super) fn installed() -> InstalledGame {
        InstalledGame {
            id: "steam:1".into(),
            name: "Valheim".into(),
            launcher: Launcher::Steam,
            install_dir: PathBuf::from("C:/Steam/common/Valheim"),
        }
    }

    /// Build a one-group agent whose `steam:1` save folder is `folder`, over the
    /// given (already-seeded) in-memory channel.
    pub(super) fn agent_for(
        channel: InMemoryChannel,
        folder: &Path,
        state_path: PathBuf,
        backups: PathBuf,
        frames: Vec<Vec<salvae_watch::process::ProcessInfo>>,
    ) -> Agent<InMemoryChannel, FakeLister> {
        let mut game_paths = BTreeMap::new();
        game_paths.insert("steam:1".to_string(), folder.to_string_lossy().to_string());
        let config = GroupConfig {
            id: "g1".into(),
            name: "Crew".into(),
            guild_id: 1,
            channel_id: 2,
            salt: "00".into(),
            max_versions: 5,
            game_paths,
        };
        let engine = SyncEngine::new(channel, [9u8; 32], "me", "dev-me", 5, backups);
        let rt = GroupRuntime::new(config, engine, state_path);
        let watcher = Watcher::new(FakeLister { frames: std::cell::RefCell::new(frames.into()) });
        Agent::new(watcher, Detector::new(vec![installed()]), vec![rt])
    }

    #[test]
    fn resolve_finds_configured_game_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = agent_for(
            InMemoryChannel::new(),
            &dir.path().join("save"),
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        let (_, folder) = agent.resolve("steam:1").expect("configured");
        assert!(folder.ends_with("save"));
        assert!(agent.resolve("steam:999").is_none());
    }

    /// Seed helper used by later tasks: push a save into a channel as version 1.
    pub(super) fn seed_remote(channel: &InMemoryChannel, bytes: &[u8]) {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "world.db", bytes);
        let packed = salvae_sync::pack::pack_folder(dir.path()).unwrap();
        Vault::new(channel, [9u8; 32])
            .push_version("steam:1", &packed, "seed", "seed-dev", 1, 5)
            .unwrap();
    }

    #[test]
    fn open_pulls_latest_into_the_save_folder() {
        use salvae_sync::engine::PullOutcome;

        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");

        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"remote day 1");

        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );

        let outcome = agent.handle_open("steam:1", 100).unwrap();
        assert!(matches!(outcome, AgentOutcome::Opened { pull: PullOutcome::Applied(v), .. } if v.number == 1));
        assert_eq!(std::fs::read(folder.join("world.db")).unwrap(), b"remote day 1");
        // State was persisted.
        assert!(dir.path().join("state.json").exists());
    }

    #[test]
    fn open_unconfigured_game_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = agent_for(
            InMemoryChannel::new(),
            &dir.path().join("save"),
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        assert_eq!(agent.handle_open("steam:999", 100).unwrap(), AgentOutcome::NotConfigured);
    }

    #[test]
    fn close_pushes_the_local_save() {
        use salvae_sync::engine::PushOutcome;

        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");
        write(&folder, "world.db", b"my progress");

        let mut agent = agent_for(
            InMemoryChannel::new(),
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );

        let outcome = agent.handle_close("steam:1", 200).unwrap();
        assert!(matches!(outcome, AgentOutcome::Closed { push: PushOutcome::Pushed(v) } if v.number == 1));
        assert!(dir.path().join("state.json").exists());
    }

    #[test]
    fn close_with_missing_folder_is_no_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = agent_for(
            InMemoryChannel::new(),
            &dir.path().join("save"), // never created
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        assert_eq!(agent.handle_close("steam:1", 200).unwrap(), AgentOutcome::NoFolder);
    }

    #[test]
    fn close_surfaces_conflict_without_overwriting() {
        use salvae_sync::engine::PushOutcome;

        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");
        write(&folder, "world.db", b"my diverged progress");

        // Remote already has v1 (from someone else); we never pulled it, and our
        // content differs -> push must report a conflict.
        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"someone elses progress");

        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        let outcome = agent.handle_close("steam:1", 200).unwrap();
        assert!(matches!(outcome, AgentOutcome::Closed { push: PushOutcome::Conflict { remote } } if remote.number == 1));
    }

    #[test]
    fn tick_pulls_when_a_configured_game_launches() {
        use salvae_sync::engine::PullOutcome;
        use salvae_watch::detector::GameEvent;
        use salvae_watch::process::ProcessInfo;

        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");

        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"remote save");

        // Frame 1: no game. Frame 2: the game's exe (under its install dir) runs.
        let frames = vec![
            vec![ProcessInfo { pid: 1, exe_path: "C:/Windows/explorer.exe".into() }],
            vec![
                ProcessInfo { pid: 1, exe_path: "C:/Windows/explorer.exe".into() },
                ProcessInfo { pid: 9, exe_path: "C:/Steam/common/Valheim/valheim.exe".into() },
            ],
        ];
        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            frames,
        );

        // Tick 1: explorer only -> no game events.
        assert!(agent.tick(100).unwrap().is_empty());

        // Tick 2: Valheim launches -> GameOpened -> pull applied.
        let results = agent.tick(200).unwrap();
        assert_eq!(results.len(), 1);
        let (event, outcome) = &results[0];
        assert_eq!(event, &GameEvent::Opened { game_id: "steam:1".into() });
        assert!(matches!(outcome, AgentOutcome::Opened { pull: PullOutcome::Applied(_), .. }));
        assert_eq!(std::fs::read(folder.join("world.db")).unwrap(), b"remote save");
    }
}
