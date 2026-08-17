//! What an approved mass deletion is worth, and for how long.
//!
//! The engine's own tests prove the guard holds and that a confirmed id lets that destination
//! through. What these pin is the wiring around it — the shape the window drives: **an approval
//! is scoped to one destination and one run, it is unioned rather than replaced when requests
//! coalesce, and it is never persisted.** Getting any of those wrong turns one deliberate "yes"
//! into a standing licence to delete.

use std::collections::HashSet;

use syncmaid::state::RunGate;
use syncmaid_core::filtering::FilterRule;
use syncmaid_core::io::PhysicalFileSystem;
use syncmaid_core::model::{Destination, SyncOutcome, SyncStrategy, SyncTask};
use syncmaid_core::sync::SyncEngine;
use syncmaid_core::triggers::Trigger;
use tempfile::TempDir;
use uuid::Uuid;

/// A source with two files and a destination with twenty, so the deletion is well over any
/// threshold and the destination is large enough for the ratio guard to apply at all.
fn scene() -> (TempDir, TempDir, SyncTask) {
    let source = TempDir::new().expect("source");
    let destination = TempDir::new().expect("destination");

    for index in 0..2 {
        std::fs::write(source.path().join(format!("keep{index}.txt")), b"kept").expect("write");
    }
    for index in 0..20 {
        std::fs::write(
            destination.path().join(format!("orphan{index}.txt")),
            b"orphan",
        )
        .expect("write");
    }
    // The two the source still has, so they are not counted as deletions.
    for index in 0..2 {
        std::fs::write(destination.path().join(format!("keep{index}.txt")), b"kept")
            .expect("write");
    }

    let mut mirror = Destination::new(
        "Archive",
        destination.path().to_string_lossy().into_owned(),
        [FilterRule::AllFiles],
        SyncStrategy::Mirror,
    );
    // Permanent, so the test does not fill the user's Recycle Bin.
    mirror.delete_mode = syncmaid_core::model::DeleteMode::Permanent;

    let task = SyncTask::new(
        "Archive",
        source.path().to_string_lossy().into_owned(),
        Trigger::Manual,
        vec![mirror],
    );
    (source, destination, task)
}

fn engine() -> SyncEngine {
    SyncEngine::local(std::sync::Arc::new(PhysicalFileSystem::new()))
}

/// One pass of the drain loop the window owns.
fn run(
    engine: &SyncEngine,
    gate: &RunGate,
    task: &SyncTask,
    approved: HashSet<Uuid>,
) -> Vec<SyncOutcome> {
    let Some(mut start) = gate.request(approved) else {
        return Vec::new(); // Absorbed by an active run; its drain will pick it up.
    };

    let mut outcomes = Vec::new();
    loop {
        let statuses = engine
            .execute(
                task,
                &start.cancellation,
                &mut |_| {},
                &start.confirmed_mass_deletes,
            )
            .expect("not cancelled");
        outcomes.extend(statuses.iter().map(|status| status.outcome));

        match gate.next() {
            Some(next) => start = next,
            None => return outcomes,
        }
    }
}

fn files_in(directory: &TempDir) -> usize {
    std::fs::read_dir(directory.path())
        .expect("read")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .count()
}

#[test]
fn an_unapproved_mass_deletion_holds_the_run_and_touches_nothing() {
    let (_source, destination, task) = scene();
    let gate = RunGate::new();

    let outcomes = run(&engine(), &gate, &task, HashSet::new());

    assert_eq!(vec![SyncOutcome::NeedsConfirmation], outcomes);
    assert_eq!(22, files_in(&destination), "not one file was removed");
}

#[test]
fn an_approval_is_spent_by_the_run_it_was_given_for() {
    // The whole point of the one-shot: tomorrow's run asks again, because nothing about the
    // approval was written down.
    let (source, destination, task) = scene();
    let gate = RunGate::new();
    let engine = engine();
    let approved = HashSet::from([task.destinations[0].id]);

    let first = run(&engine, &gate, &task, approved);
    assert_eq!(vec![SyncOutcome::Success], first);
    assert_eq!(2, files_in(&destination), "the orphans went");

    // Make it a mass deletion again and ask for a plain run.
    for index in 0..20 {
        std::fs::write(
            destination.path().join(format!("orphan{index}.txt")),
            b"orphan",
        )
        .expect("write");
    }
    // Two files the source still holds keeps the ratio a deletion rather than an empty source.
    assert_eq!(2, files_in(&source));

    let second = run(&engine, &gate, &task, HashSet::new());

    assert_eq!(
        vec![SyncOutcome::NeedsConfirmation],
        second,
        "the previous yes does not carry over"
    );
    assert_eq!(22, files_in(&destination));
}

#[test]
fn an_approval_for_another_destination_is_not_this_ones() {
    // Scoped by id, so approving one destination's deletions cannot quietly authorise a
    // sibling's.
    let (_source, destination, task) = scene();
    let gate = RunGate::new();

    let outcomes = run(&engine(), &gate, &task, HashSet::from([Uuid::new_v4()]));

    assert_eq!(vec![SyncOutcome::NeedsConfirmation], outcomes);
    assert_eq!(22, files_in(&destination));
}

#[test]
fn an_approval_given_while_a_run_is_active_survives_into_the_run_that_performs_it() {
    // Requests coalesce, and the gate unions their approvals rather than replacing them: an
    // approval given for one request must survive being folded into the run that acts on it.
    let (_source, destination, task) = scene();
    let gate = RunGate::new();
    let approved = task.destinations[0].id;

    // Take the gate as an active run would, then let a second request land behind it.
    let first = gate.request(HashSet::new()).expect("the gate was free");
    assert!(first.confirmed_mass_deletes.is_empty());
    assert!(
        gate.request(HashSet::from([approved])).is_none(),
        "absorbed into the follow-up"
    );

    let follow_up = gate.next().expect("a coalesced request is waiting");
    assert_eq!(
        HashSet::from([approved]),
        follow_up.confirmed_mass_deletes,
        "the approval reaches the run that will actually delete"
    );

    let statuses = engine()
        .execute(
            &task,
            &follow_up.cancellation,
            &mut |_| {},
            &follow_up.confirmed_mass_deletes,
        )
        .expect("not cancelled");

    assert_eq!(SyncOutcome::Success, statuses[0].outcome);
    assert_eq!(2, files_in(&destination));
}
