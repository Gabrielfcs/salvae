//! Salvaê sync engine: moves saves between the local filesystem and the
//! encrypted vault, with versioning, stale-overwrite conflict handling, and an
//! advisory "currently playing" marker.

pub mod engine;
pub mod marker;
pub mod pack;
pub mod state;

/// Errors produced by the sync engine.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Error from the vault (storage/transport/crypto).
    #[error("vault error: {0}")]
    Vault(#[from] salvae_vault::VaultError),
    /// Cryptographic error (e.g. sealing a presence marker).
    #[error("crypto error: {0}")]
    Core(#[from] salvae_core::CoreError),
    /// Filesystem error reading/writing save folders, state, or backups.
    #[error("io error: {0}")]
    Io(String),
    /// The pack/unpack archive format was malformed or contained a bad path.
    #[error("pack error: {0}")]
    Pack(String),
    /// (De)serialization of sync state or a marker failed.
    #[error("serialization error: {0}")]
    Serde(String),
}

#[cfg(test)]
mod error_tests {
    use super::SyncError;

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(
            SyncError::Pack("boom".into()).to_string(),
            "pack error: boom"
        );
        assert_eq!(SyncError::Io("disk".into()).to_string(), "io error: disk");
    }

    #[test]
    fn vault_errors_convert() {
        let v: SyncError = salvae_vault::VaultError::NotFound.into();
        assert!(matches!(v, SyncError::Vault(_)));
    }
}
