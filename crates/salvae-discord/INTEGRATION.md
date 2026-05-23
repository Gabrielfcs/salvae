# Salvaê — Discord transport integration checklist

This crate talks to a real Discord channel. CI runs only mocked tests; use this
checklist once to verify against real Discord.

## One-time bot setup (group owner)
1. Go to https://discord.com/developers/applications → New Application.
2. Bot tab → Add Bot → copy the **bot token** (keep it secret).
3. Under Bot → Privileged Gateway Intents: none are required for REST use.
4. OAuth2 → URL Generator → scopes: `bot`; bot permissions: `View Channels`,
   `Send Messages`, `Manage Messages` (for pruning), `Read Message History`,
   `Attach Files`. Open the generated URL and invite the bot to your private
   server.
5. Create a private `#saves` channel; ensure the bot role can see and post in it.
6. Enable Developer Mode in Discord (Settings → Advanced) and copy the
   **channel id** (right-click the channel → Copy Channel ID).

## Run the live smoke test
```powershell
$env:SALVAE_TEST_TOKEN  = "<bot token>"
$env:SALVAE_TEST_CHANNEL = "<channel id>"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo test -p salvae-discord --test live_discord -- --ignored --nocapture
```
Expected: prints `live round-trip OK for game_id=livetest-...` and passes. It
uses a random game id per run, so it does not disturb real saves; the two test
versions remain in the channel (delete the two `livetest-*` messages by hand if
you want a clean channel).

## Notes / limits
- Non-boosted servers cap attachments at 10 MiB; the vault chunks at 8 MiB.
- Discord CDN attachment URLs expire (~24h); downloads always re-fetch the
  message first to get a fresh URL.
- The bot token grants channel access; save confidentiality comes from the
  group password (client-side AES-256-GCM), not the token.
