//! What the agent did in response to a game event.

use salvae_sync::engine::{PullOutcome, PushOutcome};

/// The result of handling a single game open/close event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOutcome {
    /// Game opened: the pull result, plus the display names of other members
    /// currently playing this game (a warning to surface).
    Opened {
        pull: PullOutcome,
        others_playing: Vec<String>,
    },
    /// Game closed: the push result (which may be a `Conflict` for the UI).
    Closed { push: PushOutcome },
    /// The game is not configured in any group (detected but intentionally ignored).
    NotConfigured,
    /// The configured save folder does not exist, so there was nothing to push.
    NoFolder,
}
