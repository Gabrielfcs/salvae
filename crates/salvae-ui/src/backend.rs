//! The operations the worker needs, abstracted so the worker's dispatch logic
//! is testable with a fake (the real impl is `AgentBackend`, Task 7).

use crate::command::Event;
use crate::view::{ChannelView, DiscoveredCandidate, GameView, GroupView, GuildView, VersionView};

/// Everything the background worker can ask of the backend. All fallible calls
/// return `Result<_, String>` — the error string is shown verbatim in the UI.
pub trait Backend {
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
    fn set_game_path(&mut self, group_id: &str, game_id: &str, folder: &str) -> Result<(), String>;

    /// Capture the "before" snapshot for `game_id`.
    fn arm_scan(&mut self, game_id: &str) -> Result<(), String>;
    /// Diff + rank candidates for a previously armed `game_id`.
    fn collect_scan(&mut self, game_id: &str) -> Result<Vec<DiscoveredCandidate>, String>;

    fn history(&mut self, game_id: &str) -> Result<Vec<VersionView>, String>;
    fn restore(&mut self, game_id: &str, version: u64) -> Result<(), String>;
    fn resolve(&mut self, game_id: &str, take_remote: bool) -> Result<(), String>;

    /// Poll the watch/sync loop once; return any activity/conflict events.
    fn tick(&mut self) -> Vec<Event>;
}
