//! The full save-blob recipe: compress then encrypt (`seal`) and its inverse
//! (`open`). This is the exact byte format the Discord vault (Plan 2) stores.

use crate::{cipher, compress, CoreError};

/// Compress (zstd) then encrypt (AES-256-GCM). Produces the stored save blob.
pub fn seal(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CoreError> {
    let compressed = compress::compress(plaintext, compress::DEFAULT_LEVEL)?;
    cipher::encrypt(key, &compressed)
}

/// Inverse of [`seal`]: decrypt then decompress.
pub fn open(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, CoreError> {
    let compressed = cipher::decrypt(key, blob)?;
    compress::decompress(&compressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_recovers_plaintext() {
        let key = [5u8; 32];
        let data = b"Valheim world data ".repeat(200);
        let blob = seal(&key, &data).unwrap();
        assert_eq!(open(&key, &blob).unwrap(), data);
    }

    #[test]
    fn sealed_blob_is_not_plaintext() {
        let key = [5u8; 32];
        let data = b"plain readable text".repeat(20);
        let blob = seal(&key, &data).unwrap();
        // The original substring must not appear verbatim in the sealed blob.
        assert!(blob.windows(data.len()).all(|w| w != &data[..]));
    }

    #[test]
    fn wrong_key_cannot_open() {
        let blob = seal(&[1u8; 32], b"secret save").unwrap();
        assert!(matches!(open(&[2u8; 32], &blob), Err(CoreError::Decrypt)));
    }
}
