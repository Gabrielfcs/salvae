//! Serializable config model: GroupConfig and AppConfig.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

/// Default number of save versions to retain per game.
pub const DEFAULT_MAX_VERSIONS: usize = 10;

/// One group's non-secret configuration (the bot token + key live in the
/// SecretStore, never here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupConfig {
    /// Local identifier for this group (random; not shared).
    pub id: String,
    /// Human-friendly group name.
    pub name: String,
    /// Discord server (guild) id.
    pub guild_id: u64,
    /// Discord channel id used as the save vault.
    pub channel_id: u64,
    /// Per-group Argon2 salt, hex-encoded (not secret).
    pub salt: String,
    /// How many versions to keep per game.
    pub max_versions: usize,
    /// This device's resolved local save folder per game id.
    #[serde(default)]
    pub game_paths: BTreeMap<String, String>,
}

impl GroupConfig {
    /// Decode the hex salt into raw bytes.
    pub fn salt_bytes(&self) -> Result<Vec<u8>, ConfigError> {
        hex::decode(&self.salt).map_err(|e| ConfigError::Serde(format!("bad salt hex: {e}")))
    }
}

/// The whole local app configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Stable per-install device id (used by the sync engine for authorship).
    pub device_id: String,
    /// Human display name shown as the author of saves this person pushes.
    #[serde(default)]
    pub display_name: String,
    /// All groups this install belongs to.
    #[serde(default)]
    pub groups: Vec<GroupConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AppConfig {
        AppConfig {
            device_id: "dev-abc".into(),
            display_name: "Gabriel".into(),
            groups: vec![GroupConfig {
                id: "g1".into(),
                name: "Co-op Crew".into(),
                guild_id: 111,
                channel_id: 222,
                salt: "00112233445566778899aabbccddeeff".into(),
                max_versions: DEFAULT_MAX_VERSIONS,
                game_paths: BTreeMap::new(),
            }],
        }
    }

    #[test]
    fn toml_round_trip() {
        let cfg = sample();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn salt_bytes_decodes_hex() {
        let g = &sample().groups[0];
        assert_eq!(g.salt_bytes().unwrap().len(), 16);
        assert_eq!(g.salt_bytes().unwrap()[0], 0x00);
        assert_eq!(g.salt_bytes().unwrap()[15], 0xff);
    }

    #[test]
    fn empty_groups_default_when_absent() {
        let text = "device_id = \"only-device\"\n";
        let cfg: AppConfig = toml::from_str(text).unwrap();
        assert_eq!(cfg.device_id, "only-device");
        assert!(cfg.groups.is_empty());
    }
}
