//! Salvaê process watcher + detector: poll running processes and emit
//! game open/close events for known installed games.

pub mod detector;
pub mod process;

#[cfg(windows)]
pub mod system;

/// Errors produced by the watcher.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The OS process lister failed.
    #[error("process lister error: {0}")]
    Lister(String),
}
