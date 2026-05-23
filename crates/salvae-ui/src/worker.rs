//! The background worker: translate one `Command` into backend calls and the
//! resulting `Event`s (`dispatch`), and run the receive/tick loop (`run`).

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::backend::Backend;
use crate::command::{Command, Event};
use crate::view::ActivityView;

/// Translate a single command into the events it produces. Pure w.r.t. I/O:
/// all side effects go through `backend`, so this is unit-testable with a fake.
pub fn dispatch<B: Backend>(backend: &mut B, command: Command) -> Vec<Event> {
    match command {
        Command::Refresh => vec![
            Event::Groups(backend.refresh_groups()),
            Event::InstalledGames(backend.installed_games()),
        ],
        Command::CreateGroup {
            name,
            password,
            token,
            guild_id,
            channel_id,
        } => match backend.create_group(&name, &password, &token, guild_id, channel_id) {
            Ok(invite) => vec![
                Event::Invite(invite),
                Event::Groups(backend.refresh_groups()),
            ],
            Err(e) => vec![Event::Error(e)],
        },
        Command::ValidateToken { token } => match backend.validate_token(&token) {
            Ok((bot_id, bot_name)) => vec![Event::TokenValidated { bot_id, bot_name }],
            Err(e) => vec![Event::Error(e)],
        },
        Command::FetchGuilds { token } => match backend.fetch_guilds(&token) {
            Ok(guilds) => vec![Event::DiscoveredGuilds(guilds)],
            Err(e) => vec![Event::Error(e)],
        },
        Command::FetchChannels { token, guild_id } => {
            match backend.fetch_channels(&token, guild_id) {
                Ok(channels) => vec![Event::DiscoveredChannels(channels)],
                Err(e) => vec![Event::Error(e)],
            }
        }
        Command::JoinGroup { password, invite } => match backend.join_group(&password, &invite) {
            Ok(()) => vec![
                Event::Activity(ActivityView::info("Joined group")),
                Event::Groups(backend.refresh_groups()),
            ],
            Err(e) => vec![Event::Error(e)],
        },
        Command::RemoveGroup { group_id } => match backend.remove_group(&group_id) {
            Ok(()) => vec![Event::Groups(backend.refresh_groups())],
            Err(e) => vec![Event::Error(e)],
        },
        Command::SetGamePath {
            group_id,
            game_id,
            folder,
        } => match backend.set_game_path(&group_id, &game_id, &folder) {
            Ok(()) => vec![
                Event::Activity(ActivityView::info(format!("Set folder for {game_id}"))),
                Event::Groups(backend.refresh_groups()),
            ],
            Err(e) => vec![Event::Error(e)],
        },
        Command::ArmScan { game_id } => match backend.arm_scan(&game_id) {
            Ok(()) => vec![Event::ScanArmed { game_id }],
            Err(e) => vec![Event::Error(e)],
        },
        Command::CollectScan { game_id } => match backend.collect_scan(&game_id) {
            Ok(candidates) => vec![Event::ScanResults {
                game_id,
                candidates,
            }],
            Err(e) => vec![Event::Error(e)],
        },
        Command::LoadHistory { game_id } => match backend.history(&game_id) {
            Ok(versions) => vec![Event::History { game_id, versions }],
            Err(e) => vec![Event::Error(e)],
        },
        Command::Restore { game_id, version } => match backend.restore(&game_id, version) {
            Ok(()) => vec![Event::Activity(ActivityView::info(format!(
                "Restored {game_id} to version {version}"
            )))],
            Err(e) => vec![Event::Error(e)],
        },
        Command::Resolve {
            game_id,
            take_remote,
        } => match backend.resolve(&game_id, take_remote) {
            Ok(()) => vec![
                Event::Activity(ActivityView::info(format!(
                    "Resolved conflict for {game_id}"
                ))),
                Event::ResolvedConflict { game_id },
            ],
            Err(e) => vec![Event::Error(e)],
        },
        Command::Shutdown => vec![],
    }
}

