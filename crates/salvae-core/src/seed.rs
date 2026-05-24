//! The opaque "Seed" token: a channel message's machine metadata, encrypted
//! with the group key and base64-encoded. This lets a save-sync message show a
//! friendly human line instead of raw JSON, while still carrying everything the
//! app needs — and only group members (who hold the key) can read it back.

use base64::Engine;

use crate::{seal, CoreError};

/// Label that precedes the token on its own line in a channel message.
pub const SEED_PREFIX: &str = "Seed:";

/// Encrypt `plaintext` with `key` and base64-encode it into a Seed token.
pub fn seal_to_token(key: &[u8; 32], plaintext: &[u8]) -> Result<String, CoreError> {
    let blob = seal::seal(key, plaintext)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(blob))
}

/// Inverse of [`seal_to_token`]. Returns `None` if `token` is not valid base64
/// or was not sealed with `key` (so unrelated messages decode to `None`).
pub fn open_from_token(key: &[u8; 32], token: &str) -> Option<Vec<u8>> {
    let blob = base64::engine::general_purpose::STANDARD
        .decode(token.trim())
        .ok()?;
    seal::open(key, &blob).ok()
}

/// Pull the Seed token out of a channel message (the text after `Seed:` on its
/// own line), or `None` if the message has no Seed line.
pub fn seed_from_message(content: &str) -> Option<&str> {
    content
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(SEED_PREFIX).map(str::trim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_with_the_right_key() {
        let key = [7u8; 32];
        let token = seal_to_token(&key, b"hello metadata").unwrap();
        assert_eq!(open_from_token(&key, &token).unwrap(), b"hello metadata");
    }

    #[test]
    fn wrong_key_or_garbage_decodes_to_none() {
        let token = seal_to_token(&[1u8; 32], b"secret").unwrap();
        assert!(open_from_token(&[2u8; 32], &token).is_none());
        assert!(open_from_token(&[1u8; 32], "not base64!!").is_none());
    }

    #[test]
    fn seed_is_extracted_from_a_friendly_message() {
        let msg = "Milana sincronizou: Valheim\n\nSeed: AAAA";
        assert_eq!(seed_from_message(msg), Some("AAAA"));
        assert!(seed_from_message("just a normal chat line").is_none());
    }
}
