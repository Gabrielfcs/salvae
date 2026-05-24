//! The versioned-save store logic over a Channel.

use salvae_core::version::SaveVersion;
use salvae_core::{chunk, hash, seal};

use crate::channel::{Channel, Message};
use crate::record::{VersionRecord, MARKER};
use crate::VaultError;

/// Default maximum size of a single attachment chunk (8 MiB) — safely under
/// Discord's 10 MiB non-boosted limit, leaving headroom for request overhead.
pub const DEFAULT_MAX_CHUNK: usize = 8 * 1024 * 1024;

/// How many messages to fetch per pagination page when scanning the channel.
const SCAN_PAGE: u16 = 100;

/// An encrypted, versioned save store backed by a [`Channel`].
pub struct Vault<C: Channel> {
    channel: C,
    key: [u8; 32],
    max_chunk_size: usize,
}

impl<C: Channel> Vault<C> {
    /// Create a vault over `channel`, encrypting with `key` (from
    /// `salvae_core::kdf::derive_key`).
    pub fn new(channel: C, key: [u8; 32]) -> Self {
        Self {
            channel,
            key,
            max_chunk_size: DEFAULT_MAX_CHUNK,
        }
    }

    /// Override the max chunk size (e.g., for a boosted server or in tests).
    pub fn with_max_chunk_size(mut self, max_chunk_size: usize) -> Self {
        self.max_chunk_size = max_chunk_size;
        self
    }

    /// Borrow the underlying channel (test/inspection helper).
    pub fn channel(&self) -> &C {
        &self.channel
    }

    /// Scan the whole channel and return every (message, version) for `game_id`,
    /// unsorted.
    fn scan(&self, game_id: &str) -> Result<Vec<(Message, SaveVersion)>, VaultError> {
        let mut out = Vec::new();
        let mut before = None;
        loop {
            let page = self.channel.list_messages(before, SCAN_PAGE)?;
            if page.is_empty() {
                break;
            }
            before = page.last().map(|m| m.id);
            let full_page = page.len() == SCAN_PAGE as usize;
            for msg in page {
                if let Some(rec) = VersionRecord::decode(&msg.content, &self.key) {
                    if rec.game_id == game_id {
                        out.push((msg, rec.version));
                    }
                }
            }
            if !full_page {
                break;
            }
        }
        Ok(out)
    }

    /// The newest version of `game_id`, or `None` if the game has no saves yet.
    pub fn latest_version(&self, game_id: &str) -> Result<Option<SaveVersion>, VaultError> {
        let found = self.scan(game_id)?;
        Ok(found.into_iter().map(|(_, v)| v).max_by_key(|v| v.number))
    }

    /// All versions of `game_id`, sorted by version number ascending.
    pub fn list_versions(&self, game_id: &str) -> Result<Vec<SaveVersion>, VaultError> {
        let mut versions: Vec<SaveVersion> =
            self.scan(game_id)?.into_iter().map(|(_, v)| v).collect();
        versions.sort_by_key(|v| v.number);
        Ok(versions)
    }

    /// Seal `save`, store it as a new version of `game_id`, and prune to the
    /// last `max_versions`. If the latest stored version already has identical
    /// content, this is a no-op and the existing latest version is returned.
    #[allow(clippy::too_many_arguments)]
    pub fn push_version(
        &self,
        game_id: &str,
        game_name: &str,
        save: &[u8],
        author: &str,
        device_id: &str,
        now_ms: u64,
        max_versions: usize,
    ) -> Result<SaveVersion, VaultError> {
        let content_hash = hash::content_hash(save);
        let existing = self.scan(game_id)?;
        let latest = existing
            .iter()
            .map(|(_, v)| v)
            .max_by_key(|v| v.number)
            .cloned();

        if let Some(ref latest) = latest {
            if latest.content_hash == content_hash {
                return Ok(latest.clone());
            }
        }
        let next_number = latest.as_ref().map(|v| v.number + 1).unwrap_or(1);

        let blob = seal::seal(&self.key, save)?;
        let chunks = chunk::split(&blob, self.max_chunk_size)?;
        let attachments: Vec<(String, Vec<u8>)> = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| (format!("chunk_{i}.bin"), c.clone()))
            .collect();

        let version = SaveVersion {
            number: next_number,
            content_hash,
            created_at_ms: now_ms,
            author: author.to_string(),
            device_id: device_id.to_string(),
            size_bytes: save.len() as u64,
            chunk_count: chunks.len() as u32,
        };
        let record = VersionRecord {
            marker: MARKER.to_string(),
            game_id: game_id.to_string(),
            version: version.clone(),
        };
        self.channel
            .send_message(&record.encode(&self.key, game_name)?, &attachments)?;

