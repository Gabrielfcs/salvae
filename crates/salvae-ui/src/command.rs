//! Messages between the UI thread and the background worker.

use crate::view::{ActivityView, ChannelView, GameView, GroupView, GuildView, VersionView};

/// A request from the UI thread to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Re-read groups + installed games.
    Refresh,
    /// Set the user's display name (author of their saves).
    SetName {
        name: String,
    },
    CreateGroup {
        name: String,
        password: String,
        token: String,
        guild_id: u64,
        channel_id: u64,
    },
    /// Validate a bot token (and fetch the bot's identity) for the wizard.
    ValidateToken {
        token: String,
    },
    /// List the servers the bot token can see (create-group picker).
    FetchGuilds {
        token: String,
    },
    /// List a server's text channels (create-group picker).
    FetchChannels {
        token: String,
        guild_id: u64,
    },
    JoinGroup {
        password: String,
        invite: String,
    },
    RemoveGroup {
        group_id: String,
    },
    /// Rebuild a group's invite and copy it to the clipboard.
    CopyInvite {
        group_id: String,
    },
    /// Re-post a group's invite into its Discord channel via the bot.
    ResendInvite {
        group_id: String,
    },
    /// Replace a group's bot token (after resetting it in the portal).
    SetGroupToken {
        group_id: String,
        token: String,
    },
    SetGamePath {
        group_id: String,
        game_id: String,
        folder: String,
    },
    /// Turn on sync for a game: auto-resolve its save folder and store it.
    EnableSync {
        group_id: String,
        game_id: String,
    },
    /// Turn off sync for a game: forget its save folder.
    DisableSync {
        group_id: String,
        game_id: String,
    },
    LoadHistory {
        game_id: String,
    },
    Restore {
        game_id: String,
        version: u64,
    },
    Resolve {
        game_id: String,
        take_remote: bool,
    },
    /// Stop the worker loop.
    Shutdown,
}

/// A result/notification from the worker to the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Groups(Vec<GroupView>),
    InstalledGames(Vec<GameView>),
    /// The bot token validated; carries the bot's id (OAuth2 client id) + name.
    TokenValidated {
        bot_id: u64,
        bot_name: String,
    },
    /// Servers the bot can see (response to `FetchGuilds`).
    DiscoveredGuilds(Vec<GuildView>),
    /// Text channels of the selected server (response to `FetchChannels`).
    DiscoveredChannels(Vec<ChannelView>),
    /// A freshly created group's shareable invite blob.
    Invite(String),
    /// A group's invite the UI should copy to the clipboard.
    InviteToCopy(String),
    History {
        game_id: String,
        versions: Vec<VersionView>,
    },
    /// Sync was enabled but no save folder could be auto-resolved — the UI
    /// should prompt the user to pick it manually.
    SyncUnresolved {
        game_id: String,
    },
    /// A close produced a conflict; the UI must prompt for resolution.
    Conflict {
        game_id: String,
        remote: VersionView,
    },
    /// A previously pending conflict has been resolved.
    ResolvedConflict {
        game_id: String,
    },
    Activity(ActivityView),
    Error(String),
}
