//! Secret storage trait (bot token + group key) and in-memory implementation.

use std::collections::HashMap;

use salvae_core::kdf::KEY_LEN;

use crate::ConfigError;

/// The per-group secrets: the Discord bot token and the derived group key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSecret {
    pub token: String,
    pub key: [u8; KEY_LEN],
}

/// Storage for per-group secrets. Implementations protect secrets at rest
/// (the production one uses Windows DPAPI).
pub trait SecretStore {
    /// Fetch a group's secret, or `None` if absent.
    fn get(&self, group_id: &str) -> Result<Option<GroupSecret>, ConfigError>;
    /// Store (or replace) a group's secret.
    fn set(&mut self, group_id: &str, secret: GroupSecret) -> Result<(), ConfigError>;
    /// Remove a group's secret (no error if absent).
    fn remove(&mut self, group_id: &str) -> Result<(), ConfigError>;
}

/// A non-persistent secret store for tests and downstream use.
#[derive(Default)]
pub struct InMemorySecretStore {
    map: HashMap<String, GroupSecret>,
}

impl InMemorySecretStore {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl SecretStore for InMemorySecretStore {
    fn get(&self, group_id: &str) -> Result<Option<GroupSecret>, ConfigError> {
        Ok(self.map.get(group_id).cloned())
    }

    fn set(&mut self, group_id: &str, secret: GroupSecret) -> Result<(), ConfigError> {
        self.map.insert(group_id.to_string(), secret);
        Ok(())
    }

    fn remove(&mut self, group_id: &str) -> Result<(), ConfigError> {
        self.map.remove(group_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> GroupSecret {
        GroupSecret {
            token: "tok".into(),
            key: [7u8; KEY_LEN],
        }
    }

    #[test]
    fn set_get_remove_round_trip() {
        let mut store = InMemorySecretStore::new();
        assert_eq!(store.get("g1").unwrap(), None);
        store.set("g1", secret()).unwrap();
        assert_eq!(store.get("g1").unwrap(), Some(secret()));
        store.remove("g1").unwrap();
        assert_eq!(store.get("g1").unwrap(), None);
    }

    #[test]
    fn remove_absent_is_ok() {
        let mut store = InMemorySecretStore::new();
        assert!(store.remove("nope").is_ok());
    }
}
