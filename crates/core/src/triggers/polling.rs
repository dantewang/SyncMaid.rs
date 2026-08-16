//! Watching a network source by walking it.
//!
//! Windows change notifications are unreliable on mapped drives and UNC paths, so a network
//! source is polled instead — with the same quiet period, so it behaves the same way from the
//! user's side.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::io::FileSystem;
use crate::triggers::{
    Notification, TreeSnapshot, TriggerError, TriggerNotifier, TriggerObserver, TriggerSource,
};

/// How often to walk. Long enough not to hammer a share, short enough to feel responsive.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// Fires when a walk of the source shows a change that has since gone quiet.
pub struct PollingWatchTriggerSource {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    file_system: Arc<dyn FileSystem>,
    root: PathBuf,
    interval: Duration,
    /// How many consecutive unchanged polls the quiet period works out to. Zero means "fire as
    /// soon as a change is seen".
    required_quiet_polls: u32,
    notifier: TriggerNotifier,
    state: Mutex<bool>,
    wake: Condvar,
}

impl PollingWatchTriggerSource {
    pub fn new(
        file_system: Arc<dyn FileSystem>,
        root: impl Into<PathBuf>,
        settle: Duration,
        observer: Arc<dyn TriggerObserver>,
    ) -> Self {
        Self::with_interval(file_system, root, settle, DEFAULT_INTERVAL, observer)
    }

    pub fn with_interval(
        file_system: Arc<dyn FileSystem>,
        root: impl Into<PathBuf>,
        settle: Duration,
        interval: Duration,
        observer: Arc<dyn TriggerObserver>,
    ) -> Self {
        let interval = interval.max(Duration::from_millis(1));
        let required_quiet_polls = if settle.is_zero() {
            0
        } else {
            settle.as_millis().div_ceil(interval.as_millis()) as u32
        };

        Self {
            shared: Arc::new(Shared {
                file_system,
                root: root.into(),
                interval,
                required_quiet_polls,
                notifier: TriggerNotifier::new(observer),
                state: Mutex::new(false),
                wake: Condvar::new(),
            }),
            worker: None,
        }
    }
}

impl TriggerSource for PollingWatchTriggerSource {
    fn start(&mut self) -> Result<(), TriggerError> {
        if self.worker.is_some() {
            return Ok(());
        }

        *lock(&self.shared.state) = true;
        let shared = Arc::clone(&self.shared);
        let worker = std::thread::Builder::new()
            .name("syncmaid-poll".into())
            .spawn(move || shared.run())
            .map_err(|error| TriggerError::new(format!("could not start polling: {error}")))?;

        self.worker = Some(worker);
        Ok(())
    }

    fn stop(&mut self) {
        {
            let mut running = lock(&self.shared.state);
            *running = false;
            self.shared.notifier.invalidate();
        }
        self.shared.wake.notify_all();
        self.shared.notifier.wait_for_idle();

        // Deliberately *not* joined. A walk of a share that has gone unresponsive can block for
        // as long as the OS decides to wait, and stop must not inherit that. The worker checks
        // the state gate before enqueuing anything, and the invalidate above already dropped
        // whatever it had decided, so a late-finishing walk delivers nothing.
        self.worker = None;
    }
}

impl Drop for PollingWatchTriggerSource {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    fn run(&self) {
        let mut baseline: Option<TreeSnapshot> = None;
        let mut pending_change = false;
        let mut quiet_polls = 0u32;
        let mut reported_error = false;

        loop {
            if !self.park() {
                return;
            }

            // The walk happens outside the state gate on purpose: holding it here would make
            // stop and drop block behind a dead share.
            let walked = self.file_system.list_tree(Path::new(&self.root));

            let snapshot = match walked {
                Ok(listing) => {
                    if reported_error {
                        reported_error = false;
                        if !self.enqueue(Notification::Recovered) {
                            return;
                        }
                        self.notifier.drain();
                    }
                    TreeSnapshot::of(&listing)
                }
                Err(error) => {
                    // Reported once per outage, not once per poll.
                    if !reported_error {
                        reported_error = true;
                        if !self.enqueue(Notification::Error(error.to_string())) {
                            return;
                        }
                        self.notifier.drain();
                    }
                    continue;
                }
            };

            let Some(previous) = baseline.replace(snapshot.clone()) else {
                // The first successful walk is a baseline, not a change: everything already
                // there would otherwise read as brand new.
                continue;
            };

            if previous != snapshot {
                pending_change = true;
                quiet_polls = 0;
            } else if pending_change {
                quiet_polls += 1;
            }

            let settled = pending_change
                && (self.required_quiet_polls == 0 || quiet_polls >= self.required_quiet_polls);
            if settled {
                pending_change = false;
                quiet_polls = 0;
                if !self.enqueue(Notification::Fired) {
                    return;
                }
                self.notifier.drain();
            }
        }
    }

