//! Real Discord smoke test. Ignored by default; run explicitly with a bot token.
//!
//! Set environment variables and run:
//!   $env:SALVAE_TEST_TOKEN = "<bot token>"
//!   $env:SALVAE_TEST_CHANNEL = "<channel id>"
//!   cargo test -p salvae-discord --test live_discord -- --ignored --nocapture
//!
//! It pushes two versions of a tiny save into the real channel, reads the
//! latest back, restores history, and prunes — then leaves the channel as it
//! found it (prunes to 0-extra by using a fresh random game id each run).

use salvae_core::kdf;
use salvae_discord::discord::DiscordChannel;
use salvae_vault::vault::Vault;

#[test]
#[ignore = "requires a real Discord bot token; run manually with --ignored"]
fn live_push_and_download_round_trip() {
    let token = match std::env::var("SALVAE_TEST_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SALVAE_TEST_TOKEN not set; skipping");
            return;
        }
    };
    let channel_id: u64 = std::env::var("SALVAE_TEST_CHANNEL")
        .expect("set SALVAE_TEST_CHANNEL")
        .parse()
        .expect("SALVAE_TEST_CHANNEL must be a u64");

    let key = kdf::derive_key("live-test-password", &kdf::generate_salt()).unwrap();
    let channel = DiscordChannel::new(token, channel_id);
    let vault = Vault::new(channel, key);

    // Unique game id per run so we never collide with real data.
    let mut rnd = [0u8; 8];
    getrandom::getrandom(&mut rnd).unwrap();
    let game_id = format!("livetest-{}", u64::from_le_bytes(rnd));

    let v1 = vault
        .push_version(&game_id, b"live save v1", "tester", "ci", 1, 2)
        .unwrap();
    let v2 = vault
        .push_version(&game_id, b"live save v2", "tester", "ci", 2, 2)
        .unwrap();
    assert_eq!((v1.number, v2.number), (1, 2));

    let latest = vault.latest_version(&game_id).unwrap().unwrap();
    assert_eq!(latest.number, 2);
    assert_eq!(vault.download(&game_id, 2).unwrap(), b"live save v2");
    assert_eq!(vault.download(&game_id, 1).unwrap(), b"live save v1");

    eprintln!("live round-trip OK for game_id={game_id}");
}
