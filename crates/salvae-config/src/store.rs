//! ConfigStore: ties config.toml and the SecretStore together.

use std::path::{Path, PathBuf};

use crate::group::{AppConfig, GroupConfig, DEFAULT_MAX_VERSIONS};
use crate::invite;
use crate::secret::{GroupSecret, SecretStore};
use crate::ConfigError;

/// Ties the on-disk `config.toml` to a `SecretStore`.
pub struct ConfigStore<S: SecretStore> {
    config_path: PathBuf,
    config: AppConfig,
    secrets: S,
}

impl<S: SecretStore> ConfigStore<S> {
    /// Borrow the device id.
    pub fn device_id(&self) -> &str {
        &self.config.device_id
    }

    /// All groups this install belongs to.
    pub fn groups(&self) -> &[GroupConfig] {
        &self.config.groups
    }

    /// Load the config at `config_path`, or create a fresh one (with a new
    /// device id) if the file does not exist. Does not write anything.
    pub fn load_or_default(config_path: impl AsRef<Path>, secrets: S) -> Result<Self, ConfigError> {
        let config_path = config_path.as_ref().to_path_buf();
        let config = if config_path.exists() {
            let text = std::fs::read_to_string(&config_path)
                .map_err(|e| ConfigError::Io(e.to_string()))?;
            toml::from_str(&text).map_err(|e| ConfigError::Serde(e.to_string()))?
        } else {
            AppConfig { device_id: random_id(16), groups: Vec::new() }
        };
        Ok(Self { config_path, config, secrets })
    }

    /// Write the current config to `config_path` (creating parent dirs).
    pub fn save(&self) -> Result<(), ConfigError> {
        let text =
            toml::to_string_pretty(&self.config).map_err(|e| ConfigError::Serde(e.to_string()))?;
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io(e.to_string()))?;
        }
        std::fs::write(&self.config_path, text).map_err(|e| ConfigError::Io(e.to_string()))
    }
}

/// Generate a random lowercase-hex id of `bytes` random bytes.
pub(crate) fn random_id(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS RNG failure");
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::InMemorySecretStore;

    #[test]
    fn load_or_default_creates_fresh_config_with_device_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
        assert!(!store.device_id().is_empty());
        assert!(store.groups().is_empty());
    }

    #[test]
    fn save_then_reload_preserves_device_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
        let id = store.device_id().to_string();
        store.save().unwrap();

        let reloaded = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
        assert_eq!(reloaded.device_id(), id);
    }

    #[test]
    fn random_id_is_hex_and_unique() {
        let a = random_id(8);
        let b = random_id(8);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
