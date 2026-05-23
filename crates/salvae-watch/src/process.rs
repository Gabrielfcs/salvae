//! Process model, lister trait, and the diffing watcher.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::WatchError;

/// A running process: its id and full executable path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub exe_path: PathBuf,
}

/// A change in the running-process set between two listings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessEvent {
    Started { pid: u32, exe_path: PathBuf },
    Stopped { pid: u32, exe_path: PathBuf },
}

/// Lists currently-running processes. Implemented by the OS-specific lister and
/// by test fakes.
pub trait ProcessLister {
    fn list(&self) -> Result<Vec<ProcessInfo>, WatchError>;
}

/// Diffs successive process listings into [`ProcessEvent`]s.
pub struct Watcher<L: ProcessLister> {
    lister: L,
    prev: BTreeMap<u32, PathBuf>,
}

impl<L: ProcessLister> Watcher<L> {
    /// Create a watcher with an empty baseline (the first `poll` reports every
    /// running process as `Started`).
    pub fn new(lister: L) -> Self {
        Self {
            lister,
            prev: BTreeMap::new(),
        }
    }

    /// List processes now and return what changed since the previous poll.
    ///
    /// Limitation: if the OS reuses a pid for a *different* executable between
    /// two polls, the change is not detected (inherent to pid-based polling).
    pub fn poll(&mut self) -> Result<Vec<ProcessEvent>, WatchError> {
        let current: BTreeMap<u32, PathBuf> = self
            .lister
            .list()?
            .into_iter()
            .map(|p| (p.pid, p.exe_path))
            .collect();

        let mut events = Vec::new();
        for (pid, path) in &current {
            if !self.prev.contains_key(pid) {
                events.push(ProcessEvent::Started {
                    pid: *pid,
                    exe_path: path.clone(),
                });
            }
        }
        for (pid, path) in &self.prev {
            if !current.contains_key(pid) {
                events.push(ProcessEvent::Stopped {
                    pid: *pid,
                    exe_path: path.clone(),
                });
            }
        }
        self.prev = current;
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A lister that returns a scripted sequence of process listings.
    struct FakeLister {
        frames: RefCell<std::collections::VecDeque<Vec<ProcessInfo>>>,
    }
    impl FakeLister {
        fn new(frames: Vec<Vec<ProcessInfo>>) -> Self {
            Self {
                frames: RefCell::new(frames.into()),
            }
        }
    }
    impl ProcessLister for FakeLister {
        fn list(&self) -> Result<Vec<ProcessInfo>, WatchError> {
            Ok(self.frames.borrow_mut().pop_front().unwrap_or_default())
        }
    }

    fn proc(pid: u32, path: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            exe_path: PathBuf::from(path),
        }
    }

    #[test]
    fn first_poll_reports_all_as_started() {
        let lister = FakeLister::new(vec![vec![proc(1, "a.exe"), proc(2, "b.exe")]]);
        let mut w = Watcher::new(lister);
        let mut events = w.poll().unwrap();
        events.sort_by_key(|e| match e {
            ProcessEvent::Started { pid, .. } | ProcessEvent::Stopped { pid, .. } => *pid,
        });
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ProcessEvent::Started { pid: 1, .. }));
    }

    #[test]
    fn detects_started_and_stopped_between_polls() {
        let lister = FakeLister::new(vec![
            vec![proc(1, "a.exe")],                   // poll 1: 1 starts
            vec![proc(1, "a.exe"), proc(2, "b.exe")], // poll 2: 2 starts
            vec![proc(2, "b.exe")],                   // poll 3: 1 stops
        ]);
        let mut w = Watcher::new(lister);

        let p1 = w.poll().unwrap();
        assert_eq!(
            p1,
            vec![ProcessEvent::Started {
                pid: 1,
                exe_path: "a.exe".into()
            }]
        );

        let p2 = w.poll().unwrap();
        assert_eq!(
            p2,
            vec![ProcessEvent::Started {
                pid: 2,
                exe_path: "b.exe".into()
            }]
        );

        let p3 = w.poll().unwrap();
        assert_eq!(
            p3,
            vec![ProcessEvent::Stopped {
                pid: 1,
                exe_path: "a.exe".into()
            }]
        );
    }

    #[test]
    fn no_change_yields_no_events() {
        let lister = FakeLister::new(vec![vec![proc(1, "a.exe")], vec![proc(1, "a.exe")]]);
        let mut w = Watcher::new(lister);
        w.poll().unwrap();
        assert!(w.poll().unwrap().is_empty());
    }
}