        self.prune(game_id, max_versions)?;
        Ok(version)
    }

    /// Download, decrypt, decompress and integrity-check version `number` of
    /// `game_id`, returning the original save bytes.
    pub fn download(&self, game_id: &str, number: u64) -> Result<Vec<u8>, VaultError> {
        let (message, version) = self
            .scan(game_id)?
            .into_iter()
            .find(|(_, v)| v.number == number)
            .ok_or(VaultError::NotFound)?;

        // Collect chunk attachments in order: chunk_0.bin .. chunk_{n-1}.bin
        let mut chunks: Vec<Vec<u8>> = Vec::with_capacity(version.chunk_count as usize);
        for i in 0..version.chunk_count {
            let filename = format!("chunk_{i}.bin");
            let att = message
                .attachments
                .iter()
                .find(|a| a.filename == filename)
                .ok_or(VaultError::NotFound)?;
            chunks.push(self.channel.download_attachment(message.id, att)?);
        }

        let blob = chunk::join(&chunks);
        let save = seal::open(&self.key, &blob)?;
        if hash::content_hash(&save) != version.content_hash {
            return Err(VaultError::Integrity);
        }
        Ok(save)
    }

    /// Delete the oldest version messages of `game_id` until at most
    /// `max_versions` remain. `max_versions == 0` is treated as 1 (always keep
    /// at least the newest version, to never leave a game with no save).
    fn prune(&self, game_id: &str, max_versions: usize) -> Result<(), VaultError> {
        let keep = max_versions.max(1);
        let mut found = self.scan(game_id)?;
        if found.len() <= keep {
            return Ok(());
        }
        // Oldest first (lowest version number).
        found.sort_by_key(|(_, v)| v.number);
        let to_delete = found.len() - keep;
        for (message, _) in found.into_iter().take(to_delete) {
            self.channel.delete_message(message.id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::InMemoryChannel;

    const TEST_KEY: [u8; 32] = [0u8; 32];

    fn put_version(ch: &InMemoryChannel, game_id: &str, number: u64) {
        let rec = VersionRecord {
            marker: MARKER.to_string(),
            game_id: game_id.to_string(),
            version: SaveVersion {
                number,
                content_hash: format!("hash{number}"),
                created_at_ms: 1_000 + number,
                author: "tester".into(),
                device_id: "dev".into(),
                size_bytes: 10,
                chunk_count: 1,
            },
        };
        ch.send_message(
            &rec.encode(&TEST_KEY, game_id).unwrap(),
            &[("chunk_0.bin".into(), vec![0u8; 10])],
        )
        .unwrap();
    }

    #[test]
    fn latest_version_is_none_for_unknown_game() {
        let vault = Vault::new(InMemoryChannel::new(), [0u8; 32]);
        assert_eq!(vault.latest_version("nope").unwrap(), None);
    }

    #[test]
    fn latest_version_picks_highest_number() {
        let ch = InMemoryChannel::new();
        put_version(&ch, "valheim", 1);
        put_version(&ch, "valheim", 2);
        put_version(&ch, "valheim", 3);
        let vault = Vault::new(ch, [0u8; 32]);
        assert_eq!(vault.latest_version("valheim").unwrap().unwrap().number, 3);
    }

    #[test]
    fn versions_are_isolated_per_game() {
        let ch = InMemoryChannel::new();
        put_version(&ch, "valheim", 1);
        put_version(&ch, "terraria", 1);
        put_version(&ch, "terraria", 2);
        let vault = Vault::new(ch, [0u8; 32]);
        assert_eq!(vault.list_versions("valheim").unwrap().len(), 1);
        assert_eq!(vault.list_versions("terraria").unwrap().len(), 2);
    }

    #[test]
    fn unrelated_messages_are_ignored() {
        let ch = InMemoryChannel::new();
        ch.send_message("hello chat", &[]).unwrap();
        put_version(&ch, "valheim", 1);
        let vault = Vault::new(ch, [0u8; 32]);
        assert_eq!(vault.list_versions("valheim").unwrap().len(), 1);
    }

    #[test]
    fn push_creates_version_one_then_two() {
        let vault = Vault::new(InMemoryChannel::new(), [3u8; 32]);
        let v1 = vault
            .push_version(
                "valheim", "valheim", b"save-A", "Gabriel", "pc-1", 1_000, 10,
            )
            .unwrap();
        assert_eq!(v1.number, 1);
        assert_eq!(v1.author, "Gabriel");
        assert_eq!(v1.size_bytes, b"save-A".len() as u64);
        assert!(v1.chunk_count >= 1);

        let v2 = vault
            .push_version(
                "valheim",
                "valheim",
                b"save-B-different",
                "Ana",
                "pc-2",
                2_000,
                10,
            )
            .unwrap();
        assert_eq!(v2.number, 2);
        assert_eq!(vault.latest_version("valheim").unwrap().unwrap().number, 2);
    }

    #[test]
    fn push_is_noop_when_content_unchanged() {
        let vault = Vault::new(InMemoryChannel::new(), [3u8; 32]);
        let v1 = vault
            .push_version(
                "valheim",
                "valheim",
                b"same-bytes",
                "Gabriel",
                "pc-1",
                1_000,
                10,
            )
            .unwrap();
        let again = vault
            .push_version(
                "valheim",
                "valheim",
                b"same-bytes",
                "Gabriel",
                "pc-1",
                2_000,
                10,
            )
            .unwrap();
        // No new version created; returns the existing latest.
        assert_eq!(again.number, v1.number);
        assert_eq!(vault.list_versions("valheim").unwrap().len(), 1);
    }

    #[test]
    fn push_splits_large_save_into_multiple_chunks() {
        // Use a tiny max chunk size to force chunking of incompressible-ish data.
        let vault = Vault::new(InMemoryChannel::new(), [3u8; 32]).with_max_chunk_size(64);
        let big: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let v = vault
            .push_version("game", "game", &big, "a", "d", 1, 10)
            .unwrap();
        assert!(
            v.chunk_count > 1,
            "expected multiple chunks, got {}",
            v.chunk_count
        );
    }

    #[test]
    fn download_recovers_exact_save_bytes() {
        let vault = Vault::new(InMemoryChannel::new(), [9u8; 32]);
        let save = b"the actual world save bytes \x00\x01\x02";
        vault
            .push_version("valheim", "valheim", save, "a", "d", 1, 10)
            .unwrap();
        let got = vault.download("valheim", 1).unwrap();
        assert_eq!(got, save);
    }

    #[test]
    fn download_recovers_multichunk_save() {
        let vault = Vault::new(InMemoryChannel::new(), [9u8; 32]).with_max_chunk_size(64);
        let big: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let v = vault
            .push_version("game", "game", &big, "a", "d", 1, 10)
            .unwrap();
        assert!(v.chunk_count > 1);
        assert_eq!(vault.download("game", v.number).unwrap(), big);
    }

    #[test]
    fn download_missing_version_is_not_found() {
        let vault = Vault::new(InMemoryChannel::new(), [9u8; 32]);
        vault
            .push_version("valheim", "valheim", b"x", "a", "d", 1, 10)
            .unwrap();
        assert!(matches!(
            vault.download("valheim", 99),
            Err(VaultError::NotFound)
        ));
        assert!(matches!(
            vault.download("other", 1),
            Err(VaultError::NotFound)
        ));
    }

    #[test]
    fn download_with_wrong_key_fails() {
        let ch = InMemoryChannel::new();
        Vault::new(&ch, [1u8; 32])
            .push_version("valheim", "valheim", b"secret save", "a", "d", 1, 10)
            .unwrap();
        // A different key cannot open the sealed blob.
        let wrong = Vault::new(&ch, [2u8; 32]);
        assert!(wrong.download("valheim", 1).is_err());
    }

    #[test]
    fn prune_keeps_only_last_n_versions() {
        let vault = Vault::new(InMemoryChannel::new(), [4u8; 32]);
        // max_versions = 3; push 5 distinct versions.
        for i in 0..5u8 {
            let body = vec![i; (i as usize) + 1]; // distinct content each time
            vault
                .push_version("game", "game", &body, "a", "d", i as u64, 3)
                .unwrap();
        }
        let versions = vault.list_versions("game").unwrap();
        assert_eq!(versions.len(), 3);
        // The three newest version numbers are kept (3, 4, 5).
        let numbers: Vec<u64> = versions.iter().map(|v| v.number).collect();
        assert_eq!(numbers, vec![3, 4, 5]);
    }

    #[test]
    fn prune_does_not_touch_other_games() {
        let vault = Vault::new(InMemoryChannel::new(), [4u8; 32]);
        vault
            .push_version("keep", "keep", b"only-one", "a", "d", 1, 2)
            .unwrap();
        for i in 0..4u8 {
            vault
                .push_version(
                    "churn",
                    "churn",
                    &vec![i; (i as usize) + 1],
                    "a",
                    "d",
                    i as u64,
                    2,
                )
                .unwrap();
        }
        assert_eq!(vault.list_versions("keep").unwrap().len(), 1);
        assert_eq!(vault.list_versions("churn").unwrap().len(), 2);
    }

    #[test]
    fn downloading_a_pruned_version_is_not_found() {
        let vault = Vault::new(InMemoryChannel::new(), [4u8; 32]);
        for i in 0..4u8 {
            vault
                .push_version(
                    "game",
                    "game",
                    &vec![i; (i as usize) + 1],
                    "a",
                    "d",
                    i as u64,
                    2,
                )
                .unwrap();
        }
        // Versions 1 and 2 were pruned (only 3 and 4 remain).
        assert!(matches!(
            vault.download("game", 1),
            Err(VaultError::NotFound)
        ));
        assert_eq!(vault.download("game", 4).unwrap(), vec![3u8; 4]);
    }
}
