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
            AppConfig {
                device_id: random_id(16),
                groups: Vec::new(),
            }
        };
        Ok(Self {
            config_path,
            config,
            secrets,
        })
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

    /// Create a new group: derive its key from `password`, store the secret,
    /// persist the config, and return the group plus a shareable invite string.
    pub fn create_group(
        &mut self,
        name: &str,
        password: &str,
        token: &str,
        guild_id: u64,
        channel_id: u64,
    ) -> Result<(GroupConfig, String), ConfigError> {
        let salt = salvae_core::kdf::generate_salt();
        let key = salvae_core::kdf::derive_key(password, &salt)?;
        let id = random_id(8);

        let group = GroupConfig {
            id: id.clone(),
            name: name.to_string(),
            guild_id,
            channel_id,
            salt: hex::encode(salt),
            max_versions: DEFAULT_MAX_VERSIONS,
            game_paths: Default::default(),
        };

        self.secrets.set(
            &id,
            GroupSecret {
                token: token.to_string(),
                key,
            },
        )?;
        self.config.groups.push(group.clone());
        self.save()?;

        let invite = invite::encode_invite(password, &salt, name, token, guild_id, channel_id)?;
        Ok((group, invite))
    }

    /// Join a group from an invite string + the shared password.
    pub fn join_group(&mut self, password: &str, invite: &str) -> Result<GroupConfig, ConfigError> {
        let decoded = invite::decode_invite(password, invite)?;
        let id = random_id(8);

        let group = GroupConfig {
            id: id.clone(),
            name: decoded.name,
            guild_id: decoded.guild_id,
            channel_id: decoded.channel_id,
            salt: hex::encode(decoded.salt),
            max_versions: DEFAULT_MAX_VERSIONS,
            game_paths: Default::default(),
        };

        self.secrets.set(
            &id,
            GroupSecret {
                token: decoded.token,
                key: decoded.key,
            },
        )?;
        self.config.groups.push(group.clone());
        self.save()?;
        Ok(group)
    }

    /// Fetch a group's secret (token + key) for the sync engine.
    pub fn group_secret(&self, group_id: &str) -> Result<GroupSecret, ConfigError> {
        self.secrets
            .get(group_id)?
            .ok_or_else(|| ConfigError::GroupNotFound(group_id.to_string()))
    }

    /// Remove a group and its secret, then persist.
    pub fn remove_group(&mut self, group_id: &str) -> Result<(), ConfigError> {
        self.config.groups.retain(|g| g.id != group_id);
        self.secrets.remove(group_id)?;
        self.save()
    }

    /// Set (or replace) the local save folder for `game_id` in `group_id`, then
    /// persist. Errors if the group is unknown.
    pub fn set_game_path(
        &mut self,
        group_id: &str,
        game_id: &str,
        folder: &str,
    ) -> Result<(), ConfigError> {
        let group = self
            .config
            .groups
            .iter_mut()
            .find(|g| g.id == group_id)
            .ok_or_else(|| ConfigError::GroupNotFound(group_id.to_string()))?;
        group.game_paths.insert(game_id.to_string(), folder.to_string());
        self.save()
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

    #[test]
    fn create_group_persists_and_yields_a_working_invite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut store = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();

        let (group, invite) = store
            .create_group("Crew", "pw123", "bot-token", 111, 222)
            .unwrap();
        assert_eq!(group.name, "Crew");
        assert_eq!(store.groups().len(), 1);

        // The group's secret is stored (token + key).
        let secret = store.group_secret(&group.id).unwrap();
        assert_eq!(secret.token, "bot-token");

        // The produced invite decodes (with the password) to the same info+key.
        let decoded = crate::invite::decode_invite("pw123", &invite).unwrap();
        assert_eq!(decoded.token, "bot-token");
        assert_eq!(decoded.guild_id, 111);
        assert_eq!(decoded.key, secret.key);
    }

    #[test]
    fn join_group_from_invite_adds_group_and_secret() {
        let dir = tempfile::tempdir().unwrap();

        // Owner creates a group on one "install".
        let owner_path = dir.path().join("owner.toml");
        let mut owner =
            ConfigStore::load_or_default(&owner_path, InMemorySecretStore::new()).unwrap();
        let (_g, invite) = owner
            .create_group("Crew", "pw123", "bot-token", 111, 222)
            .unwrap();

        // Friend joins on another "install".
        let friend_path = dir.path().join("friend.toml");
        let mut friend =
            ConfigStore::load_or_default(&friend_path, InMemorySecretStore::new()).unwrap();
        let joined = friend.join_group("pw123", &invite).unwrap();

        assert_eq!(joined.name, "Crew");
        assert_eq!(joined.guild_id, 111);
        assert_eq!(joined.channel_id, 222);
        let secret = friend.group_secret(&joined.id).unwrap();
        assert_eq!(secret.token, "bot-token");
    }

    #[test]
    fn join_with_wrong_password_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner =
            ConfigStore::load_or_default(dir.path().join("o.toml"), InMemorySecretStore::new())
                .unwrap();
        let (_g, invite) = owner.create_group("Crew", "right", "tok", 1, 2).unwrap();

        let mut friend =
            ConfigStore::load_or_default(dir.path().join("f.toml"), InMemorySecretStore::new())
                .unwrap();
        assert!(matches!(
            friend.join_group("wrong", &invite),
            Err(ConfigError::WrongPassword)
        ));
        assert!(friend.groups().is_empty());
    }

    #[test]
    fn remove_group_drops_config_and_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        let mut store = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
        let (group, _invite) = store.create_group("Crew", "pw", "tok", 1, 2).unwrap();

        store.remove_group(&group.id).unwrap();
        assert!(store.groups().is_empty());
        assert!(matches!(
            store.group_secret(&group.id),
            Err(ConfigError::GroupNotFound(_))
        ));
    }

    #[test]
    fn set_game_path_records_and_persists_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let group = {
            let mut store = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
            let g = store.create_group("Crew", "pw", "tok", 1, 2).unwrap().0;
            store
                .set_game_path(&g.id, "steam:892970", "C:/Users/me/AppData/LocalLow/Valheim")
                .unwrap();
            g
        };

        // Reload: the game path is persisted on the right group.
        let reloaded = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
        let stored = reloaded.groups().iter().find(|g| g.id == group.id).unwrap();
        assert_eq!(
            stored.game_paths.get("steam:892970").map(String::as_str),
            Some("C:/Users/me/AppData/LocalLow/Valheim")
        );
    }

    #[test]
    fn set_game_path_for_unknown_group_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut store =
            ConfigStore::load_or_default(dir.path().join("c.toml"), InMemorySecretStore::new())
                .unwrap();
        assert!(matches!(
            store.set_game_path("nope", "steam:1", "C:/x"),
            Err(ConfigError::GroupNotFound(_))
        ));
    }
}
