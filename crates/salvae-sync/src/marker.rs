//! "Currently playing" presence marker.
//!
//! Posted as a friendly line plus an encrypted `Seed:` token in the group
//! channel (a distinct marker string inside, so the vault ignores it). Advisory
//! only, with a TTL so stale markers (from a crash) expire on their own.

use serde::{Deserialize, Serialize};

use salvae_core::{seed, CoreError};

/// Marker string identifying a "currently playing" message.
pub const PLAYING_MARKER: &str = "salvae-playing-v1";

/// A presence record: who is playing which game, until when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayingRecord {
    pub marker: String,
    pub game_id: String,
    pub member: String,
    pub device_id: String,
    pub expires_at_ms: u64,
}

impl PlayingRecord {
    /// Build the channel message: a friendly line plus the encrypted `Seed:`
    /// token carrying this record. `game_name` is the human title (falls back to
    /// the id if empty).
    pub fn encode(&self, key: &[u8; 32], game_name: &str) -> Result<String, CoreError> {
        let json = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string());
        let token = seed::seal_to_token(key, json.as_bytes())?;
        let title = if game_name.is_empty() {
            &self.game_id
        } else {
            game_name
        };
        Ok(format!(
            "{} está jogando: {}\n\nSeed: {}",
            self.member, title, token
        ))
    }

    /// Recover a record from a message's content, or `None` if it is not a
    /// Salvaê playing marker (no Seed line, not ours, or wrong marker).
    pub fn decode(content: &str, key: &[u8; 32]) -> Option<PlayingRecord> {
        let token = seed::seed_from_message(content)?;
        let bytes = seed::open_from_token(key, token)?;
        let rec: PlayingRecord = serde_json::from_slice(&bytes).ok()?;
        (rec.marker == PLAYING_MARKER).then_some(rec)
    }

    /// Whether this marker is still active at `now_ms`.
    pub fn is_active(&self, now_ms: u64) -> bool {
        self.expires_at_ms > now_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(expires: u64) -> PlayingRecord {
        PlayingRecord {
            marker: PLAYING_MARKER.into(),
            game_id: "valheim".into(),
            member: "Gabriel".into(),
            device_id: "dev-1".into(),
            expires_at_ms: expires,
        }
    }

    const KEY: [u8; 32] = [6u8; 32];

    #[test]
    fn encode_then_decode_round_trips() {
        let r = rec(5000);
        let content = r.encode(&KEY, "Valheim").unwrap();
        assert_eq!(PlayingRecord::decode(&content, &KEY).unwrap(), r);
    }

    #[test]
    fn encoded_message_is_friendly_and_hides_the_marker() {
        let content = rec(5000).encode(&KEY, "Valheim").unwrap();
        assert!(content.starts_with("Gabriel está jogando: Valheim"));
        assert!(!content.contains("salvae-playing-v1"));
    }

    #[test]
    fn decode_rejects_other_messages() {
        assert!(PlayingRecord::decode("hello chat", &KEY).is_none());
        assert!(PlayingRecord::decode("Seed: garbage!!", &KEY).is_none());
        // A valid token sealed with another key is not readable.
        let content = rec(5000).encode(&KEY, "Valheim").unwrap();
        assert!(PlayingRecord::decode(&content, &[0u8; 32]).is_none());
    }

    #[test]
    fn active_until_expiry() {
        let r = rec(1000);
        assert!(r.is_active(999));
        assert!(!r.is_active(1000));
        assert!(!r.is_active(1001));
    }
}
