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
        Self { channel, key, max_chunk_size: DEFAULT_MAX_CHUNK }
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
                if let Some(rec) = VersionRecord::parse(&msg.content) {
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
        let mut versions: Vec<SaveVersion> = self.scan(game_id)?.into_iter().map(|(_, v)| v).collect();
        versions.sort_by_key(|v| v.number);
        Ok(versions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::InMemoryChannel;

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
        ch.send_message(&rec.to_content(), &[("chunk_0.bin".into(), vec![0u8; 10])]).unwrap();
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
}
