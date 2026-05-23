//! Salvaê agent: polls for game open/close and drives save sync per group.

pub mod agent;
pub mod outcome;

/// Errors from the agent's per-event handling.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// A sync (storage/transport/crypto/io) error.
    #[error("sync error: {0}")]
    Sync(#[from] salvae_sync::SyncError),
    /// A process-watcher error.
    #[error("watch error: {0}")]
    Watch(#[from] salvae_watch::WatchError),
}

#[cfg(test)]
mod error_tests {
    use super::AgentError;

    #[test]
    fn sync_errors_convert() {
        let e: AgentError = salvae_sync::SyncError::Pack("x".into()).into();
        assert!(matches!(e, AgentError::Sync(_)));
    }

    #[test]
    fn watch_errors_convert() {
        let e: AgentError = salvae_watch::WatchError::Lister("x".into()).into();
        assert!(matches!(e, AgentError::Watch(_)));
    }
}
