//! UI-facing data types: plain data the egui layer renders, decoupled from the
//! backend's domain types.

use std::path::PathBuf;

/// A group as shown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub games: Vec<GameMapping>,
}

/// A configured (game id -> local folder) mapping inside a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMapping {
    pub game_id: String,
    pub folder: String,
}

/// An installed game discovered on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameView {
    pub id: String,
    pub name: String,
}

/// One stored save version, formatted for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionView {
    pub number: u64,
    pub author: String,
    pub size: u64,
    pub created_at_ms: u64,
}

/// A pending conflict awaiting the user's resolution choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub game_id: String,
    pub remote: VersionView,
}

/// A ranked save-folder candidate from auto-discovery (absolute path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCandidate {
    pub folder: PathBuf,
    pub changed_files: usize,
    pub score: i64,
}

/// Severity of an activity-log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Info,
    Warning,
    Error,
}

/// One line in the activity log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityView {
    pub kind: ActivityKind,
    pub message: String,
}

impl ActivityView {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: ActivityKind::Info,
            message: message.into(),
        }
    }
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            kind: ActivityKind::Warning,
            message: message.into(),
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: ActivityKind::Error,
            message: message.into(),
        }
    }
}

/// Human-readable byte size (e.g. `1.5 MB`).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_scales_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn activity_constructors_set_kind() {
        assert_eq!(ActivityView::warning("x").kind, ActivityKind::Warning);
        assert_eq!(ActivityView::error("y").kind, ActivityKind::Error);
    }
}