    /// Waits out the poll interval, returning false once stopped.
    fn park(&self) -> bool {
        let mut running = lock(&self.state);
        if !*running {
            return false;
        }
        let (guard, _) = self
            .wake
            .wait_timeout(running, self.interval)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        running = guard;
        *running
    }

    /// Queues a notification under the state gate, or reports that we have been stopped.
    fn enqueue(&self, notification: Notification) -> bool {
        let running = lock(&self.state);
        if !*running {
            return false;
        }
        self.notifier.enqueue(notification);
        true
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::io::InMemoryFileSystem;

    const ROOT: &str = r"\\server\share";

    fn channel_observer() -> (Arc<dyn TriggerObserver>, mpsc::Receiver<Notification>) {
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let observer = move |notification: Notification| {
            let _ = sender.lock().unwrap().send(notification);
        };
        (Arc::new(observer), receiver)
    }

    fn poller(
        memory: &InMemoryFileSystem,
        settle: Duration,
        observer: Arc<dyn TriggerObserver>,
    ) -> PollingWatchTriggerSource {
        PollingWatchTriggerSource::with_interval(
            Arc::new(memory.clone()),
            ROOT,
            settle,
            Duration::from_millis(25),
            observer,
        )
    }

    #[test]
    fn the_first_walk_is_a_baseline_and_does_not_fire() {
        let memory = InMemoryFileSystem::new();
        memory.add_file(format!(r"{ROOT}\already-here.txt"), b"a");
        let (observer, notifications) = channel_observer();
        let mut source = poller(&memory, Duration::ZERO, observer);

        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        source.stop();

        assert!(
            notifications.try_recv().is_err(),
            "everything already there would otherwise read as brand new"
        );
    }

    #[test]
    fn a_change_fires_once_the_quiet_period_has_passed() {
        let memory = InMemoryFileSystem::new();
        memory.add_directory(ROOT);
        let (observer, notifications) = channel_observer();
        // Two quiet polls' worth of settling.
        let mut source = poller(&memory, Duration::from_millis(50), observer);
        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(80));

        memory.add_file(format!(r"{ROOT}\new.txt"), b"n");

        assert_eq!(
            Ok(Notification::Fired),
            notifications.recv_timeout(Duration::from_secs(5))
        );
        source.stop();
    }

    #[test]
    fn a_stream_of_changes_collapses_into_one_run() {
        let memory = InMemoryFileSystem::new();
        memory.add_directory(ROOT);
        let (observer, notifications) = channel_observer();
        let mut source = poller(&memory, Duration::from_millis(75), observer);
        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(60));

        for index in 0..6 {
            memory.add_file(format!(r"{ROOT}\f{index}.txt"), b"x");
            std::thread::sleep(Duration::from_millis(25));
        }

