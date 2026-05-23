//! End-to-end: the exact pipeline a save file travels through in production.

use salvae_core::{chunk, hash, kdf, seal, version::SaveVersion};

#[test]
fn full_save_pipeline_round_trips() {
    // 1. Derive the group key from a password + per-group salt.
    let salt = kdf::generate_salt();
    let key = kdf::derive_key("group-password", &salt).unwrap();

    // 2. A pretend save file.
    let save: Vec<u8> = b"world=Midgard;seed=42;players=4;".repeat(500);
    let original_hash = hash::content_hash(&save);

    // 3. Seal it (compress + encrypt), then chunk for an attachment limit.
    let blob = seal::seal(&key, &save).unwrap();
    let chunks = chunk::split(&blob, 8 * 1024).unwrap();

    // 4. Build the version metadata that would be stored alongside it.
    let meta = SaveVersion {
        number: 1,
        content_hash: original_hash.clone(),
        created_at_ms: 1_716_400_000_000,
        author: "Gabriel".into(),
        device_id: "pc-gabriel".into(),
        size_bytes: save.len() as u64,
        chunk_count: chunks.len() as u32,
    };

    // 5. Download side: rejoin chunks, open, verify integrity against metadata.
    let rejoined = chunk::join(&chunks);
    let recovered = seal::open(&key, &rejoined).unwrap();
    assert_eq!(recovered, save);
    assert_eq!(hash::content_hash(&recovered), meta.content_hash);
    assert_eq!(recovered.len() as u64, meta.size_bytes);
}

#[test]
fn wrong_password_cannot_open_pipeline() {
    let salt = kdf::generate_salt();
    let key = kdf::derive_key("right-password", &salt).unwrap();
    let wrong = kdf::derive_key("wrong-password", &salt).unwrap();

    let blob = seal::seal(&key, b"my precious save").unwrap();
    assert!(seal::open(&wrong, &blob).is_err());
}
