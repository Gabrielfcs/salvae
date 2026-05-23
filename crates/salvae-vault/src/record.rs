//! Version record: the JSON header stored in a save-version message.
//!
//! Each save version is one channel message whose `content` is a `VersionRecord`
//! serialized as JSON (small — well under Discord's 2000-char content limit),
//! and whose attachments are the sealed save split into `chunk_i.bin` parts.

use serde::{Deserialize, Serialize};

use salvae_core::version::SaveVersion;

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
    /// Serialize to the JSON string stored as a message's content.
    pub fn to_content(&self) -> String {
        // Serialization of this small, owned struct cannot fail in practice;
        // fall back to an empty object only to avoid a panic.
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Parse a message's content into a record, or `None` if it is not a
    /// Salvaê save-version message (unrelated text, bad JSON, or wrong marker).
    pub fn parse(content: &str) -> Option<VersionRecord> {
        let rec: VersionRecord = serde_json::from_str(content).ok()?;
        if rec.marker == MARKER {
            Some(rec)
        } else {
            None
        }
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

    #[test]
    fn to_content_then_parse_round_trips() {
        let rec = sample();
        let content = rec.to_content();
        let back = VersionRecord::parse(&content).unwrap();
        assert_eq!(rec, back);
    }

    #[test]
    fn parse_rejects_non_salvae_content() {
        assert!(VersionRecord::parse("just a normal chat message").is_none());
        // Valid JSON but wrong/absent marker:
        assert!(VersionRecord::parse("{\"hello\":1}").is_none());
    }

    #[test]
    fn parse_rejects_wrong_marker_value() {
        let mut rec = sample();
        rec.marker = "something-else".into();
        let content = serde_json::to_string(&rec).unwrap();
        assert!(VersionRecord::parse(&content).is_none());
    }

    #[test]
    fn content_is_small() {
        // Sanity: a record stays well under Discord's 2000-char message limit.
        assert!(sample().to_content().len() < 2000);
    }
}
