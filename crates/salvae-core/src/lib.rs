//! Salvaê core: pure crypto, compression, chunking and version model.
//!
//! No OS or network dependencies — fully unit-testable.

pub mod kdf;
pub mod cipher;
pub mod compress;
pub mod chunk;
pub mod hash;
pub mod version;

/// Errors produced by salvae-core operations.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed (wrong password or corrupted data)")]
    Decrypt,
    #[error("compression failed: {0}")]
    Compress(String),
    #[error("decompression failed: {0}")]
    Decompress(String),
    #[error("invalid chunk data: {0}")]
    Chunk(String),
    #[error("serialization failed: {0}")]
    Serde(String),
}

#[cfg(test)]
mod error_tests {
    use super::CoreError;

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(
            CoreError::Decrypt.to_string(),
            "decryption failed (wrong password or corrupted data)"
        );
        assert_eq!(CoreError::Kdf("boom".into()).to_string(), "key derivation failed: boom");
    }
}
