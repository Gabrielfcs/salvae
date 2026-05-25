//! The operations the worker needs, abstracted so the worker's dispatch logic
//! is testable with a fake (the real impl is `AgentBackend`, Task 7).

use crate::command::Event;
use crate::view::{ChannelView, GameView, GroupView, GuildView, VersionView};

/// Everything the background worker can ask of the backend. All fallible calls
/// return `Result<_, String>` — the error string is shown verbatim in the UI.
pub trait Backend {
    /// The user's display name (author of their saves); empty until set.
    fn display_name(&self) -> String;
    /// Set the user's display name (used as the save author).
    fn set_display_name(&mut self, name: &str) -> Result<(), String>;

    /// Current groups (with their configured game→folder mappings).
    fn refresh_groups(&self) -> Vec<GroupView>;
    /// Games discovered on this machine.
    fn installed_games(&self) -> Vec<GameView>;

    /// Validate `token` and return the bot's id (OAuth2 client id) + name.
    fn validate_token(&self, token: &str) -> Result<(u64, String), String>;
    /// List the servers the bot `token` can see (create-group picker).
    fn fetch_guilds(&self, token: &str) -> Result<Vec<GuildView>, String>;
    /// List a server's text channels (create-group picker).
    fn fetch_channels(&self, token: &str, guild_id: u64) -> Result<Vec<ChannelView>, String>;

    fn create_group(
        &mut self,
        name: &str,
        password: &str,
        token: &str,
        guild_id: u64,
        channel_id: u64,
    ) -> Result<String, String>;
    fn join_group(&mut self, password: &str, invite: &str) -> Result<(), String>;
    fn remove_group(&mut self, group_id: &str) -> Result<(), String>;
    /// Rebuild a group's shareable invite string (for copying).
    fn group_invite(&self, group_id: &str) -> Result<String, String>;
    /// Re-post a group's invite into its Discord channel via the bot.
    fn resend_invite(&self, group_id: &str) -> Result<(), String>;
    /// Replace a group's bot token (after the owner reset it in the portal).
    fn set_group_token(&mut self, group_id: &str, token: &str) -> Result<(), String>;
    fn set_game_path(&mut self, group_id: &str, game_id: &str, folder: &str) -> Result<(), String>;

    /// Enable sync for `game_id` in `group_id`: auto-resolve its save folder and
    /// store it. Returns the resolved folder, or `None` if it couldn't be found
    /// (the UI then asks for a manual pick).
    fn enable_sync(&mut self, group_id: &str, game_id: &str) -> Result<Option<String>, String>;
    /// Disable sync for `game_id` in `group_id` (forget its save folder).
    fn disable_sync(&mut self, group_id: &str, game_id: &str) -> Result<(), String>;

    fn history(&mut self, game_id: &str) -> Result<Vec<VersionView>, String>;
    fn restore(&mut self, game_id: &str, version: u64) -> Result<(), String>;
    fn resolve(&mut self, game_id: &str, take_remote: bool) -> Result<(), String>;

    /// Check GitHub for a newer release. Returns an `UpdateAvailable` event if
    /// one is newly found (no-op if a check already found one). Never errors —
    /// network failures are ignored and retried later.
    fn check_update(&mut self) -> Vec<Event>;

    /// Download and launch the pending app update (the installer then closes
    /// and reopens the app). Errors if no update is pending or the download
    /// fails.
    fn apply_update(&mut self) -> Result<(), String>;

    /// Poll the watch/sync loop once; return any activity/conflict events.
    fn tick(&mut self) -> Vec<Event>;
}
