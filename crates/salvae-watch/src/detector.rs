//! Game open/close detector.

use std::collections::BTreeMap;

use salvae_detect::game::{match_process_to_game, InstalledGame};

use crate::process::ProcessEvent;

/// A game opening or closing (the last of its processes exiting).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameEvent {
    Opened { game_id: String },
    Closed { game_id: String },
}

/// Maps process events to game events for a known set of installed games.
pub struct Detector {
    games: Vec<InstalledGame>,
    /// Matched process id -> game id.
    pid_game: BTreeMap<u32, String>,
    /// Game id -> number of its processes currently running.
    open_counts: BTreeMap<String, u32>,
}

impl Detector {
    /// Create a detector for the given installed games.
    pub fn new(games: Vec<InstalledGame>) -> Self {
        Self {
            games,
            pid_game: BTreeMap::new(),
            open_counts: BTreeMap::new(),
        }
    }

    /// Whether `game_id` currently has at least one process running.
    pub fn is_open(&self, game_id: &str) -> bool {
        self.open_counts.contains_key(game_id)
    }

    /// Feed process events; return the resulting game events (in order).
    pub fn process(&mut self, events: &[ProcessEvent]) -> Vec<GameEvent> {
        let mut out = Vec::new();
        for ev in events {
            match ev {
                ProcessEvent::Started { pid, exe_path } => {
                    if self.pid_game.contains_key(pid) {
                        continue;
                    }
                    if let Some(game) = match_process_to_game(&self.games, exe_path) {
                        let gid = game.id.clone();
                        self.pid_game.insert(*pid, gid.clone());
                        let count = self.open_counts.entry(gid.clone()).or_insert(0);
                        *count += 1;
                        if *count == 1 {
                            out.push(GameEvent::Opened { game_id: gid });
                        }
                    }
                }
                ProcessEvent::Stopped { pid, .. } => {
                    if let Some(gid) = self.pid_game.remove(pid) {
                        if let Some(count) = self.open_counts.get_mut(&gid) {
                            *count -= 1;
                            if *count == 0 {
                                self.open_counts.remove(&gid);
                                out.push(GameEvent::Closed { game_id: gid });
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvae_detect::game::Launcher;
    use std::path::PathBuf;

    fn games() -> Vec<InstalledGame> {
        vec![InstalledGame {
            id: "steam:892970".into(),
            name: "Valheim".into(),
            launcher: Launcher::Steam,
            install_dir: PathBuf::from("C:/Steam/common/Valheim"),
        }]
    }

    fn started(pid: u32, path: &str) -> ProcessEvent {
        ProcessEvent::Started {
            pid,
            exe_path: PathBuf::from(path),
        }
    }
    fn stopped(pid: u32, path: &str) -> ProcessEvent {
        ProcessEvent::Stopped {
            pid,
            exe_path: PathBuf::from(path),
        }
    }

    #[test]
    fn matched_process_opens_then_closes_the_game() {
        let mut d = Detector::new(games());
        let opened = d.process(&[started(10, "C:/Steam/common/Valheim/valheim.exe")]);
        assert_eq!(
            opened,
            vec![GameEvent::Opened {
                game_id: "steam:892970".into()
            }]
        );

        let closed = d.process(&[stopped(10, "C:/Steam/common/Valheim/valheim.exe")]);
        assert_eq!(
            closed,
            vec![GameEvent::Closed {
                game_id: "steam:892970".into()
            }]
        );
    }

    #[test]
    fn is_open_tracks_running_state() {
        let mut d = Detector::new(games());
        assert!(!d.is_open("steam:892970"));
        d.process(&[started(10, "C:/Steam/common/Valheim/valheim.exe")]);
        assert!(d.is_open("steam:892970"));
        d.process(&[stopped(10, "C:/Steam/common/Valheim/valheim.exe")]);
        assert!(!d.is_open("steam:892970"));
    }

    #[test]
    fn unmatched_process_yields_nothing() {
        let mut d = Detector::new(games());
        assert!(d
            .process(&[started(10, "C:/Windows/notepad.exe")])
            .is_empty());
        assert!(d
            .process(&[stopped(10, "C:/Windows/notepad.exe")])
            .is_empty());
    }

    #[test]
    fn multi_process_game_opens_once_and_closes_on_last_exit() {
        let mut d = Detector::new(games());
        // Two helper processes of the same game start.
        let e1 = d.process(&[started(10, "C:/Steam/common/Valheim/valheim.exe")]);
        let e2 = d.process(&[started(11, "C:/Steam/common/Valheim/helper.exe")]);
        assert_eq!(
            e1,
            vec![GameEvent::Opened {
                game_id: "steam:892970".into()
            }]
        );
        assert!(e2.is_empty()); // already open

        // First exits -> still open; second exits -> closed.
        assert!(d.process(&[stopped(10, "x")]).is_empty());
        assert_eq!(
            d.process(&[stopped(11, "x")]),
            vec![GameEvent::Closed {
                game_id: "steam:892970".into()
            }]
        );
    }

    #[test]
    fn full_loop_from_listings_to_game_events() {
        use crate::process::{ProcessInfo, ProcessLister, Watcher};
        use crate::WatchError;
        use std::cell::RefCell;

        struct FakeLister {
            frames: RefCell<std::collections::VecDeque<Vec<ProcessInfo>>>,
        }
        impl ProcessLister for FakeLister {
            fn list(&self) -> Result<Vec<ProcessInfo>, WatchError> {
                Ok(self.frames.borrow_mut().pop_front().unwrap_or_default())
            }
        }

        let valheim = ProcessInfo {
            pid: 7,
            exe_path: "C:/Steam/common/Valheim/valheim.exe".into(),
        };
        let lister = FakeLister {
            frames: RefCell::new(
                vec![
                    vec![ProcessInfo {
                        pid: 1,
                        exe_path: "C:/Windows/explorer.exe".into(),
                    }],
                    vec![
                        ProcessInfo {
                            pid: 1,
                            exe_path: "C:/Windows/explorer.exe".into(),
                        },
                        valheim.clone(),
                    ],
                    vec![ProcessInfo {
                        pid: 1,
                        exe_path: "C:/Windows/explorer.exe".into(),
                    }],
                ]
                .into(),
            ),
        };

        let mut watcher = Watcher::new(lister);
        let mut detector = Detector::new(games());

        // Poll 1: only explorer (no game).
        assert!(detector.process(&watcher.poll().unwrap()).is_empty());
        // Poll 2: Valheim launches -> Opened.
        assert_eq!(
            detector.process(&watcher.poll().unwrap()),
            vec![GameEvent::Opened {
                game_id: "steam:892970".into()
            }]
        );
        // Poll 3: Valheim exits -> Closed.
        assert_eq!(
            detector.process(&watcher.poll().unwrap()),
            vec![GameEvent::Closed {
                game_id: "steam:892970".into()
            }]
        );
    }
}
