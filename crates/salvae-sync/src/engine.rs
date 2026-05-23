//! The sync engine: pull/push/resolve and presence over a Channel.

use std::path::{Path, PathBuf};

use salvae_core::hash::content_hash;
use salvae_core::version::SaveVersion;
use salvae_vault::channel::Channel;
use salvae_vault::vault::Vault;

use crate::marker::{PlayingRecord, PLAYING_MARKER};
use crate::pack;
use crate::state::SyncState;
use crate::SyncError;

/// How long a "currently playing" marker stays active (10 minutes).
pub const PLAYING_TTL_MS: u64 = 10 * 60 * 1000;

const SCAN_PAGE: u16 = 100;

/// Result of [`SyncEngine::pull`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// The game has no save in the vault yet.
    NoRemoteSave,
    /// Local already has the latest version (number).
    AlreadyUpToDate(u64),
    /// Downloaded and applied this version.
    Applied(SaveVersion),
}

/// Result of [`SyncEngine::push`] / [`SyncEngine::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    /// Local content matches the latest version (number); nothing uploaded.
    NoChange(u64),
    /// Uploaded a new version.
    Pushed(SaveVersion),
    /// A newer version exists in the vault than what we last synced, and our
    /// content differs — the caller must resolve before overwriting.
    Conflict { remote: SaveVersion },
}

/// How to resolve a [`PushOutcome::Conflict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Upload local content as a new version (the remote stays in history).
    PushLocal,
    /// Discard local content and take the remote version.
    TakeRemote,
}

/// Drives save sync for one group over a `Channel`.
pub struct SyncEngine<C: Channel> {
    channel: C,
    key: [u8; 32],
    member: String,
    device_id: String,
    max_versions: usize,
    backups_dir: PathBuf,
    state: SyncState,
}

impl<C: Channel> SyncEngine<C> {
    /// Create an engine for one group.
    pub fn new(
        channel: C,
        key: [u8; 32],
        member: impl Into<String>,
        device_id: impl Into<String>,
        max_versions: usize,
        backups_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            channel,
            key,
            member: member.into(),
            device_id: device_id.into(),
            max_versions,
            backups_dir: backups_dir.into(),
            state: SyncState::default(),
        }
    }

    /// Replace the sync state (e.g., loaded from disk).
    pub fn with_state(mut self, state: SyncState) -> Self {
        self.state = state;
        self
    }

    /// Borrow the current sync state (persist it after operations).
    pub fn state(&self) -> &SyncState {
        &self.state
    }

    fn vault(&self) -> Vault<&C> {
        Vault::new(&self.channel, self.key)
    }

    /// Back up the current `save_folder` (as a packed blob) before overwriting.
    fn backup(&self, game_id: &str, save_folder: &Path, now_ms: u64) -> Result<(), SyncError> {
        if !save_folder.exists() {
            return Ok(());
        }
        let packed = pack::pack_folder(save_folder)?;
        let dir = self.backups_dir.join(game_id);
        std::fs::create_dir_all(&dir).map_err(|e| SyncError::Io(e.to_string()))?;
        std::fs::write(dir.join(format!("{now_ms}.svpk")), &packed)
            .map_err(|e| SyncError::Io(e.to_string()))
    }

    /// Download `version` and write it over `save_folder` (backing up first).
    fn apply_version(
        &mut self,
        game_id: &str,
        save_folder: &Path,
        version: &SaveVersion,
        now_ms: u64,
    ) -> Result<(), SyncError> {
        let packed = self.vault().download(game_id, version.number)?;
        self.backup(game_id, save_folder, now_ms)?;
        clear_folder(save_folder)?;
        pack::unpack_folder(&packed, save_folder)?;
        self.state.set(game_id, version.number);
        Ok(())
    }

    /// Pull the latest version of `game_id` into `save_folder`.
    pub fn pull(
        &mut self,
        game_id: &str,
        save_folder: &Path,
        now_ms: u64,
    ) -> Result<PullOutcome, SyncError> {
        let Some(latest) = self.vault().latest_version(game_id)? else {
            return Ok(PullOutcome::NoRemoteSave);
        };
        if self.state.get(game_id) == Some(latest.number) {
            return Ok(PullOutcome::AlreadyUpToDate(latest.number));
        }
        self.apply_version(game_id, save_folder, &latest, now_ms)?;
        Ok(PullOutcome::Applied(latest))
    }
}

/// Remove all entries inside `path` (but keep `path` itself).
fn clear_folder(path: &Path) -> Result<(), SyncError> {
    if !path.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path).map_err(|e| SyncError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| SyncError::Io(e.to_string()))?;
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p).map_err(|e| SyncError::Io(e.to_string()))?;
        } else {
            std::fs::remove_file(&p).map_err(|e| SyncError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvae_vault::memory::InMemoryChannel;
    use salvae_vault::vault::Vault;

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn pull_no_remote_save() {
        let backups = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let mut engine = SyncEngine::new(
            InMemoryChannel::new(),
            [1u8; 32],
            "me",
            "dev-1",
            5,
            backups.path(),
        );
        assert_eq!(engine.pull("valheim", folder.path(), 1).unwrap(), PullOutcome::NoRemoteSave);
    }

    #[test]
    fn pull_downloads_and_writes_latest() {
        let channel = InMemoryChannel::new();
        let backups = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();

        // Seed the vault with a packed save (as the engine would push it).
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "world.db", b"day 3");
        let packed = pack::pack_folder(src.path()).unwrap();
        Vault::new(&channel, [1u8; 32])
            .push_version("valheim", &packed, "owner", "dev-owner", 100, 5)
            .unwrap();

        let mut engine =
            SyncEngine::new(&channel, [1u8; 32], "me", "dev-1", 5, backups.path());
        let outcome = engine.pull("valheim", folder.path(), 200).unwrap();
        assert!(matches!(outcome, PullOutcome::Applied(v) if v.number == 1));
        assert_eq!(std::fs::read(folder.path().join("world.db")).unwrap(), b"day 3");

        // Pulling again is a no-op (already up to date).
        assert_eq!(
            engine.pull("valheim", folder.path(), 300).unwrap(),
            PullOutcome::AlreadyUpToDate(1)
        );
    }
}
