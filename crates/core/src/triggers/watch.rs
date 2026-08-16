//! Fires when the source changes, once it has been quiet for a while.
//!
//! Change notifications only say *something* changed, and programs write in bursts — an image,
//! then its thumbnail, then its metadata — so every fresh change restarts the wait and one
//! burst syncs as one run.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::triggers::{
    Notification, TriggerError, TriggerNotifier, TriggerObserver, TriggerSource,
};

/// Watcher events and the stop signal share one channel, so the worker parks on a single
/// receive and needs no polling to notice either.
enum WatchMessage {
    Changed,
    WatcherFailed(String),
    Stop,
}

/// Fires after `settle` of quiet following any change under the source.
pub struct WatchTriggerSource {
    root: PathBuf,
    settle: Duration,
    notifier: Arc<TriggerNotifier>,
    control: Option<Sender<WatchMessage>>,
    worker: Option<JoinHandle<()>>,
}

impl WatchTriggerSource {
    pub fn new(
        root: impl Into<PathBuf>,
        settle: Duration,
        observer: Arc<dyn TriggerObserver>,
    ) -> Self {
        Self {
            root: root.into(),
            settle,
            notifier: Arc::new(TriggerNotifier::new(observer)),
            control: None,
            worker: None,
        }
    }
}

impl TriggerSource for WatchTriggerSource {
    fn start(&mut self) -> Result<(), TriggerError> {
        if self.worker.is_some() {
            return Ok(());
        }

        let (sender, receiver) = mpsc::channel();
        let watcher = spawn_watcher(&self.root, sender.clone())?;

        let notifier = Arc::clone(&self.notifier);
        let root = self.root.clone();
        let settle = self.settle;
        let events = sender.clone();
        let worker = std::thread::Builder::new()
            .name("syncmaid-watch".into())
            .spawn(move || {
                let mut worker = Worker {
                    root,
                    settle,
                    notifier,
                    watcher: Some(watcher),
                    events,
                };
                worker.run(receiver);
            })
            .map_err(|error| TriggerError::new(format!("could not start the watcher: {error}")))?;

        self.control = Some(sender);
        self.worker = Some(worker);
        Ok(())
    }

    fn stop(&mut self) {
        // Decided before the transition, so nothing queued can deliver after it.
        self.notifier.invalidate();

        if let Some(control) = self.control.take() {
            let _ = control.send(WatchMessage::Stop);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.notifier.wait_for_idle();
    }
}

impl Drop for WatchTriggerSource {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn_watcher(
    root: &Path,
    sender: Sender<WatchMessage>,
) -> Result<RecommendedWatcher, TriggerError> {
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let message = match result {
            Ok(_) => WatchMessage::Changed,
            Err(error) => WatchMessage::WatcherFailed(error.to_string()),
        };
        let _ = sender.send(message);
    })
    .map_err(|error| TriggerError::new(error.to_string()))?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|error| TriggerError::new(error.to_string()))?;
    Ok(watcher)
}

struct Worker {
    root: PathBuf,
    settle: Duration,
    notifier: Arc<TriggerNotifier>,
    watcher: Option<RecommendedWatcher>,
    events: Sender<WatchMessage>,
}

