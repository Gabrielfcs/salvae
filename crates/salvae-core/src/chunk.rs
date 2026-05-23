//! Split byte blobs into size-bounded ordered pieces (for Discord attachment
//! limits) and rejoin them losslessly.

use crate::CoreError;

/// Split `data` into ordered chunks of at most `max_size` bytes.
/// Empty input yields a single empty chunk (so there is always >= 1 piece).
pub fn split(data: &[u8], max_size: usize) -> Result<Vec<Vec<u8>>, CoreError> {
    if max_size == 0 {
        return Err(CoreError::Chunk("max_size must be greater than 0".into()));
    }
    if data.is_empty() {
        return Ok(vec![Vec::new()]);
    }
    Ok(data.chunks(max_size).map(|c| c.to_vec()).collect())
}

/// Reassemble ordered chunks (as produced by [`split`]) into one byte vector.
pub fn join(chunks: &[Vec<u8>]) -> Vec<u8> {
    chunks.iter().flat_map(|c| c.iter().copied()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_into_bounded_chunks_and_rejoins() {
        let data: Vec<u8> = (0..=255u8).collect(); // 256 bytes
        let chunks = split(&data, 100).unwrap();
        assert_eq!(chunks.len(), 3); // 100 + 100 + 56
        assert_eq!(chunks[0].len(), 100);
        assert_eq!(chunks[2].len(), 56);
        assert_eq!(join(&chunks), data);
    }

    #[test]
    fn data_smaller_than_chunk_is_single_chunk() {
        let data = vec![1, 2, 3];
        let chunks = split(&data, 100).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(join(&chunks), data);
    }

    #[test]
    fn empty_data_produces_one_empty_chunk() {
        let chunks = split(&[], 100).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_empty());
        assert_eq!(join(&chunks), Vec::<u8>::new());
    }

    #[test]
    fn zero_max_size_is_rejected() {
        assert!(matches!(split(&[1, 2, 3], 0), Err(CoreError::Chunk(_))));
    }
}
