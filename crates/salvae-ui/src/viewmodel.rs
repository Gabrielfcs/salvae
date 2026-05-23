//! The render state the egui layer draws each frame, updated by applying
//! worker `Event`s. Pure data + `apply` — no I/O.

use std::collections::BTreeMap;

use crate::command::Event;
use crate::view::{ActivityView, Conflict, DiscoveredCandidate, GameView, GroupView, VersionView};

/// Maximum activity-log lines kept in memory.
const ACTIVITY_CAP: usize = 200;

/// Everything the UI renders.
#[derive(Debug, Default)]
pub struct ViewModel {
    pub groups: Vec<GroupView>,
    pub installed_games: Vec<GameView>,
    pub history: BTreeMap<String, Vec<VersionView>>,
    pub scan_armed: Vec<String>,
    pub scan_results: BTreeMap<String, Vec<DiscoveredCandidate>>,
    pub pending_conflicts: Vec<Conflict>,
    pub activity: Vec<ActivityView>,
    pub last_invite: Option<String>,
    pub last_error: Option<String>,
}

impl ViewModel {
    /// Fold one worker event into the render state.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Groups(g) => self.groups = g,
            Event::InstalledGames(g) => self.installed_games = g,
            Event::Invite(s) => {
                self.last_invite = Some(s);
                self.push_activity(ActivityView::info("Group created — share the invite below"));
            }
            Event::History { game_id, versions } => {
                self.history.insert(game_id, versions);
            }
            Event::ScanArmed { game_id } => {
                if !self.scan_armed.contains(&game_id) {
                    self.scan_armed.push(game_id);
                }
            }
            Event::ScanResults {
                game_id,
                candidates,
            } => {
                self.scan_armed.retain(|g| g != &game_id);
                self.scan_results.insert(game_id, candidates);
            }
            Event::Conflict { game_id, remote } => {
                if !self.pending_conflicts.iter().any(|c| c.game_id == game_id) {
                    self.pending_conflicts.push(Conflict { game_id, remote });
                }
            }
            Event::ResolvedConflict { game_id } => {
                self.pending_conflicts.retain(|c| c.game_id != game_id);
            }
            Event::Activity(a) => self.push_activity(a),
            Event::Error(e) => {
                self.last_error = Some(e.clone());
                self.push_activity(ActivityView::error(e));
            }
        }
    }

    fn push_activity(&mut self, a: ActivityView) {
        self.activity.push(a);
        if self.activity.len() > ACTIVITY_CAP {
            let overflow = self.activity.len() - ACTIVITY_CAP;
            self.activity.drain(0..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{ActivityKind, GroupView};

    fn version(n: u64) -> VersionView {
        VersionView {
            number: n,
            author: "a".into(),
            size: 10,
            created_at_ms: 0,
        }
    }

    #[test]
    fn groups_replace_wholesale() {
        let mut vm = ViewModel::default();
        vm.apply(Event::Groups(vec![GroupView {
            id: "g1".into(),
            name: "Crew".into(),
            games: vec![],
        }]));
        assert_eq!(vm.groups.len(), 1);
        vm.apply(Event::Groups(vec![]));
        assert!(vm.groups.is_empty());
    }

    #[test]
    fn conflict_is_recorded_once_and_cleared_on_resolve() {
        let mut vm = ViewModel::default();
        vm.apply(Event::Conflict {
            game_id: "steam:1".into(),
            remote: version(3),
        });
        vm.apply(Event::Conflict {
            game_id: "steam:1".into(),
            remote: version(3),
        });
        assert_eq!(vm.pending_conflicts.len(), 1);
        vm.apply(Event::ResolvedConflict {
            game_id: "steam:1".into(),
        });
        assert!(vm.pending_conflicts.is_empty());
    }

    #[test]
    fn scan_armed_then_results_moves_state() {
        let mut vm = ViewModel::default();
        vm.apply(Event::ScanArmed {
            game_id: "steam:1".into(),
        });
        assert_eq!(vm.scan_armed, vec!["steam:1".to_string()]);
        vm.apply(Event::ScanResults {
            game_id: "steam:1".into(),
            candidates: vec![],
        });
        assert!(vm.scan_armed.is_empty());
        assert!(vm.scan_results.contains_key("steam:1"));
    }

    #[test]
    fn error_sets_last_error_and_logs_it() {
        let mut vm = ViewModel::default();
        vm.apply(Event::Error("boom".into()));
        assert_eq!(vm.last_error.as_deref(), Some("boom"));
        assert_eq!(vm.activity.last().unwrap().kind, ActivityKind::Error);
    }

    #[test]
    fn activity_log_is_capped() {
        let mut vm = ViewModel::default();
        for i in 0..(ACTIVITY_CAP + 50) {
            vm.apply(Event::Activity(ActivityView::info(format!("line {i}"))));
        }
        assert_eq!(vm.activity.len(), ACTIVITY_CAP);
        // Oldest lines were dropped; the newest remains.
        assert_eq!(
            vm.activity.last().unwrap().message,
            format!("line {}", ACTIVITY_CAP + 49)
        );
    }
}
