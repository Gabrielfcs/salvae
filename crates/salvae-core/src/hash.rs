//! Deterministic content fingerprint (BLAKE3, hex-encoded) for change detection.

/// Hex-encoded BLAKE3 hash of `data` (64 lowercase hex chars).
pub fn content_hash(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_same_hash() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
    }

    #[test]
    fn different_content_different_hash() {
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
    }

    #[test]
    fn hash_is_64_hex_chars() {
        let h = content_hash(b"anything");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
