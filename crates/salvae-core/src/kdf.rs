//! Argon2id password-based key derivation.

use crate::CoreError;
use argon2::{Algorithm, Argon2, Params, Version};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;

/// Derive a 32-byte symmetric key from a password and salt using Argon2id.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], CoreError> {
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default());
    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| CoreError::Kdf(e.to_string()))?;
    Ok(key)
}

/// Generate a random 16-byte salt from the OS RNG.
pub fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    getrandom::getrandom(&mut salt).expect("OS RNG failure");
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_password_and_salt_yield_same_key() {
        let salt = [7u8; SALT_LEN];
        let a = derive_key("hunter2", &salt).unwrap();
        let b = derive_key("hunter2", &salt).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), KEY_LEN);
    }

    #[test]
    fn different_password_yields_different_key() {
        let salt = [7u8; SALT_LEN];
        let a = derive_key("hunter2", &salt).unwrap();
        let b = derive_key("hunter3", &salt).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn different_salt_yields_different_key() {
        let a = derive_key("hunter2", &[1u8; SALT_LEN]).unwrap();
        let b = derive_key("hunter2", &[2u8; SALT_LEN]).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn generated_salts_differ() {
        assert_ne!(generate_salt(), generate_salt());
    }
}
