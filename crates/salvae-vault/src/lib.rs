//! Salvaê vault: encrypted, versioned save storage over a message channel.
//!
//! The vault logic is transport-agnostic (see [`channel::Channel`]); the live
//! Discord REST transport lives in a separate crate/plan and reuses the pure
//! helpers in [`multipart`] and [`ratelimit`].

pub mod channel;
pub mod memory;
pub mod multipart;
pub mod ratelimit;
pub mod record;
pub mod vault;

/// Errors produced by vault operations.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// A crypto/compression/chunking error from salvae-core.
    #[error("core error: {0}")]
    Core(#[from] salvae_core::CoreError),
    /// The underlying channel/transport failed (network, HTTP, IO).
    #[error("transport error: {0}")]
    Transport(String),
    /// Serialization/deserialization of a version record failed.
    #[error("record error: {0}")]
    Record(String),
    /// The requested version/game does not exist in the channel.
    #[error("requested save version was not found in the vault")]
    NotFound,
    /// A downloaded save did not match its recorded content hash.
    #[error("downloaded save failed its integrity check (hash mismatch)")]
    Integrity,
}

#[cfg(test)]
mod error_tests {
    use super::VaultError;

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(
            VaultError::NotFound.to_string(),
            "requested save version was not found in the vault"
        );
        assert_eq!(
            VaultError::Integrity.to_string(),
            "downloaded save failed its integrity check (hash mismatch)"
        );
        assert_eq!(VaultError::Transport("boom".into()).to_string(), "transport error: boom");
    }

    #[test]
    fn core_errors_convert_into_vault_errors() {
        let core = salvae_core::CoreError::Decrypt;
        let v: VaultError = core.into();
        assert!(matches!(v, VaultError::Core(_)));
    }
}
