//! End-to-end vault behavior over the in-memory channel: the full lifecycle a
//! co-op save goes through (push, list, download a specific version, prune).

use salvae_core::kdf;
use salvae_vault::memory::InMemoryChannel;
use salvae_vault::vault::Vault;
use salvae_vault::VaultError;

#[test]
fn coop_save_lifecycle_over_in_memory_channel() {
    // Group key derived from the shared password.
    let salt = kdf::generate_salt();
    let key = kdf::derive_key("group-password", &salt).unwrap();

    let channel = InMemoryChannel::new();
    let vault = Vault::new(channel, key);

    // Three play sessions of the same co-op game, each producing a new save.
    let s1 = b"valheim world: day 1".to_vec();
    let s2 = b"valheim world: day 2, built a base".to_vec();
    let s3 = b"valheim world: day 3, killed first boss".to_vec();

    let v1 = vault
        .push_version("valheim", "valheim", &s1, "Gabriel", "pc-gabriel", 1_000, 5)
        .unwrap();
    let v2 = vault
        .push_version("valheim", "valheim", &s2, "Ana", "pc-ana", 2_000, 5)
        .unwrap();
    let v3 = vault
        .push_version("valheim", "valheim", &s3, "Gabriel", "pc-gabriel", 3_000, 5)
        .unwrap();
    assert_eq!((v1.number, v2.number, v3.number), (1, 2, 3));

    // Another player pulls the latest before hosting.
    let latest = vault.latest_version("valheim").unwrap().unwrap();
    assert_eq!(latest.number, 3);
    assert_eq!(vault.download("valheim", latest.number).unwrap(), s3);

    // History is browsable and each version restores its exact bytes.
    assert_eq!(vault.list_versions("valheim").unwrap().len(), 3);
    assert_eq!(vault.download("valheim", 1).unwrap(), s1);
    assert_eq!(vault.download("valheim", 2).unwrap(), s2);
}

#[test]
fn pruning_bounds_history_and_keeps_newest() {
    let key = kdf::derive_key("pw", &kdf::generate_salt()).unwrap();
    let vault = Vault::new(InMemoryChannel::new(), key);

    for day in 1..=6u64 {
        let save = format!("save for day {day}").into_bytes();
        vault
            .push_version("terraria", "terraria", &save, "p", "d", day, 3)
            .unwrap();
    }

    let versions = vault.list_versions("terraria").unwrap();
    assert_eq!(
        versions.iter().map(|v| v.number).collect::<Vec<_>>(),
        vec![4, 5, 6]
    );
    assert!(matches!(
        vault.download("terraria", 1),
        Err(VaultError::NotFound)
    ));
    assert_eq!(
        vault.download("terraria", 6).unwrap(),
        b"save for day 6".to_vec()
    );
}

#[test]
fn wrong_group_password_cannot_open_saves() {
    let salt = kdf::generate_salt();
    let right = kdf::derive_key("correct-horse", &salt).unwrap();
    let wrong = kdf::derive_key("wrong-horse", &salt).unwrap();

    let channel = InMemoryChannel::new();
    Vault::new(&channel, right)
        .push_version("valheim", "valheim", b"top secret save", "a", "d", 1, 5)
        .unwrap();

    let intruder = Vault::new(&channel, wrong);
    assert!(intruder.download("valheim", 1).is_err());
}
