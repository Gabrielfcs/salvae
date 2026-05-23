//! Metadata record describing one stored save version. Pure data — clocks,
//! device IDs and version numbering are assigned by the sync engine (Plan 4).

use serde::{Deserialize, Serialize};

/// Metadata for one version of one game's save in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveVersion {
    /// Monotonic version number, starts at 1 for the first push.
    pub number: u64,
    /// BLAKE3 hex hash of the decrypted, decompressed save content.
    pub content_hash: String,
    /// Unix epoch milliseconds when this version was created.
    pub created_at_ms: u64,
    /// Display name of the member who pushed it.
    pub author: String,
    /// Stable identifier of the device that pushed it.
    pub device_id: String,
    /// Size in bytes of the decrypted, decompressed save content.
    pub size_bytes: u64,
    /// Number of encrypted chunks this version was split into.
    pub chunk_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SaveVersion {
        SaveVersion {
            number: 3,
            content_hash: "deadbeef".into(),
            created_at_ms: 1_716_400_000_000,
            author: "Gabriel".into(),
            device_id: "pc-gabriel".into(),
            size_bytes: 4096,
            chunk_count: 1,
        }
    }

    #[test]
    fn serializes_and_deserializes_round_trip() {
        let v = sample();
        let json = serde_json::to_string(&v).unwrap();
        let back: SaveVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn json_uses_expected_field_names() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("\"number\":3"));
        assert!(json.contains("\"content_hash\":\"deadbeef\""));
        assert!(json.contains("\"chunk_count\":1"));
    }
}
