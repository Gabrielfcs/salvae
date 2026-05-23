//! "Currently playing" presence marker.
//!
//! Posted as a small JSON message in the group channel (distinct marker string,
//! so the vault ignores it). Advisory only, with a TTL so stale markers (from a
//! crash) expire on their own.

use serde::{Deserialize, Serialize};

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
    /// Serialize to the JSON stored as a message's content.
    pub fn to_content(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Parse a message's content into a record, or `None` if it is not a
    /// Salvaê playing marker.
    pub fn parse(content: &str) -> Option<PlayingRecord> {
        let rec: PlayingRecord = serde_json::from_str(content).ok()?;
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

    #[test]
    fn to_content_then_parse_round_trips() {
        let r = rec(5000);
        assert_eq!(PlayingRecord::parse(&r.to_content()).unwrap(), r);
    }

    #[test]
    fn parse_rejects_other_messages() {
        assert!(PlayingRecord::parse("hello chat").is_none());
        assert!(PlayingRecord::parse("{\"marker\":\"something-else\"}").is_none());
    }

    #[test]
    fn active_until_expiry() {
        let r = rec(1000);
        assert!(r.is_active(999));
        assert!(!r.is_active(1000));
        assert!(!r.is_active(1001));
    }
}
