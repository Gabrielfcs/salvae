//! AES-256-GCM authenticated encryption.
//!
//! Wire layout of `encrypt` output: `nonce (12 bytes) || ciphertext+tag`.

use crate::CoreError;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};

pub const NONCE_LEN: usize = 12;

/// Encrypt `plaintext` with AES-256-GCM. Returns `nonce || ciphertext+tag`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CoreError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes).map_err(|_| CoreError::Encrypt)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).map_err(|_| CoreError::Encrypt)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt`] (`nonce || ciphertext+tag`).
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CoreError> {
    if data.len() < NONCE_LEN {
        return Err(CoreError::Decrypt);
    }
    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).map_err(|_| CoreError::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovers_plaintext() {
        let key = [9u8; 32];
        let msg = b"save data: world seed 12345";
        let blob = encrypt(&key, msg).unwrap();
        let back = decrypt(&key, &blob).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn ciphertext_is_not_plaintext_and_has_nonce_prefix() {
        let key = [9u8; 32];
        let msg = b"hello";
        let blob = encrypt(&key, msg).unwrap();
        assert!(blob.len() > NONCE_LEN + msg.len()); // nonce + ciphertext + 16-byte tag
        assert_ne!(&blob[NONCE_LEN..NONCE_LEN + msg.len()], msg);
    }

    #[test]
    fn two_encryptions_differ_due_to_random_nonce() {
        let key = [9u8; 32];
        let a = encrypt(&key, b"same").unwrap();
        let b = encrypt(&key, b"same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let blob = encrypt(&[1u8; 32], b"secret").unwrap();
        let err = decrypt(&[2u8; 32], &blob);
        assert!(matches!(err, Err(CoreError::Decrypt)));
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let key = [9u8; 32];
        let mut blob = encrypt(&key, b"secret").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert!(matches!(decrypt(&key, &blob), Err(CoreError::Decrypt)));
    }

    #[test]
    fn too_short_input_fails_to_decrypt() {
        assert!(matches!(decrypt(&[0u8; 32], &[0u8; 4]), Err(CoreError::Decrypt)));
    }
}
