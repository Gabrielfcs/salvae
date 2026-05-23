//! Windows DPAPI protect/unprotect and a file-backed SecretStore.
//!
//! `protect`/`unprotect` wrap `CryptProtectData`/`CryptUnprotectData`, encrypting
//! data so only the current Windows user (on this machine) can read it.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use crate::secret::{GroupSecret, SecretStore};
use crate::ConfigError;

/// Build a DATA_BLOB pointing at `data` (the API does not mutate the input).
fn in_blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

/// Copy an output DATA_BLOB into a Vec and free the OS-allocated buffer.
///
/// # Safety
/// `out` must be a blob populated by a successful CryptProtectData /
/// CryptUnprotectData call (its `pbData` was allocated by the OS).
unsafe fn take_out_blob(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    LocalFree(out.pbData as *mut c_void);
    bytes
}

/// Encrypt `data` with DPAPI (current user). Output is opaque ciphertext.
pub fn protect(data: &[u8]) -> Result<Vec<u8>, ConfigError> {
    let input = in_blob(data);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // SAFETY: pointers are valid for the call; output blob is consumed below.
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(), // description
            ptr::null(), // optional entropy
            ptr::null(), // reserved
            ptr::null(), // prompt struct
            0,           // flags
            &mut output,
        )
    };
    if ok == 0 {
        return Err(ConfigError::Secret("CryptProtectData failed".into()));
    }
    Ok(unsafe { take_out_blob(output) })
}

/// Decrypt data produced by [`protect`]. Fails if it was not protected by this
/// user/machine or is corrupted.
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, ConfigError> {
    let input = in_blob(data);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // SAFETY: pointers are valid for the call; output blob is consumed below.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(), // description out (unused)
            ptr::null(),     // optional entropy
            ptr::null(),     // reserved
            ptr::null(),     // prompt struct
            0,               // flags
            &mut output,
        )
    };
    if ok == 0 {
        return Err(ConfigError::Secret("CryptUnprotectData failed".into()));
    }
    Ok(unsafe { take_out_blob(output) })
}

/// Serializable form of a secret (key as bytes for JSON friendliness).
#[derive(Serialize, Deserialize)]
struct StoredSecret {
    token: String,
    key: Vec<u8>,
}

/// A `SecretStore` that persists all secrets to one DPAPI-protected file.
pub struct DpapiSecretStore {
    path: PathBuf,
}

impl DpapiSecretStore {
    /// Use `path` as the protected secrets file (created on first `set`).
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Read + decrypt the secrets map (empty if the file does not exist).
    fn load_map(&self) -> Result<HashMap<String, StoredSecret>, ConfigError> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let sealed = std::fs::read(&self.path).map_err(|e| ConfigError::Io(e.to_string()))?;
        let plain = unprotect(&sealed)?;
        serde_json::from_slice(&plain).map_err(|e| ConfigError::Serde(e.to_string()))
    }

    /// Encrypt + write the secrets map.
    fn save_map(&self, map: &HashMap<String, StoredSecret>) -> Result<(), ConfigError> {
        let plain = serde_json::to_vec(map).map_err(|e| ConfigError::Serde(e.to_string()))?;
        let sealed = protect(&plain)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io(e.to_string()))?;
        }
        std::fs::write(&self.path, sealed).map_err(|e| ConfigError::Io(e.to_string()))
    }
}

impl SecretStore for DpapiSecretStore {
    fn get(&self, group_id: &str) -> Result<Option<GroupSecret>, ConfigError> {
        let map = self.load_map()?;
        match map.get(group_id) {
            None => Ok(None),
            Some(s) => {
                let key: [u8; salvae_core::kdf::KEY_LEN] = s
                    .key
                    .clone()
                    .try_into()
                    .map_err(|_| ConfigError::Secret("stored key has wrong length".into()))?;
                Ok(Some(GroupSecret {
                    token: s.token.clone(),
                    key,
                }))
            }
        }
    }

    fn set(&mut self, group_id: &str, secret: GroupSecret) -> Result<(), ConfigError> {
        let mut map = self.load_map()?;
        map.insert(
            group_id.to_string(),
            StoredSecret {
                token: secret.token,
                key: secret.key.to_vec(),
            },
        );
        self.save_map(&map)
    }

    fn remove(&mut self, group_id: &str) -> Result<(), ConfigError> {
        let mut map = self.load_map()?;
        map.remove(group_id);
        self.save_map(&map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protect_then_unprotect_round_trips() {
        let secret = b"top-secret group key bytes";
        let sealed = protect(secret).unwrap();
        assert_ne!(sealed, secret); // it is actually transformed
        assert_eq!(unprotect(&sealed).unwrap(), secret);
    }

    #[test]
    fn unprotect_garbage_fails() {
        assert!(unprotect(&[0u8, 1, 2, 3, 4, 5]).is_err());
    }

    #[test]
    fn file_store_round_trips_secrets() {
        use crate::secret::{GroupSecret, SecretStore};
        use salvae_core::kdf::KEY_LEN;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.dat");

        let mut store = DpapiSecretStore::new(&path);
        assert_eq!(store.get("g1").unwrap(), None);
        store
            .set(
                "g1",
                GroupSecret {
                    token: "tok".into(),
                    key: [9u8; KEY_LEN],
                },
            )
            .unwrap();

        // A fresh store reading the same file sees the persisted secret.
        let reopened = DpapiSecretStore::new(&path);
        assert_eq!(
            reopened.get("g1").unwrap(),
            Some(GroupSecret {
                token: "tok".into(),
                key: [9u8; KEY_LEN]
            })
        );

        store.remove("g1").unwrap();
        assert_eq!(DpapiSecretStore::new(&path).get("g1").unwrap(), None);
    }

    #[test]
    fn file_store_is_dpapi_protected_on_disk() {
        use crate::secret::{GroupSecret, SecretStore};
        use salvae_core::kdf::KEY_LEN;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.dat");
        let mut store = DpapiSecretStore::new(&path);
        store
            .set(
                "g1",
                GroupSecret {
                    token: "plaintext-marker".into(),
                    key: [1u8; KEY_LEN],
                },
            )
            .unwrap();

        // The token must NOT appear verbatim in the on-disk file.
        let raw = std::fs::read(&path).unwrap();
        assert!(raw
            .windows(b"plaintext-marker".len())
            .all(|w| w != b"plaintext-marker"));
    }
}