impl Worker {
    fn run(&mut self, receiver: mpsc::Receiver<WatchMessage>) {
        let mut quiet_by: Option<Instant> = None;

        loop {
            let message = match quiet_by {
                // Nothing pending: park until something happens.
                None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
                // A change is waiting out its quiet period.
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    receiver.recv_timeout(remaining)
                }
            };

            match message {
                Ok(WatchMessage::Changed) => {
                    // Every fresh change restarts the wait, so one burst is one run.
                    quiet_by = Some(Instant::now() + self.settle);
                }
                Ok(WatchMessage::WatcherFailed(reason)) => {
                    if !self.recover(&reason) {
                        return;
                    }
                    quiet_by = None;
                }
                Ok(WatchMessage::Stop) => return,
                Err(RecvTimeoutError::Timeout) => {
                    quiet_by = None;
                    self.notifier.enqueue(Notification::Fired);
                    self.notifier.drain();
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Rebuilds a watcher the OS dropped — most often because its event buffer overflowed.
    ///
    /// Reports the failure, then the recovery, then **one** fire: the events that were dropped
    /// still describe real changes, and a run over an unchanged tree is a planner no-op, so
    /// syncing once is strictly cheaper than missing something.
    fn recover(&mut self, reason: &str) -> bool {
        self.notifier
            .enqueue(Notification::Error(reason.to_owned()));
        self.watcher = None;

        match spawn_watcher(&self.root, self.events.clone()) {
            Ok(watcher) => {
                self.watcher = Some(watcher);
                self.notifier.enqueue(Notification::Recovered);
                self.notifier.enqueue(Notification::Fired);
                self.notifier.drain();
                true
            }
            Err(error) => {
                self.notifier.enqueue(Notification::Error(format!(
                    "The filesystem watcher stopped and its restart failed: {error}"
                )));
                self.notifier.drain();
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Mutex};

    use super::*;

    fn channel_observer() -> (Arc<dyn TriggerObserver>, mpsc::Receiver<Notification>) {
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let observer = move |notification: Notification| {
            let _ = sender.lock().unwrap().send(notification);
        };
        (Arc::new(observer), receiver)
    }

    fn settle() -> Duration {
        Duration::from_millis(120)
    }

    #[test]
    fn a_change_fires_once_the_source_goes_quiet() {
        let root = tempfile::tempdir().unwrap();
        let (observer, notifications) = channel_observer();
        let mut source = WatchTriggerSource::new(root.path(), settle(), observer);
        source.start().unwrap();

        std::fs::write(root.path().join("a.txt"), b"a").unwrap();

        assert_eq!(
            Ok(Notification::Fired),
            notifications.recv_timeout(Duration::from_secs(5))
        );
        source.stop();
    }

    #[test]
    fn one_burst_of_changes_is_one_run() {
        let root = tempfile::tempdir().unwrap();
        let (observer, notifications) = channel_observer();
        let mut source = WatchTriggerSource::new(root.path(), settle(), observer);
        source.start().unwrap();

        // An image, then its thumbnail, then its metadata — the shape a real program writes in.
        for index in 0..6 {
            std::fs::write(root.path().join(format!("f{index}.txt")), b"x").unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(
            Ok(Notification::Fired),
            notifications.recv_timeout(Duration::from_secs(5))
        );
        assert!(
            notifications
                .recv_timeout(Duration::from_millis(400))
                .is_err(),
            "every fresh change restarts the wait, so the burst collapses into one run"
        );
        source.stop();
    }

    #[test]
    fn nothing_fires_without_a_change() {
        let root = tempfile::tempdir().unwrap();
        let (observer, notifications) = channel_observer();
        let mut source = WatchTriggerSource::new(root.path(), settle(), observer);

        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        source.stop();

        assert!(notifications.try_recv().is_err());
    }

    #[test]
    fn a_change_still_settling_when_stop_arrives_never_fires() {
        let root = tempfile::tempdir().unwrap();
        let (observer, notifications) = channel_observer();
        let mut source = WatchTriggerSource::new(root.path(), Duration::from_secs(30), observer);
        source.start().unwrap();

        std::fs::write(root.path().join("a.txt"), b"a").unwrap();
        std::thread::sleep(Duration::from_millis(80));
        source.stop();

        assert!(
            notifications.try_recv().is_err(),
            "a fire decided before stop must not land after stop returns"
        );
    }

    #[test]
    fn watching_a_folder_that_is_not_there_fails_to_start() {
        let root = tempfile::tempdir().unwrap();
        let (observer, _) = channel_observer();
        let mut source = WatchTriggerSource::new(root.path().join("gone"), settle(), observer);

        let started = source.start();

        assert!(
            started.is_err(),
            "the card shows this as a trigger error and stays manual-only"
        );
    }

    #[test]
    fn stopping_a_source_that_never_started_is_harmless() {
        let root = tempfile::tempdir().unwrap();
        let (observer, _) = channel_observer();
        let mut source = WatchTriggerSource::new(root.path(), settle(), observer);

        source.stop();
        source.stop();
    }
}
