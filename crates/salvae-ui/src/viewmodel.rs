//! The render state the egui layer draws each frame, updated by applying
//! worker `Event`s. Pure data + `apply` — no I/O.

use std::collections::BTreeMap;

use crate::command::Event;
use crate::view::{
    ActivityView, ChannelView, Conflict, GameView, GroupView, GuildView, VersionView,
};

/// Maximum activity-log lines kept in memory.
const ACTIVITY_CAP: usize = 200;

/// Everything the UI renders.
#[derive(Debug, Default)]
pub struct ViewModel {
    pub groups: Vec<GroupView>,
    pub installed_games: Vec<GameView>,
    /// Servers the bot can see, for the create-group picker.
    pub discovered_guilds: Vec<GuildView>,
    /// Channels of the picked server, for the create-group picker.
    pub discovered_channels: Vec<ChannelView>,
    /// Whether a `FetchGuilds` has succeeded (token validated) — gates the
    /// server/channel pickers in the create dialog.
    pub guilds_loaded: bool,
    /// Whether the bot token validated (wizard step 2).
    pub token_validated: bool,
    /// The validated bot's id (OAuth2 client id) + display name.
    pub bot_id: Option<u64>,
    pub bot_name: Option<String>,
    pub history: BTreeMap<String, Vec<VersionView>>,
    /// Games whose sync was enabled but whose save folder could not be
    /// auto-resolved — the UI prompts a manual pick for these.
    pub unresolved: Vec<String>,
    pub pending_conflicts: Vec<Conflict>,
    pub activity: Vec<ActivityView>,
    pub last_invite: Option<String>,
    /// An invite the UI should copy to the clipboard on the next frame.
    pub invite_to_copy: Option<String>,
    /// A newer app version available to install (shows the "Atualizar" button).
    pub available_update: Option<String>,
    pub last_error: Option<String>,
}

impl ViewModel {
    /// Fold one worker event into the render state.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Groups(mut g) => {
                // A game that now has a configured folder is no longer unresolved.
                self.unresolved.retain(|id| {
                    !g.iter()
                        .any(|grp| grp.games.iter().any(|m| &m.game_id == id))
                });
                g.sort_by_key(|x| x.name.to_lowercase());
                self.groups = g;
            }
            Event::InstalledGames(mut g) => {
                g.sort_by_key(|x| x.name.to_lowercase());
                self.installed_games = g;
            }
            Event::TokenValidated { bot_id, bot_name } => {
                self.token_validated = true;
                self.bot_id = Some(bot_id);
                self.bot_name = Some(bot_name);
            }
            Event::DiscoveredGuilds(g) => {
                self.discovered_guilds = g;
                self.discovered_channels.clear();
                self.guilds_loaded = true;
            }
            Event::DiscoveredChannels(c) => self.discovered_channels = c,
            Event::Invite(s) => {
                self.last_invite = Some(s);
                self.push_activity(ActivityView::info(
                    "Grupo criado — compartilhe o convite abaixo",
                ));
            }
            Event::InviteToCopy(s) => self.invite_to_copy = Some(s),
            Event::UpdateAvailable { version } => {
                self.available_update = Some(version.clone());
                self.push_activity(ActivityView::info(format!(
                    "Atualização disponível: v{version}"
                )));
            }
            Event::History { game_id, versions } => {
                self.history.insert(game_id, versions);
            }
            Event::SyncUnresolved { game_id } => {
                if !self.unresolved.contains(&game_id) {
                    self.unresolved.push(game_id);
                }
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
        // Collapse consecutive identical lines (e.g. a sync error repeating
        // every tick) instead of spamming the log.
        if self.activity.last().map(|l| &l.message) == Some(&a.message) {
            return;
        }
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
    fn unresolved_is_set_then_cleared_when_game_gets_a_folder() {
        use crate::view::GameMapping;
        let mut vm = ViewModel::default();
        vm.apply(Event::SyncUnresolved {
            game_id: "steam:1".into(),
        });
        assert_eq!(vm.unresolved, vec!["steam:1".to_string()]);
        // Once the game shows up with a configured folder, it's resolved.
        vm.apply(Event::Groups(vec![GroupView {
            id: "g1".into(),
            name: "Crew".into(),
            games: vec![GameMapping {
                game_id: "steam:1".into(),
                folder: "C:/x".into(),
            }],
        }]));
        assert!(vm.unresolved.is_empty());
    }

    #[test]
    fn discovered_guilds_replace_and_clear_channels() {
        use crate::view::{ChannelView, GuildView};
        let mut vm = ViewModel::default();
        vm.apply(Event::DiscoveredChannels(vec![ChannelView {
            id: 1,
            name: "old".into(),
        }]));
        vm.apply(Event::DiscoveredGuilds(vec![GuildView {
            id: 9,
            name: "Crew".into(),
        }]));
        assert_eq!(vm.discovered_guilds.len(), 1);
        assert!(vm.guilds_loaded);
        // Picking servers afresh clears any previously-listed channels.
        assert!(vm.discovered_channels.is_empty());

        vm.apply(Event::DiscoveredChannels(vec![ChannelView {
            id: 2,
            name: "saves".into(),
        }]));
        assert_eq!(vm.discovered_channels.len(), 1);
    }

    #[test]
    fn error_sets_last_error_and_logs_it() {
        let mut vm = ViewModel::default();
        vm.apply(Event::Error("boom".into()));
        assert_eq!(vm.last_error.as_deref(), Some("boom"));
        assert_eq!(vm.activity.last().unwrap().kind, ActivityKind::Error);
    }

    #[test]
    fn consecutive_identical_activity_is_collapsed() {
        let mut vm = ViewModel::default();
        vm.apply(Event::Error("boom".into()));
        vm.apply(Event::Error("boom".into()));
        vm.apply(Event::Error("boom".into()));
        assert_eq!(vm.activity.len(), 1);
        vm.apply(Event::Activity(ActivityView::info("other")));
        assert_eq!(vm.activity.len(), 2);
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
