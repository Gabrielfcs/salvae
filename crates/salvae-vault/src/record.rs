//! Version record: the machine metadata carried by a save-version message.
//!
//! Each save version is one channel message. Its visible content is a friendly
//! human line plus an encrypted `Seed:` token (the record below, sealed with the
//! group key and base64-encoded), and its attachments are the sealed save split
//! into `chunk_i.bin` parts. The friendly line keeps the channel readable; the
//! Seed keeps the metadata private to members and out of sight.

use serde::{Deserialize, Serialize};

use salvae_core::version::SaveVersion;
use salvae_core::{seed, CoreError};

/// Marker string identifying a message as a Salvaê save version (so the vault
/// can ignore unrelated messages in the channel).
pub const MARKER: &str = "salvae-save-v1";

/// The JSON header stored in a save-version message's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRecord {
    /// Always [`MARKER`]; lets the vault recognize its own messages.
    pub marker: String,
    /// Which game this save belongs to (caller-defined stable id).
    pub game_id: String,
    /// The version metadata.
    pub version: SaveVersion,
}

impl VersionRecord {
    /// Build the channel message: a friendly line naming the author and game,
    /// followed by the encrypted `Seed:` token carrying this record. `game_name`
    /// is the human title to show (falls back to the id if empty).
    pub fn encode(&self, key: &[u8; 32], game_name: &str) -> Result<String, CoreError> {
        let json = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string());
        let token = seed::seal_to_token(key, json.as_bytes())?;
        let title = if game_name.is_empty() {
            &self.game_id
        } else {
            game_name
        };
        Ok(format!(
            "{} acaba de emitir uma sincronização dos saves do jogo: {}\n\nSeed: {}",
            self.version.author, title, token
        ))
    }

    /// Recover a record from a message's content, or `None` if it is not a
    /// Salvaê save-version message (no Seed line, not ours, or wrong marker).
    pub fn decode(content: &str, key: &[u8; 32]) -> Option<VersionRecord> {
        let token = seed::seed_from_message(content)?;
        let bytes = seed::open_from_token(key, token)?;
        let rec: VersionRecord = serde_json::from_slice(&bytes).ok()?;
        (rec.marker == MARKER).then_some(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VersionRecord {
        VersionRecord {
            marker: MARKER.to_string(),
            game_id: "valheim".into(),
            version: SaveVersion {
                number: 2,
                content_hash: "abc123".into(),
                created_at_ms: 1_716_400_000_000,
                author: "Gabriel".into(),
                device_id: "pc-gabriel".into(),
                size_bytes: 2048,
                chunk_count: 1,
            },
        }
    }

    const KEY: [u8; 32] = [9u8; 32];

    #[test]
    fn encode_then_decode_round_trips() {
        let rec = sample();
        let content = rec.encode(&KEY, "Valheim").unwrap();
        let back = VersionRecord::decode(&content, &KEY).unwrap();
        assert_eq!(rec, back);
    }

    #[test]
    fn encoded_message_is_friendly_and_hides_the_json() {
        let content = sample().encode(&KEY, "Valheim").unwrap();
        assert!(content
            .starts_with("Gabriel acaba de emitir uma sincronização dos saves do jogo: Valheim"));
        assert!(content.contains("Seed:"));
        // The raw metadata must not leak into the visible text.
        assert!(!content.contains("salvae-save-v1"));
        assert!(!content.contains("content_hash"));
    }

    #[test]
    fn decode_rejects_non_salvae_content() {
        assert!(VersionRecord::decode("just a normal chat message", &KEY).is_none());
        assert!(VersionRecord::decode("Seed: not-base64!!", &KEY).is_none());
    }

    #[test]
    fn decode_rejects_a_token_sealed_with_another_key() {
        let content = sample().encode(&KEY, "Valheim").unwrap();
        assert!(VersionRecord::decode(&content, &[0u8; 32]).is_none());
    }

    #[test]
    fn content_is_small() {
        // Sanity: a message stays well under Discord's 2000-char content limit.
        assert!(sample().encode(&KEY, "Valheim").unwrap().len() < 2000);
    }
}
