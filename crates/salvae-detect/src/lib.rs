//! Salvaê detection (catalog/discovery): enumerate installed games and locate
//! their save folders. Pure logic + filesystem — no live OS event machinery.

pub mod candidate;
pub mod epic;
pub mod game;
pub mod roots;
pub mod snapshot;
pub mod steam;
pub mod vdf;

/// Errors produced by detection.
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    /// A launcher manifest could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),
    /// Filesystem error scanning launchers or snapshotting folders.
    #[error("io error: {0}")]
    Io(String),
}

#[cfg(test)]
mod error_tests {
    use super::DetectError;

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(DetectError::Parse("bad".into()).to_string(), "parse error: bad");
        assert_eq!(DetectError::Io("disk".into()).to_string(), "io error: disk");
    }
}
