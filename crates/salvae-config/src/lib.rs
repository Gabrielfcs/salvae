//! Salvaê config: groups, encrypted invites, local `config.toml`, and
//! DPAPI-protected secret storage (bot token + derived group key).

pub mod group;
pub mod invite;
pub mod secret;
pub mod store;

#[cfg(windows)]
pub mod dpapi;

/// Errors produced by config/group operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A crypto error from salvae-core.
    #[error("core error: {0}")]
    Core(#[from] salvae_core::CoreError),
    /// Filesystem error reading/writing config or secrets.
    #[error("io error: {0}")]
    Io(String),
    /// (De)serialization of config or invite payload failed.
    #[error("serialization error: {0}")]
    Serde(String),
    /// The invite string is malformed (bad base64 / too short / bad payload).
    #[error("invalid invite: {0}")]
    Invite(String),
    /// The password did not decrypt the invite (wrong password or corruption).
    #[error("wrong password or corrupted invite")]
    WrongPassword,
    /// No group with the given id exists locally.
    #[error("group not found: {0}")]
    GroupNotFound(String),
    /// The platform secret store (DPAPI) failed.
    #[error("secret store error: {0}")]
    Secret(String),
}

#[cfg(test)]
mod error_tests {
    use super::ConfigError;

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(
            ConfigError::WrongPassword.to_string(),
            "wrong password or corrupted invite"
        );
        assert_eq!(
            ConfigError::GroupNotFound("g1".into()).to_string(),
            "group not found: g1"
        );
    }

    #[test]
    fn core_errors_convert() {
        let v: ConfigError = salvae_core::CoreError::Decrypt.into();
        assert!(matches!(v, ConfigError::Core(_)));
    }
}
