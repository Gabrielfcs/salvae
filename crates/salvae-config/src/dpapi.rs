//! Windows DPAPI protect/unprotect and a file-backed SecretStore.
//!
//! `protect`/`unprotect` wrap `CryptProtectData`/`CryptUnprotectData`, encrypting
//! data so only the current Windows user (on this machine) can read it.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use crate::ConfigError;

/// Build a DATA_BLOB pointing at `data` (the API does not mutate the input).
fn in_blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
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
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
    // SAFETY: pointers are valid for the call; output blob is consumed below.
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),     // description
            ptr::null(),     // optional entropy
            ptr::null(),     // reserved
            ptr::null(),     // prompt struct
            0,               // flags
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
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
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
}