        assert_eq!(
            Ok(Notification::Fired),
            notifications.recv_timeout(Duration::from_secs(5))
        );
        assert!(
            notifications
                .recv_timeout(Duration::from_millis(250))
                .is_err(),
            "the quiet period restarts on every change, exactly as the watcher's does"
        );
        source.stop();
    }

    #[test]
    fn a_missing_root_reports_an_error_once_and_recovers_when_it_returns() {
        let memory = InMemoryFileSystem::new();
        let (observer, notifications) = channel_observer();
        let mut source = poller(&memory, Duration::ZERO, observer);

        source.start().unwrap();
        let first = notifications.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(first, Notification::Error(_)), "got {first:?}");
        assert!(
            notifications
                .recv_timeout(Duration::from_millis(150))
                .is_err(),
            "an outage is reported once, not once per poll"
        );

        memory.add_directory(ROOT);

        assert_eq!(
            Ok(Notification::Recovered),
            notifications.recv_timeout(Duration::from_secs(5))
        );
        source.stop();
    }

    #[test]
    fn stopping_before_the_quiet_period_ends_delivers_nothing() {
        let memory = InMemoryFileSystem::new();
        memory.add_directory(ROOT);
        let (observer, notifications) = channel_observer();
        let mut source = poller(&memory, Duration::from_secs(30), observer);
        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(60));

        memory.add_file(format!(r"{ROOT}\new.txt"), b"n");
        std::thread::sleep(Duration::from_millis(60));
        source.stop();

        assert!(notifications.try_recv().is_err());
    }

    #[test]
    fn stop_does_not_wait_for_a_walk_stuck_on_an_unresponsive_share() {
        struct Hangs {
            inner: InMemoryFileSystem,
            release: Arc<(Mutex<bool>, Condvar)>,
        }

        impl FileSystem for Hangs {
            fn list_tree(&self, _root: &Path) -> std::io::Result<crate::io::TreeListing> {
                let (lock, condvar) = &*self.release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = condvar.wait(released).unwrap();
                }
                Ok(crate::io::TreeListing::empty())
            }

            // Everything else is irrelevant here and simply forwards.
            fn file_exists(&self, path: &Path) -> bool {
                self.inner.file_exists(path)
            }
            fn get_stamp(&self, path: &Path) -> std::io::Result<crate::io::FileStamp> {
                self.inner.get_stamp(path)
            }
            fn read_all_bytes(&self, path: &Path) -> std::io::Result<Vec<u8>> {
                self.inner.read_all_bytes(path)
            }
            fn write_all_bytes(&self, path: &Path, contents: &[u8]) -> std::io::Result<()> {
                self.inner.write_all_bytes(path, contents)
            }
            fn delete_file(&self, path: &Path) -> std::io::Result<()> {
                self.inner.delete_file(path)
            }
            fn recycle(&self, path: &Path) -> std::io::Result<()> {
                self.inner.recycle(path)
            }
            fn ensure_directory(&self, path: &Path) -> std::io::Result<()> {
                self.inner.ensure_directory(path)
            }
            fn delete_empty_directory(&self, path: &Path) -> std::io::Result<()> {
                self.inner.delete_empty_directory(path)
            }
            fn recycle_empty_directory(&self, path: &Path) -> std::io::Result<()> {
                self.inner.recycle_empty_directory(path)
            }
            fn set_directory_last_write_time_utc(
                &self,
                path: &Path,
                time: chrono::DateTime<chrono::Utc>,
            ) -> std::io::Result<()> {
                self.inner.set_directory_last_write_time_utc(path, time)
            }
            fn open_read(&self, path: &Path) -> std::io::Result<Box<dyn std::io::Read + Send>> {
                self.inner.open_read(path)
            }
            fn create_write_through(
                &self,
                path: &Path,
            ) -> std::io::Result<Box<dyn std::io::Write + Send>> {
                self.inner.create_write_through(path)
            }
            fn set_last_write_time_utc(
                &self,
                path: &Path,
                time: chrono::DateTime<chrono::Utc>,
            ) -> std::io::Result<()> {
                self.inner.set_last_write_time_utc(path, time)
            }
            fn replace(&self, source: &Path, destination: &Path) -> std::io::Result<()> {
                self.inner.replace(source, destination)
            }
            fn available_free_space(&self, path: &Path) -> u64 {
                self.inner.available_free_space(path)
            }
        }

        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let file_system: Arc<dyn FileSystem> = Arc::new(Hangs {
            inner: InMemoryFileSystem::new(),
            release: Arc::clone(&release),
        });
        let (observer, _notifications) = channel_observer();
        let mut source = PollingWatchTriggerSource::with_interval(
            file_system,
            ROOT,
            Duration::ZERO,
            Duration::from_millis(5),
            observer,
        );

        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(50));

        let started = std::time::Instant::now();
        source.stop();
        let took = started.elapsed();

        // Let the stuck walk finish so the thread can exit.
        *release.0.lock().unwrap() = true;
        release.1.notify_all();

        assert!(
            took < Duration::from_millis(500),
            "stop inherited the walk's wait, which on a dead share is however long the OS says"
        );
    }
}