/// Run the worker loop until `Shutdown` or the command channel closes. Sends an
/// initial refresh, processes commands as they arrive, and `tick`s the backend
/// on each idle timeout. `repaint` wakes the UI thread after sending events.
pub fn run<B: Backend>(
    mut backend: B,
    rx: Receiver<Command>,
    tx: Sender<Event>,
    repaint: impl Fn(),
    tick_interval: Duration,
) {
    for ev in dispatch(&mut backend, Command::Refresh) {
        let _ = tx.send(ev);
    }
    repaint();

    loop {
        let events = match rx.recv_timeout(tick_interval) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(command) => dispatch(&mut backend, command),
            Err(RecvTimeoutError::Timeout) => backend.tick(),
        };
        let had_events = !events.is_empty();
        for ev in events {
            let _ = tx.send(ev);
        }
        if had_events {
            repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{
        ChannelView, DiscoveredCandidate, GameView, GroupView, GuildView, VersionView,
    };

    /// A scripted fake backend recording calls and returning canned results.
    #[derive(Default)]
    struct FakeBackend {
        groups: Vec<GroupView>,
        create_result: Option<Result<String, String>>,
        history_result: Vec<VersionView>,
        guilds_result: Vec<GuildView>,
        resolved: Vec<String>,
        tick_events: Vec<Event>,
    }

    impl Backend for FakeBackend {
        fn refresh_groups(&self) -> Vec<GroupView> {
            self.groups.clone()
        }
        fn installed_games(&self) -> Vec<GameView> {
            vec![GameView {
                id: "steam:1".into(),
                name: "Valheim".into(),
            }]
        }
        fn create_group(
            &mut self,
            _: &str,
            _: &str,
            _: &str,
            _: u64,
            _: u64,
        ) -> Result<String, String> {
            self.create_result
                .clone()
                .unwrap_or_else(|| Ok("invite-blob".into()))
        }
        fn validate_token(&self, _: &str) -> Result<(u64, String), String> {
            Ok((42, "SalvaeBot".into()))
        }
        fn fetch_guilds(&self, _: &str) -> Result<Vec<GuildView>, String> {
            Ok(self.guilds_result.clone())
        }
        fn fetch_channels(&self, _: &str, _: u64) -> Result<Vec<ChannelView>, String> {
            Ok(vec![ChannelView {
                id: 10,
                name: "saves".into(),
            }])
        }
        fn join_group(&mut self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_group(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn set_game_path(&mut self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn arm_scan(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn collect_scan(&mut self, _: &str) -> Result<Vec<DiscoveredCandidate>, String> {
            Ok(vec![])
        }
        fn history(&mut self, _: &str) -> Result<Vec<VersionView>, String> {
            Ok(self.history_result.clone())
        }
        fn restore(&mut self, _: &str, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn resolve(&mut self, game_id: &str, _: bool) -> Result<(), String> {
            self.resolved.push(game_id.to_string());
            Ok(())
        }
        fn tick(&mut self) -> Vec<Event> {
            std::mem::take(&mut self.tick_events)
        }
    }

    #[test]
    fn create_group_emits_invite_then_groups() {
        let mut b = FakeBackend::default();
        let events = dispatch(
            &mut b,
            Command::CreateGroup {
                name: "Crew".into(),
                password: "pw".into(),
                token: "tok".into(),
                guild_id: 1,
                channel_id: 2,
            },
        );
        assert_eq!(events[0], Event::Invite("invite-blob".into()));
        assert!(matches!(events[1], Event::Groups(_)));
    }

    #[test]
    fn failed_create_group_emits_only_error() {
        let mut b = FakeBackend {
            create_result: Some(Err("bad token".into())),
            ..Default::default()
        };
        let events = dispatch(
            &mut b,
            Command::CreateGroup {
                name: "Crew".into(),
                password: "pw".into(),
                token: "tok".into(),
                guild_id: 1,
                channel_id: 2,
            },
        );
        assert_eq!(events, vec![Event::Error("bad token".into())]);
    }

    #[test]
    fn resolve_emits_resolved_conflict() {
        let mut b = FakeBackend::default();
        let events = dispatch(
            &mut b,
            Command::Resolve {
                game_id: "steam:1".into(),
                take_remote: true,
            },
        );
        assert_eq!(b.resolved, vec!["steam:1".to_string()]);
        assert!(events.contains(&Event::ResolvedConflict {
            game_id: "steam:1".into()
        }));
    }

    #[test]
    fn validate_token_emits_token_validated() {
        let mut b = FakeBackend::default();
        let events = dispatch(
            &mut b,
            Command::ValidateToken {
                token: "tok".into(),
            },
        );
        assert_eq!(
            events,
            vec![Event::TokenValidated {
                bot_id: 42,
                bot_name: "SalvaeBot".into()
            }]
        );
    }

    #[test]
    fn fetch_guilds_emits_discovered_guilds() {
        let mut b = FakeBackend {
            guilds_result: vec![GuildView {
                id: 7,
                name: "Crew".into(),
            }],
            ..Default::default()
        };
        let events = dispatch(
            &mut b,
            Command::FetchGuilds {
                token: "tok".into(),
            },
        );
        assert_eq!(
            events,
            vec![Event::DiscoveredGuilds(vec![GuildView {
                id: 7,
                name: "Crew".into()
            }])]
        );
    }

    #[test]
    fn fetch_channels_emits_discovered_channels() {
        let mut b = FakeBackend::default();
        let events = dispatch(
            &mut b,
            Command::FetchChannels {
                token: "tok".into(),
                guild_id: 7,
            },
        );
        assert!(matches!(&events[0], Event::DiscoveredChannels(c) if c[0].id == 10));
    }

    #[test]
    fn load_history_returns_versions() {
        let mut b = FakeBackend {
            history_result: vec![VersionView {
                number: 1,
                author: "a".into(),
                size: 1,
                created_at_ms: 0,
            }],
            ..Default::default()
        };
        let events = dispatch(
            &mut b,
            Command::LoadHistory {
                game_id: "steam:1".into(),
            },
        );
        assert!(matches!(&events[0], Event::History { versions, .. } if versions.len() == 1));
    }
}
