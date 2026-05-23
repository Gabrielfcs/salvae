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
        let dir = self.backups_dir.join(sanitize_component(game_id));
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

    /// Push `save_folder` as a new version of `game_id`. Returns a conflict if
    /// a newer version exists in the vault than what we last synced and our
    /// content differs (the caller must [`resolve`](Self::resolve)).
    pub fn push(
        &mut self,
        game_id: &str,
        save_folder: &Path,
        now_ms: u64,
    ) -> Result<PushOutcome, SyncError> {
        let packed = pack::pack_folder(save_folder)?;
        let local_hash = content_hash(&packed);
        let latest = self.vault().latest_version(game_id)?;

        match latest {
            None => self.do_push(game_id, &packed, now_ms),
            Some(latest) if latest.content_hash == local_hash => {
                self.state.set(game_id, latest.number);
                Ok(PushOutcome::NoChange(latest.number))
            }
            Some(latest) if self.state.get(game_id) == Some(latest.number) => {
                self.do_push(game_id, &packed, now_ms)
            }
            Some(latest) => Ok(PushOutcome::Conflict { remote: latest }),
        }
    }

    /// Upload `packed` as a new version and record it in the sync state.
    fn do_push(
        &mut self,
        game_id: &str,
        packed: &[u8],
        now_ms: u64,
    ) -> Result<PushOutcome, SyncError> {
        let version = self.vault().push_version(
            game_id,
            packed,
            &self.member,
            &self.device_id,
            now_ms,
            self.max_versions,
        )?;
        self.state.set(game_id, version.number);
        Ok(PushOutcome::Pushed(version))
    }

    /// Resolve a conflict for `game_id`: either push local content as a new
    /// version, or discard local and take the latest remote version.
    pub fn resolve(
        &mut self,
        game_id: &str,
        save_folder: &Path,
        resolution: Resolution,
        now_ms: u64,
    ) -> Result<PushOutcome, SyncError> {
        match resolution {
            Resolution::PushLocal => {
                let packed = pack::pack_folder(save_folder)?;
                // If local already equals the latest remote, nothing to upload.
                if let Some(latest) = self.vault().latest_version(game_id)? {
                    if latest.content_hash == content_hash(&packed) {
                        self.state.set(game_id, latest.number);
                        return Ok(PushOutcome::NoChange(latest.number));
                    }
                }
                self.do_push(game_id, &packed, now_ms)
            }
            Resolution::TakeRemote => {
                let Some(latest) = self.vault().latest_version(game_id)? else {
                    return Err(SyncError::Vault(salvae_vault::VaultError::NotFound));
                };
                self.apply_version(game_id, save_folder, &latest, now_ms)?;
                Ok(PushOutcome::NoChange(latest.number))
            }
        }
    }

    /// List all stored versions of `game_id`, oldest first.
    pub fn list_versions(
        &self,
        game_id: &str,
    ) -> Result<Vec<salvae_core::version::SaveVersion>, SyncError> {
        Ok(self.vault().list_versions(game_id)?)
    }

    /// Restore a specific `version` of `game_id` into `save_folder` (backing up
    /// the current local contents first). Returns the restored version's metadata.
    pub fn restore_version(
        &mut self,
        game_id: &str,
        version: u64,
        save_folder: &Path,
        now_ms: u64,
    ) -> Result<salvae_core::version::SaveVersion, SyncError> {
        let target = self
            .vault()
            .list_versions(game_id)?
            .into_iter()
            .find(|v| v.number == version)
            .ok_or(salvae_vault::VaultError::NotFound)?;
        self.apply_version(game_id, save_folder, &target, now_ms)?;
        Ok(target)
    }

    /// Post a "currently playing" marker for `game_id` (expires after the TTL).
    pub fn begin_playing(&self, game_id: &str, now_ms: u64) -> Result<(), SyncError> {
        let record = PlayingRecord {
            marker: PLAYING_MARKER.to_string(),
            game_id: game_id.to_string(),
            member: self.member.clone(),
            device_id: self.device_id.clone(),
            expires_at_ms: now_ms + PLAYING_TTL_MS,
        };
        self.channel.send_message(&record.to_content(), &[])?;
        Ok(())
    }

    /// List active "playing" markers for `game_id` posted by OTHER devices.
    pub fn who_is_playing(
        &self,
        game_id: &str,
        now_ms: u64,
    ) -> Result<Vec<PlayingRecord>, SyncError> {
        let mut out = Vec::new();
        let mut before = None;
        loop {
            let page = self.channel.list_messages(before, SCAN_PAGE)?;
            if page.is_empty() {
                break;
            }
            before = page.last().map(|m| m.id);
            let full = page.len() == SCAN_PAGE as usize;
            for msg in page {
                if let Some(rec) = PlayingRecord::parse(&msg.content) {
                    if rec.game_id == game_id
                        && rec.device_id != self.device_id
                        && rec.is_active(now_ms)
                    {
                        out.push(rec);
                    }
                }
            }
            if !full {
                break;
            }
        }
        Ok(out)
    }

    /// Remove this device's "playing" markers for `game_id`.
    pub fn end_playing(&self, game_id: &str) -> Result<(), SyncError> {
        let mut before = None;
        loop {
            let page = self.channel.list_messages(before, SCAN_PAGE)?;
            if page.is_empty() {
                break;
            }
            before = page.last().map(|m| m.id);
            let full = page.len() == SCAN_PAGE as usize;
            let mut to_delete = Vec::new();
            for msg in &page {
                if let Some(rec) = PlayingRecord::parse(&msg.content) {
                    if rec.game_id == game_id && rec.device_id == self.device_id {
                        to_delete.push(msg.id);
                    }
                }
            }
            for id in to_delete {
                self.channel.delete_message(id)?;
            }
            if !full {
                break;
            }
        }
        Ok(())
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

/// Make a game id safe to use as a single filesystem path component. Real ids
/// like `steam:892970` contain `:`, which is illegal in Windows path names, so
/// any character that is not alphanumeric, `-`, `_`, or `.` becomes `_`.
fn sanitize_component(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
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
        assert_eq!(
            engine.pull("valheim", folder.path(), 1).unwrap(),
            PullOutcome::NoRemoteSave
        );
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

        let mut engine = SyncEngine::new(&channel, [1u8; 32], "me", "dev-1", 5, backups.path());
        let outcome = engine.pull("valheim", folder.path(), 200).unwrap();
        assert!(matches!(outcome, PullOutcome::Applied(v) if v.number == 1));
        assert_eq!(
            std::fs::read(folder.path().join("world.db")).unwrap(),
            b"day 3"
        );

        // Pulling again is a no-op (already up to date).
        assert_eq!(
            engine.pull("valheim", folder.path(), 300).unwrap(),
            PullOutcome::AlreadyUpToDate(1)
        );
    }

    #[test]
    fn pull_over_existing_folder_backs_up_with_colon_game_id() {
        // Real ids contain `:` (`steam:892970`), which is illegal in Windows
        // path names. Backing up an existing folder must still succeed.
        let channel = InMemoryChannel::new();
        let backups = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();

        let src = tempfile::tempdir().unwrap();
        write(src.path(), "world.db", b"remote");
        let packed = pack::pack_folder(src.path()).unwrap();
        Vault::new(&channel, [1u8; 32])
            .push_version("steam:892970", &packed, "owner", "dev-owner", 100, 5)
            .unwrap();

        // The local folder already has a (diverged) save to be backed up.
        write(folder.path(), "world.db", b"local");

        let mut engine = SyncEngine::new(&channel, [1u8; 32], "me", "dev-1", 5, backups.path());
        let outcome = engine.pull("steam:892970", folder.path(), 200).unwrap();
        assert!(matches!(outcome, PullOutcome::Applied(v) if v.number == 1));
        assert_eq!(
            std::fs::read(folder.path().join("world.db")).unwrap(),
            b"remote"
        );

        // A backup was written under a sanitized (colon-free) directory.
        let backup_dir = backups.path().join("steam_892970");
        assert!(backup_dir.is_dir());
        assert_eq!(std::fs::read_dir(&backup_dir).unwrap().count(), 1);
    }

    #[test]
    fn push_creates_first_version_then_detects_conflict() {
        let channel = InMemoryChannel::new();
        let backups = tempfile::tempdir().unwrap();

        // Owner pushes v1.
        let owner_folder = tempfile::tempdir().unwrap();
        write(owner_folder.path(), "world.db", b"owner day1");
        let mut owner =
            SyncEngine::new(&channel, [2u8; 32], "owner", "dev-owner", 5, backups.path());
        assert!(matches!(
            owner.push("valheim", owner_folder.path(), 100).unwrap(),
            PushOutcome::Pushed(v) if v.number == 1
        ));

        // Pushing the same content again is a no-op.
        assert_eq!(
            owner.push("valheim", owner_folder.path(), 110).unwrap(),
            PushOutcome::NoChange(1)
        );

        // Friend pulls v1, edits, pushes v2.
        let friend_backups = tempfile::tempdir().unwrap();
        let friend_folder = tempfile::tempdir().unwrap();
        let mut friend = SyncEngine::new(
            &channel,
            [2u8; 32],
            "friend",
            "dev-friend",
            5,
            friend_backups.path(),
        );
        friend.pull("valheim", friend_folder.path(), 200).unwrap();
        write(friend_folder.path(), "world.db", b"friend day2");
        assert!(matches!(
            friend.push("valheim", friend_folder.path(), 210).unwrap(),
            PushOutcome::Pushed(v) if v.number == 2
        ));

        // Owner (still on v1 locally) edits and pushes -> CONFLICT (v2 exists).
        write(owner_folder.path(), "world.db", b"owner day2 diverged");
        assert!(matches!(
            owner.push("valheim", owner_folder.path(), 220).unwrap(),
            PushOutcome::Conflict { remote } if remote.number == 2
        ));
    }

    fn seed_conflict() -> (InMemoryChannel, tempfile::TempDir) {
        // Vault has v1 ("owner day1") and v2 ("friend day2").
        let channel = InMemoryChannel::new();
        let tmp = tempfile::tempdir().unwrap();
        let s1 = tmp.path().join("s1");
        let s2 = tmp.path().join("s2");
        write(&s1, "world.db", b"owner day1");
        write(&s2, "world.db", b"friend day2");
        let v = Vault::new(&channel, [3u8; 32]);
        v.push_version("valheim", &pack::pack_folder(&s1).unwrap(), "o", "do", 1, 5)
            .unwrap();
        v.push_version("valheim", &pack::pack_folder(&s2).unwrap(), "f", "df", 2, 5)
            .unwrap();
        (channel, tmp)
    }

    #[test]
    fn resolve_push_local_uploads_new_version() {
        let (channel, _tmp) = seed_conflict();
        let backups = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), "world.db", b"owner day2 diverged");

        let mut owner =
            SyncEngine::new(&channel, [3u8; 32], "owner", "dev-owner", 5, backups.path());
        // owner is behind (never synced) -> push would conflict; resolve PushLocal.
        let out = owner
            .resolve("valheim", folder.path(), Resolution::PushLocal, 300)
            .unwrap();
        assert!(matches!(out, PushOutcome::Pushed(v) if v.number == 3));
        // Local content unchanged by PushLocal.
        assert_eq!(
            std::fs::read(folder.path().join("world.db")).unwrap(),
            b"owner day2 diverged"
        );
    }

    #[test]
    fn resolve_take_remote_overwrites_local() {
        let (channel, _tmp) = seed_conflict();
        let backups = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), "world.db", b"owner day2 diverged");

        let mut owner =
            SyncEngine::new(&channel, [3u8; 32], "owner", "dev-owner", 5, backups.path());
        let out = owner
            .resolve("valheim", folder.path(), Resolution::TakeRemote, 300)
            .unwrap();
        assert!(matches!(out, PushOutcome::NoChange(2)));
        // Local now holds the remote v2 content; the old local was backed up.
        assert_eq!(
            std::fs::read(folder.path().join("world.db")).unwrap(),
            b"friend day2"
        );
        assert!(
            std::fs::read_dir(backups.path().join("valheim"))
                .unwrap()
                .count()
                >= 1
        );
    }

    #[test]
    fn marker_lifecycle_across_two_members() {
        let channel = InMemoryChannel::new();
        let b1 = tempfile::tempdir().unwrap();
        let b2 = tempfile::tempdir().unwrap();
        let friend = SyncEngine::new(&channel, [4u8; 32], "friend", "dev-friend", 5, b1.path());
        let me = SyncEngine::new(&channel, [4u8; 32], "me", "dev-me", 5, b2.path());

        // Friend starts playing at t=1000 (marker active until 1000 + TTL).
        friend.begin_playing("valheim", 1000).unwrap();

        // I see the friend playing (and not myself).
        let playing = me.who_is_playing("valheim", 1000).unwrap();
        assert_eq!(playing.len(), 1);
        assert_eq!(playing[0].member, "friend");

        // After the TTL, the marker is no longer active.
        assert!(me
            .who_is_playing("valheim", 1000 + PLAYING_TTL_MS)
            .unwrap()
            .is_empty());

        // Friend stops; their marker is removed.
        friend.end_playing("valheim").unwrap();
        assert!(me.who_is_playing("valheim", 1000).unwrap().is_empty());
    }

    #[test]
    fn who_is_playing_excludes_self() {
        let channel = InMemoryChannel::new();
        let b = tempfile::tempdir().unwrap();
        let me = SyncEngine::new(&channel, [4u8; 32], "me", "dev-me", 5, b.path());
        me.begin_playing("valheim", 1000).unwrap();
        assert!(me.who_is_playing("valheim", 1000).unwrap().is_empty());
    }

    fn engine_over<'a>(
        channel: &'a InMemoryChannel,
        backups: &std::path::Path,
    ) -> SyncEngine<&'a InMemoryChannel> {
        SyncEngine::new(
            channel,
            [5u8; 32],
            "tester",
            "dev",
            5,
            backups.to_path_buf(),
        )
    }

    fn seed(channel: &InMemoryChannel, game_id: &str, content: &[u8]) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("world.db"), content).unwrap();
        let packed = crate::pack::pack_folder(dir.path()).unwrap();
        Vault::new(channel, [5u8; 32])
            .push_version(game_id, &packed, "seed", "seed-dev", 1, 5)
            .unwrap();
    }

    #[test]
    fn list_versions_returns_history_in_order() {
        let channel = InMemoryChannel::new();
        seed(&channel, "game", b"v1 content");
        seed(&channel, "game", b"v2 content");
        let backups = tempfile::tempdir().unwrap();
        let engine = engine_over(&channel, backups.path());
        assert_eq!(
            engine
                .list_versions("game")
                .unwrap()
                .iter()
                .map(|v| v.number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn restore_version_writes_that_version_to_the_folder() {
        let channel = InMemoryChannel::new();
        seed(&channel, "game", b"day one");
        seed(&channel, "game", b"day two");
        let backups = tempfile::tempdir().unwrap();
        let mut engine = engine_over(&channel, backups.path());

        let folder = tempfile::tempdir().unwrap();
        let restored = engine
            .restore_version("game", 1, folder.path(), 100)
            .unwrap();
        assert_eq!(restored.number, 1);
        assert_eq!(
            std::fs::read(folder.path().join("world.db")).unwrap(),
            b"day one"
        );
    }

    #[test]
    fn restore_missing_version_is_not_found() {
        let channel = InMemoryChannel::new();
        seed(&channel, "game", b"only one");
        let backups = tempfile::tempdir().unwrap();
        let mut engine = engine_over(&channel, backups.path());
        let folder = tempfile::tempdir().unwrap();
        assert!(matches!(
            engine.restore_version("game", 99, folder.path(), 100),
            Err(SyncError::Vault(salvae_vault::VaultError::NotFound))
        ));
    }
}
