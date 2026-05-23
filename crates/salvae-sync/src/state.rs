//! Per-game last-synced version state.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::SyncError;

/// The highest version number this device has pulled or pushed, per game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    last_synced: BTreeMap<String, u64>,
}

impl SyncState {
    /// The last-synced version of `game_id`, if any.
    pub fn get(&self, game_id: &str) -> Option<u64> {
        self.last_synced.get(game_id).copied()
    }

    /// Record the last-synced version of `game_id`.
    pub fn set(&mut self, game_id: &str, version: u64) {
        self.last_synced.insert(game_id.to_string(), version);
    }

    /// Load state from `path`, or default (empty) if the file is absent.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SyncError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(|e| SyncError::Io(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| SyncError::Serde(e.to_string()))
    }

    /// Persist state to `path` (creating parent dirs).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SyncError> {
        let path = path.as_ref();
        let text =
            serde_json::to_string_pretty(self).map_err(|e| SyncError::Serde(e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SyncError::Io(e.to_string()))?;
        }
        std::fs::write(path, text).map_err(|e| SyncError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_round_trip() {
        let mut s = SyncState::default();
        assert_eq!(s.get("g1"), None);
        s.set("g1", 5);
        assert_eq!(s.get("g1"), Some(5));
    }

    #[test]
    fn save_then_load_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.json");
        let mut s = SyncState::default();
        s.set("valheim", 7);
        s.save(&path).unwrap();

        let loaded = SyncState::load(&path).unwrap();
        assert_eq!(loaded.get("valheim"), Some(7));
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let s = SyncState::load(dir.path().join("nope.json")).unwrap();
        assert_eq!(s.get("any"), None);
    }
}
