//! End-to-end co-op session over the in-memory channel: owner hosts and pushes,
//! a friend pulls the latest, plays and pushes, then the owner hits a conflict
//! and resolves it — plus the presence marker.

use std::path::Path;

use salvae_sync::engine::{PullOutcome, PushOutcome, Resolution, SyncEngine};
use salvae_vault::memory::InMemoryChannel;

fn write(dir: &Path, rel: &str, content: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn coop_session_round_trip_and_conflict() {
    let channel = InMemoryChannel::new();
    let key = [7u8; 32];

    // --- Owner hosts day 1 and pushes. ---
    let owner_backups = tempfile::tempdir().unwrap();
    let owner_save = tempfile::tempdir().unwrap();
    write(owner_save.path(), "world.db", b"day 1 progress");
    let mut owner = SyncEngine::new(
        &channel,
        key,
        "Gabriel",
        "dev-gabriel",
        5,
        owner_backups.path(),
    );
    assert!(matches!(
        owner.push("valheim", owner_save.path(), 1_000).unwrap(),
        PushOutcome::Pushed(v) if v.number == 1
    ));

    // --- Friend pulls the latest before hosting. ---
    let friend_backups = tempfile::tempdir().unwrap();
    let friend_save = tempfile::tempdir().unwrap();
    let mut friend = SyncEngine::new(&channel, key, "Ana", "dev-ana", 5, friend_backups.path());
    assert!(matches!(
        friend.pull("valheim", friend_save.path(), 2_000).unwrap(),
        PullOutcome::Applied(v) if v.number == 1
    ));
    assert_eq!(
        std::fs::read(friend_save.path().join("world.db")).unwrap(),
        b"day 1 progress"
    );

    // Friend plays (marker), advances the save, pushes v2, stops.
    friend.begin_playing("valheim", 2_000).unwrap();
    assert_eq!(owner.who_is_playing("valheim", 2_000).unwrap().len(), 1);
    write(friend_save.path(), "world.db", b"day 2 progress");
    assert!(matches!(
        friend.push("valheim", friend_save.path(), 2_500).unwrap(),
        PushOutcome::Pushed(v) if v.number == 2
    ));
    friend.end_playing("valheim").unwrap();
    assert!(owner.who_is_playing("valheim", 2_600).unwrap().is_empty());

    // --- Owner (still on v1) edits and tries to push -> CONFLICT. ---
    write(owner_save.path(), "world.db", b"owner diverged");
    let conflict = owner.push("valheim", owner_save.path(), 3_000).unwrap();
    assert!(matches!(conflict, PushOutcome::Conflict { remote } if remote.number == 2));

    // Owner takes the remote: local now holds the friend's day-2 save.
    let resolved = owner
        .resolve("valheim", owner_save.path(), Resolution::TakeRemote, 3_100)
        .unwrap();
    assert!(matches!(resolved, PushOutcome::NoChange(2)));
    assert_eq!(
        std::fs::read(owner_save.path().join("world.db")).unwrap(),
        b"day 2 progress"
    );
}
