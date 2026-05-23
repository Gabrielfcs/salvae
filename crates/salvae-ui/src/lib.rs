//! Salvaê desktop UI: tray app, on-demand window, and the background worker
//! that drives the sync agent.

pub mod backend;
pub mod command;
pub mod discovery;
pub mod view;
pub mod viewmodel;
pub mod worker;

#[cfg(not(test))]
pub mod agent_backend;

// Enabled in later tasks (they pull in winit/tray machinery):
// #[cfg(not(test))]
// pub mod app;
// #[cfg(not(test))]
// pub mod tray;

/// Errors surfaced by the UI layer.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    /// A backend (config/sync/agent) operation failed.
    #[error("{0}")]
    Backend(String),
}
