//! Agent orchestration: resolve a game to its group, drive sync on open/close.

use std::path::PathBuf;

use salvae_config::group::GroupConfig;
use salvae_core::version::SaveVersion;
use salvae_sync::engine::{PullOutcome, PushOutcome, Resolution, SyncEngine};
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
        Self {
            config,
            engine,
            state_path: state_path.into(),
        }
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
        Self {
            watcher,
            detector,
            groups,
        }
    }

    /// Replace the per-group runtimes (e.g. after a config change), keeping the
    /// watcher and detector — and thus their live process/open-game state —
    /// intact. Rebuilding the whole agent instead would reset that state and
    /// can drop a real close-push or fire a spurious open-pull over a running
    /// game.
    pub fn set_groups(&mut self, groups: Vec<GroupRuntime<C>>) {
        self.groups = groups;
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
        Ok(AgentOutcome::Opened {
            pull,
            others_playing,
        })
    }

    /// Handle a game closing: remove our "playing" marker and push the local
    /// save (which may surface a conflict for the UI to resolve).
    pub fn handle_close(&mut self, game_id: &str, now_ms: u64) -> Result<AgentOutcome, AgentError> {
        let Some((rt, folder)) = self.resolve(game_id) else {
            return Ok(AgentOutcome::NotConfigured);
        };
        // Stop advertising that we're playing as soon as we close, even if the
        // push below fails (the marker is advisory and self-heals on next open).
        rt.engine.end_playing(game_id)?;
        if !folder.exists() {
            return Ok(AgentOutcome::NoFolder);
        }
        let push = rt.engine.push(game_id, &folder, now_ms)?;
        // A conflict didn't advance our synced version, so only persist when the
        // push actually changed state.
        if !matches!(push, PushOutcome::Conflict { .. }) {
            save_state(rt)?;
        }
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

    /// Poll the channel for newer saves of every configured game and pull the
    /// ones that advanced — so a member receives a teammate's save without
    /// having to launch the game. Games currently running are skipped (a live
    /// pull would clobber the save the player is actively writing). Returns the
    /// `(game_id, outcome)` for each game whose save was actually applied.
    pub fn poll_remote(&mut self, now_ms: u64) -> Result<Vec<(String, PullOutcome)>, AgentError> {
        let mut applied = Vec::new();
        for rt in &mut self.groups {
            // Snapshot (game_id, folder) so the immutable borrow of the config
            // ends before we pull through the engine (mutable borrow of `rt`).
            let configured: Vec<(String, PathBuf)> = rt
                .config
                .game_paths
                .iter()
                .map(|(id, path)| (id.clone(), PathBuf::from(path)))
                .collect();
            for (game_id, folder) in configured {
                if self.detector.is_open(&game_id) {
                    continue;
                }
                let outcome = rt.engine.pull(&game_id, &folder, now_ms)?;
                if matches!(outcome, PullOutcome::Applied(_)) {
                    save_state(rt)?;
                    applied.push((game_id, outcome));
                }
            }
        }
        Ok(applied)
    }

    /// Resolve a pending conflict for `game_id` with the user's choice.
    pub fn handle_resolve(
        &mut self,
        game_id: &str,
        resolution: Resolution,
        now_ms: u64,
    ) -> Result<AgentOutcome, AgentError> {
        let Some((rt, folder)) = self.resolve(game_id) else {
            return Ok(AgentOutcome::NotConfigured);
        };
        let push = rt.engine.resolve(game_id, &folder, resolution, now_ms)?;
        save_state(rt)?;
        Ok(AgentOutcome::Closed { push })
    }

    /// List the stored versions of `game_id` (empty if it is not configured).
    pub fn history(&mut self, game_id: &str) -> Result<Vec<SaveVersion>, AgentError> {
        let Some((rt, _folder)) = self.resolve(game_id) else {
            return Ok(Vec::new());
        };
        Ok(rt.engine.list_versions(game_id)?)
    }

    /// Restore a specific past `version` of `game_id` into its save folder.
    pub fn restore(
        &mut self,
        game_id: &str,
        version: u64,
        now_ms: u64,
    ) -> Result<AgentOutcome, AgentError> {
        let Some((rt, folder)) = self.resolve(game_id) else {
            return Ok(AgentOutcome::NotConfigured);
        };
        let restored = rt
            .engine
            .restore_version(game_id, version, &folder, now_ms)?;
        save_state(rt)?;
        Ok(AgentOutcome::Restored { version: restored })
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
        let watcher = Watcher::new(FakeLister {
            frames: std::cell::RefCell::new(frames.into()),
        });
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
        assert!(
            matches!(outcome, AgentOutcome::Opened { pull: PullOutcome::Applied(v), .. } if v.number == 1)
        );
        assert_eq!(
            std::fs::read(folder.join("world.db")).unwrap(),
            b"remote day 1"
        );
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
        assert_eq!(
            agent.handle_open("steam:999", 100).unwrap(),
            AgentOutcome::NotConfigured
        );
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
        assert!(
            matches!(outcome, AgentOutcome::Closed { push: PushOutcome::Pushed(v) } if v.number == 1)
        );
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
        assert_eq!(
            agent.handle_close("steam:1", 200).unwrap(),
            AgentOutcome::NoFolder
        );
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
        assert!(
            matches!(outcome, AgentOutcome::Closed { push: PushOutcome::Conflict { remote } } if remote.number == 1)
        );
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
            vec![ProcessInfo {
                pid: 1,
                exe_path: "C:/Windows/explorer.exe".into(),
            }],
            vec![
                ProcessInfo {
                    pid: 1,
                    exe_path: "C:/Windows/explorer.exe".into(),
                },
                ProcessInfo {
                    pid: 9,
                    exe_path: "C:/Steam/common/Valheim/valheim.exe".into(),
                },
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
        assert_eq!(
            event,
            &GameEvent::Opened {
                game_id: "steam:1".into()
            }
        );
        assert!(matches!(
            outcome,
            AgentOutcome::Opened {
                pull: PullOutcome::Applied(_),
                ..
            }
        ));
        assert_eq!(
            std::fs::read(folder.join("world.db")).unwrap(),
            b"remote save"
        );
    }

    #[test]
    fn poll_remote_pulls_a_teammates_save_without_opening_the_game() {
        use salvae_sync::engine::PullOutcome;

        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");

        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"teammate save");

        // No process frames -> the game is never detected as running.
        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );

        let pulled = agent.poll_remote(100).unwrap();
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0].0, "steam:1");
        assert!(matches!(pulled[0].1, PullOutcome::Applied(ref v) if v.number == 1));
        assert_eq!(
            std::fs::read(folder.join("world.db")).unwrap(),
            b"teammate save"
        );

        // Polling again is a no-op (already up to date), so nothing is reported.
        assert!(agent.poll_remote(200).unwrap().is_empty());
    }

    #[test]
    fn poll_remote_skips_a_currently_running_game() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");

        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"remote save");

        // One frame that launches the game so the detector marks it open.
        let frames = vec![vec![salvae_watch::process::ProcessInfo {
            pid: 9,
            exe_path: "C:/Steam/common/Valheim/valheim.exe".into(),
        }]];
        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            frames,
        );

        // Tick consumes the frame -> game is now open (and pulled on open).
        agent.tick(50).unwrap();
        // While it's open, the background poll must not touch its folder again.
        assert!(agent.poll_remote(100).unwrap().is_empty());
    }

    #[test]
    fn handle_resolve_take_remote_overwrites_local() {
        use salvae_sync::engine::{PushOutcome, Resolution};

        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");
        write(&folder, "world.db", b"my diverged");

        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"the remote save");

        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        let outcome = agent
            .handle_resolve("steam:1", Resolution::TakeRemote, 300)
            .unwrap();
        assert!(matches!(
            outcome,
            AgentOutcome::Closed {
                push: PushOutcome::NoChange(1)
            }
        ));
        assert_eq!(
            std::fs::read(folder.join("world.db")).unwrap(),
            b"the remote save"
        );
    }

    #[test]
    fn history_lists_versions_and_restore_writes_one() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");

        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"v1"); // version 1

        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );

        // Open first so the local save is based on v1, then a normal close
        // pushes a clean version 2 (not a stale-overwrite conflict).
        agent.handle_open("steam:1", 150).unwrap();
        write(&folder, "world.db", b"v2");
        agent.handle_close("steam:1", 200).unwrap();

        let versions = agent.history("steam:1").unwrap();
        assert_eq!(
            versions.iter().map(|v| v.number).collect::<Vec<_>>(),
            vec![1, 2]
        );

        // Restore version 1 over the folder.
        let outcome = agent.restore("steam:1", 1, 300).unwrap();
        assert!(matches!(outcome, AgentOutcome::Restored { version } if version.number == 1));
        assert_eq!(std::fs::read(folder.join("world.db")).unwrap(), b"v1");
    }

    #[test]
    fn history_for_unconfigured_game_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = agent_for(
            InMemoryChannel::new(),
            &dir.path().join("save"),
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        assert!(agent.history("steam:999").unwrap().is_empty());
        assert_eq!(
            agent.restore("steam:999", 1, 100).unwrap(),
            AgentOutcome::NotConfigured
        );
    }

    #[test]
    fn set_groups_swaps_the_configured_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("save");
        let channel = InMemoryChannel::new();
        seed_remote(&channel, b"v1");
        let mut agent = agent_for(
            channel,
            &folder,
            dir.path().join("state.json"),
            dir.path().join("backups"),
            vec![],
        );
        // The seeded group has steam:1 configured.
        assert_eq!(agent.history("steam:1").unwrap().len(), 1);
        // Replacing the runtimes with none drops that configuration.
        agent.set_groups(vec![]);
        assert!(agent.history("steam:1").unwrap().is_empty());
    }
}
