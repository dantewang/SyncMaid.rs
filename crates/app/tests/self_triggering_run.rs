//! What a task's own run costs its own trigger.
//!
//! Nothing suppresses a trigger around a run. A Move run mutates the very source its watcher is
//! watching, so the watcher fires once afterwards — and that follow-up is a no-op, because
//! planning is idempotent. These tests pin the price at **exactly one extra run, with no
//! cascade**, so that nobody later "fixes" it by stopping the trigger around runs: doing that
//! saves one tree walk and buys a resume-failure path that has to surface on the card.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use syncmaid::state::RunGate;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::io::PhysicalFileSystem;
use syncmaid_core::model::{Destination, SyncStrategy, SyncTask, SyncTaskKind};
use syncmaid_core::sync::SyncEngine;
use syncmaid_core::triggers::{
    Notification, Trigger, TriggerObserver, TriggerSource, WatchTriggerSource,
};
use tempfile::TempDir;

/// Long enough that a burst collapses, short enough that the test is not a nap.
const SETTLE: Duration = Duration::from_millis(300);
/// Well past the point where a cascade would have shown itself.
const OBSERVE: Duration = Duration::from_millis(2500);

struct Harness {
    engine: SyncEngine,
    gate: RunGate,
    task: Mutex<SyncTask>,
    runs: AtomicUsize,
    files_moved: AtomicUsize,
}

impl Harness {
    /// One pass of the drain loop a caller owns: run, then take whatever was absorbed.
    fn drive(&self) {
        let Some(mut start) = self.gate.request(HashSet::new()) else {
            return;
        };
        loop {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let task = self.task.lock().unwrap().clone();
            let statuses = self
                .engine
                .execute(
                    &task,
                    &start.cancellation,
                    &mut |_| {},
                    &start.confirmed_mass_deletes,
                )
                .expect("not cancelled");
            let moved: i32 = statuses.iter().map(|status| status.files_copied).sum();
            self.files_moved.fetch_add(moved as usize, Ordering::SeqCst);

            match self.gate.next() {
                Some(next) => start = next,
                None => break,
            }
        }
    }
}

/// Runs the task whenever the watcher says so, on the watcher's own delivery thread — which is
/// where a real trigger subscriber runs too.
struct RunOnFire(Arc<Harness>);

impl TriggerObserver for RunOnFire {
    fn notify(&self, notification: Notification) {
        if notification == Notification::Fired {
            self.0.drive();
        }
    }
}

fn harness(source: &TempDir, destination: &TempDir, strategy: SyncStrategy) -> Arc<Harness> {
    let kind = if strategy == SyncStrategy::Move {
        SyncTaskKind::Move
    } else {
        SyncTaskKind::Sync
    };
    let task = SyncTask::new(
        "T",
        source.path().to_string_lossy().into_owned(),
        Trigger::Watch { settle_seconds: 1 },
        vec![Destination::new(
            "D",
            destination.path().to_string_lossy().into_owned(),
            [FilterRule::AllFiles],
            strategy,
        )],
    )
    .with_kind(kind);

    Arc::new(Harness {
        engine: SyncEngine::local(Arc::new(PhysicalFileSystem::new())),
        gate: RunGate::new(),
        task: Mutex::new(task),
        runs: AtomicUsize::new(0),
        files_moved: AtomicUsize::new(0),
    })
}

fn seed(source: &TempDir) {
    std::fs::create_dir_all(source.path().join("sub")).unwrap();
    std::fs::write(source.path().join("a.txt"), b"a").unwrap();
    std::fs::write(source.path().join("sub/b.txt"), b"b").unwrap();
    std::fs::write(source.path().join("sub/c.txt"), b"c").unwrap();
}

/// Waits until `runs` stops climbing, or the observation window ends.
fn settle(harness: &Harness) {
    let deadline = Instant::now() + OBSERVE;
    let mut last = harness.runs.load(Ordering::SeqCst);
    let mut unchanged_since = Instant::now();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        let now = harness.runs.load(Ordering::SeqCst);
        if now != last {
            last = now;
            unchanged_since = Instant::now();
        } else if unchanged_since.elapsed() > SETTLE * 4 {
            return; // Quiet for well over a settle window: nothing more is coming.
        }
    }
}

#[test]
fn a_move_run_with_a_live_watcher_costs_exactly_one_extra_no_op_run() {
    let source = TempDir::new().unwrap();
    let destination = TempDir::new().unwrap();
    seed(&source);
    let harness = harness(&source, &destination, SyncStrategy::Move);

    let mut watcher = WatchTriggerSource::new(
        Arc::new(PhysicalFileSystem::new()),
        source.path(),
        SETTLE,
        Arc::new(RunOnFire(harness.clone())),
    );
    watcher.start().unwrap();

    harness.drive();
    settle(&harness);
    watcher.stop();

    assert_eq!(
        3,
        harness.files_moved.load(Ordering::SeqCst),
        "the first run moves everything and the follow-up finds nothing left"
    );
    assert_eq!(
        2,
        harness.runs.load(Ordering::SeqCst),
        "a Move run empties the source its own watcher is watching: one extra run, and it does \
         not cascade"
    );
    assert!(destination.path().join("a.txt").is_file());
    assert!(destination.path().join("sub/b.txt").is_file());
    assert!(!source.path().join("a.txt").exists());
}

#[test]
fn a_mirror_run_does_not_retrigger_itself_at_all() {
    let source = TempDir::new().unwrap();
    let destination = TempDir::new().unwrap();
    seed(&source);
    let harness = harness(&source, &destination, SyncStrategy::Mirror);

    let mut watcher = WatchTriggerSource::new(
        Arc::new(PhysicalFileSystem::new()),
        source.path(),
        SETTLE,
        Arc::new(RunOnFire(harness.clone())),
    );
    watcher.start().unwrap();

    harness.drive();
    settle(&harness);
    watcher.stop();

    assert_eq!(
        1,
        harness.runs.load(Ordering::SeqCst),
        "Mirror only reads the source, so there is nothing for its watcher to notice"
    );
    assert_eq!(3, harness.files_moved.load(Ordering::SeqCst));
}

#[test]
fn a_burst_of_source_changes_collapses_into_one_run() {
    let source = TempDir::new().unwrap();
    let destination = TempDir::new().unwrap();
    std::fs::create_dir_all(source.path()).unwrap();
    let harness = harness(&source, &destination, SyncStrategy::Mirror);

    let mut watcher = WatchTriggerSource::new(
        Arc::new(PhysicalFileSystem::new()),
        source.path(),
        SETTLE,
        Arc::new(RunOnFire(harness.clone())),
    );
    watcher.start().unwrap();

    // The shape a program saving several files in a row produces.
    for index in 0..6 {
        std::fs::write(source.path().join(format!("f{index}.txt")), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }

    settle(&harness);
    watcher.stop();

    assert_eq!(
        1,
        harness.runs.load(Ordering::SeqCst),
        "every fresh change restarts the quiet period, so one burst is one run"
    );
    assert_eq!(6, harness.files_moved.load(Ordering::SeqCst));
}
