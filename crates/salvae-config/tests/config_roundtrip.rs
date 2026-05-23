//! End-to-end: an owner creates a group, shares the invite, a friend joins,
//! and both end up with the SAME group key (so they can read each other's
//! encrypted saves) — exercised over the in-memory secret store + temp configs.

use salvae_config::secret::InMemorySecretStore;
use salvae_config::store::ConfigStore;

#[test]
fn owner_and_friend_share_the_same_group_key() {
    let dir = tempfile::tempdir().unwrap();

    // Owner creates the group.
    let mut owner =
        ConfigStore::load_or_default(dir.path().join("owner.toml"), InMemorySecretStore::new())
            .unwrap();
    let (owner_group, invite) = owner
        .create_group("Valheim Crew", "shared-secret", "BOT.TOKEN.XYZ", 9001, 9002)
        .unwrap();
    let owner_secret = owner.group_secret(&owner_group.id).unwrap();

    // Friend joins with the same password.
    let mut friend =
        ConfigStore::load_or_default(dir.path().join("friend.toml"), InMemorySecretStore::new())
            .unwrap();
    let friend_group = friend.join_group("shared-secret", &invite).unwrap();
    let friend_secret = friend.group_secret(&friend_group.id).unwrap();

    // Same channel + token + KEY (the whole point: shared decryption key).
    assert_eq!(friend_group.guild_id, 9001);
    assert_eq!(friend_group.channel_id, 9002);
    assert_eq!(friend_secret.token, owner_secret.token);
    assert_eq!(friend_secret.key, owner_secret.key);

    // Local group ids are independent (random per install).
    assert_ne!(owner_group.id, friend_group.id);
}

#[test]
fn config_persists_across_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let device_id = {
        let mut store = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
        store.create_group("Crew", "pw", "tok", 1, 2).unwrap();
        store.device_id().to_string()
    };

    // A fresh store over the same config file keeps device id + the group.
    let reloaded = ConfigStore::load_or_default(&path, InMemorySecretStore::new()).unwrap();
    assert_eq!(reloaded.device_id(), device_id);
    assert_eq!(reloaded.groups().len(), 1);
    assert_eq!(reloaded.groups()[0].name, "Crew");
}
