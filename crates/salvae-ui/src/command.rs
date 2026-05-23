//! Messages between the UI thread and the background worker.

use crate::view::{ActivityView, DiscoveredCandidate, GameView, GroupView, VersionView};

/// A request from the UI thread to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Re-read groups + installed games.
    Refresh,
    CreateGroup {
        name: String,
        password: String,
        token: String,
        guild_id: u64,
        channel_id: u64,
    },
    JoinGroup {
        password: String,
        invite: String,
    },
    RemoveGroup {
        group_id: String,
    },
    SetGamePath {
        group_id: String,
        game_id: String,
        folder: String,
    },
    /// Capture the "before" snapshot for auto-discovery.
    ArmScan {
        game_id: String,
    },
    /// Capture the "after" snapshot, diff, and rank candidates.
    CollectScan {
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
    /// A freshly created group's shareable invite blob.
    Invite(String),
    History {
        game_id: String,
        versions: Vec<VersionView>,
    },
    ScanArmed {
        game_id: String,
    },
    ScanResults {
        game_id: String,
        candidates: Vec<DiscoveredCandidate>,
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
