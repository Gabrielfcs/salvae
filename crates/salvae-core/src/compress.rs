//! zstd compression of arbitrary bytes.

use crate::CoreError;

/// Default zstd compression level (good speed/ratio balance for save files).
pub const DEFAULT_LEVEL: i32 = 3;

/// Compress `data` with zstd at the given level.
pub fn compress(data: &[u8], level: i32) -> Result<Vec<u8>, CoreError> {
    zstd::encode_all(data, level).map_err(|e| CoreError::Compress(e.to_string()))
}

/// Decompress zstd `data` produced by [`compress`].
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, CoreError> {
    zstd::decode_all(data).map_err(|e| CoreError::Decompress(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovers_data() {
        let data = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBB".repeat(50);
        let packed = compress(&data, DEFAULT_LEVEL).unwrap();
        let back = decompress(&packed).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn compressible_data_gets_smaller() {
        let data = vec![0u8; 10_000];
        let packed = compress(&data, DEFAULT_LEVEL).unwrap();
        assert!(packed.len() < data.len());
    }

    #[test]
    fn empty_round_trips() {
        let packed = compress(&[], DEFAULT_LEVEL).unwrap();
        assert_eq!(decompress(&packed).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn garbage_fails_to_decompress() {
        assert!(matches!(decompress(&[1, 2, 3, 4]), Err(CoreError::Decompress(_))));
    }
}
