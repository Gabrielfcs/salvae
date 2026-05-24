//! Proves the core promise: a save published to the channel by one member is
//! pulled by another member who was NOT online at the time — there is no P2P or
//! shared server, the encrypted save lives in the channel, so the uploader can
//! be long gone (game closed, PC off) and the sync still happens whenever the
//! other member's app next runs.

use std::path::Path;

use salvae_sync::engine::{PullOutcome, PushOutcome, SyncEngine};
use salvae_vault::channel::Channel;
use salvae_vault::memory::InMemoryChannel;

fn write(dir: &Path, rel: &str, content: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn an_offline_members_published_save_is_synced_later_by_another() {
    // One Discord channel + one group key shared by both members (from the
    // invite). The channel is the only thing they have in common — A and B are
    // never "online" at the same moment in this test.
    let channel = InMemoryChannel::new();
    let key = [7u8; 32];

    // ---- A plays, pushes v1, then "shuts the PC down" (we stop using A). ----
    let a_backups = tempfile::tempdir().unwrap();
    let a_save = tempfile::tempdir().unwrap();
    write(a_save.path(), "world.db", b"A: day 1");
    {
        let mut a = SyncEngine::new(&channel, key, "Ana", "dev-ana", 5, a_backups.path());
        assert!(matches!(
            a.push("steam:1", "Stardew", a_save.path(), 1_000).unwrap(),
            PushOutcome::Pushed(v) if v.number == 1
        ));
    } // `a` dropped here — A is gone.

    // The save is now sitting in the channel, encrypted, in the friendly format
    // (not raw JSON), independent of A being online.
    let stored = &channel.list_messages(None, 100).unwrap()[0].content;
    assert!(
        stored.contains("Seed:"),
        "stored message must carry a Seed token"
    );
    assert!(
        !stored.contains("salvae-save-v1"),
        "raw marker/JSON must not be visible in the channel"
    );
    assert!(stored.contains("Stardew"), "friendly line names the game");

    // ---- B connects only now, having never synced this game. ----
    let b_backups = tempfile::tempdir().unwrap();
    let b_save = tempfile::tempdir().unwrap();
    let mut b = SyncEngine::new(&channel, key, "Bob", "dev-bob", 5, b_backups.path());

    // B has no local save yet and version state is empty -> the pull applies A's
    // published save even though A is offline.
    let outcome = b.pull("steam:1", b_save.path(), 5_000).unwrap();
    assert!(matches!(outcome, PullOutcome::Applied(v) if v.number == 1));
    assert_eq!(
        std::fs::read(b_save.path().join("world.db")).unwrap(),
        b"A: day 1"
    );

    // A second pull is a no-op: B is now up to date, so it won't re-download.
    assert_eq!(
        b.pull("steam:1", b_save.path(), 5_100).unwrap(),
        PullOutcome::AlreadyUpToDate(1)
    );

    // ---- Later, A returns, plays again and publishes v2, then leaves again. ----
    write(a_save.path(), "world.db", b"A: day 2");
    {
        let mut a = SyncEngine::new(&channel, key, "Ana", "dev-ana", 5, a_backups.path());
        // A is in sync with v1 (its state file would say so on a real machine);
        // here we pull first to mirror "open game -> pull -> play -> close -> push".
        a.pull("steam:1", a_save.path(), 6_000).unwrap();
        write(a_save.path(), "world.db", b"A: day 2");
        assert!(matches!(
            a.push("steam:1", "Stardew", a_save.path(), 6_100).unwrap(),
            PushOutcome::Pushed(v) if v.number == 2
        ));
    }

    // B, still un-synced for v2, pulls the newer published save on its next poll.
    let outcome = b.pull("steam:1", b_save.path(), 7_000).unwrap();
    assert!(matches!(outcome, PullOutcome::Applied(v) if v.number == 2));
    assert_eq!(
        std::fs::read(b_save.path().join("world.db")).unwrap(),
        b"A: day 2"
    );
}
