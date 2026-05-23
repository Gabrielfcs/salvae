//! Encrypted, shareable invite blob (encode/decode).
//!
//! Layout (before base64): `salt (16 bytes, cleartext) || cipher::encrypt(key, payload)`
//! where `key = Argon2id(password, salt)` and `payload` is JSON of the group's
//! connection info. The salt is cleartext so a joiner can derive the key from
//! their typed password; a wrong password makes AES-GCM decryption fail.

use base64::Engine;
use serde::{Deserialize, Serialize};

use salvae_core::cipher;
use salvae_core::kdf::{self, KEY_LEN, SALT_LEN};

use crate::ConfigError;

#[derive(Serialize, Deserialize)]
struct InvitePayload {
    name: String,
    token: String,
    guild_id: u64,
    channel_id: u64,
}

/// The group info recovered from a decoded invite, plus the derived key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInvite {
    pub name: String,
    pub token: String,
    pub guild_id: u64,
    pub channel_id: u64,
    pub salt: [u8; SALT_LEN],
    pub key: [u8; KEY_LEN],
}

/// Build a shareable invite string for a group with the given `salt` (which
/// must match the salt the group key was derived from).
pub fn encode_invite(
    password: &str,
    salt: &[u8; SALT_LEN],
    name: &str,
    token: &str,
    guild_id: u64,
    channel_id: u64,
) -> Result<String, ConfigError> {
    let key = kdf::derive_key(password, salt)?;
    let payload = InvitePayload {
        name: name.to_string(),
        token: token.to_string(),
        guild_id,
        channel_id,
    };
    let payload_bytes =
        serde_json::to_vec(&payload).map_err(|e| ConfigError::Serde(e.to_string()))?;
    let ciphertext = cipher::encrypt(&key, &payload_bytes)?;

    let mut blob = Vec::with_capacity(SALT_LEN + ciphertext.len());
    blob.extend_from_slice(salt);
    blob.extend_from_slice(&ciphertext);
    Ok(base64::engine::general_purpose::STANDARD.encode(&blob))
}

/// Decode an invite string with the group `password`, recovering the group
/// info and the derived key.
pub fn decode_invite(password: &str, invite: &str) -> Result<DecodedInvite, ConfigError> {
    let blob = base64::engine::general_purpose::STANDARD
        .decode(invite.trim())
        .map_err(|e| ConfigError::Invite(format!("bad base64: {e}")))?;
    if blob.len() <= SALT_LEN {
        return Err(ConfigError::Invite("invite too short".into()));
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&blob[..SALT_LEN]);

    let key = kdf::derive_key(password, &salt)?;
    let payload_bytes =
        cipher::decrypt(&key, &blob[SALT_LEN..]).map_err(|_| ConfigError::WrongPassword)?;
    let payload: InvitePayload =
        serde_json::from_slice(&payload_bytes).map_err(|e| ConfigError::Serde(e.to_string()))?;

    Ok(DecodedInvite {
        name: payload.name,
        token: payload.token,
        guild_id: payload.guild_id,
        channel_id: payload.channel_id,
        salt,
        key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_recovers_info_and_key() {
        let salt = kdf::generate_salt();
        let invite = encode_invite("group-pw", &salt, "Crew", "bot-token", 111, 222).unwrap();
        let decoded = decode_invite("group-pw", &invite).unwrap();
        assert_eq!(decoded.name, "Crew");
        assert_eq!(decoded.token, "bot-token");
        assert_eq!(decoded.guild_id, 111);
        assert_eq!(decoded.channel_id, 222);
        assert_eq!(decoded.salt, salt);
        // The decoded key equals deriving from the same password + salt.
        assert_eq!(decoded.key, kdf::derive_key("group-pw", &salt).unwrap());
    }

    #[test]
    fn wrong_password_fails() {
        let salt = kdf::generate_salt();
        let invite = encode_invite("right", &salt, "Crew", "tok", 1, 2).unwrap();
        assert!(matches!(
            decode_invite("wrong", &invite),
            Err(ConfigError::WrongPassword)
        ));
    }

    #[test]
    fn garbage_invite_is_invalid() {
        assert!(matches!(
            decode_invite("pw", "not base64!!!"),
            Err(ConfigError::Invite(_))
        ));
        // Valid base64 but too short to contain a salt.
        let short = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        assert!(matches!(
            decode_invite("pw", &short),
            Err(ConfigError::Invite(_))
        ));
    }
}
