//! What a task card says about itself in one line.
//!
//! A task has several destinations, each with its own outcome, and the card shows one summary.
//! The ladder below is a **precedence order, not a tally**: anything that needs the user comes
//! before anything that is merely working, and anything failing comes before anything that is
//! only behind. Getting this order wrong hides a failure behind a success.

use std::collections::HashMap;

use syncmaid_core::model::{DestinationSyncStatus, SyncOutcome, SyncTask};
use uuid::Uuid;

/// The card's one-line summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    /// Which colour and glyph the row takes.
    pub outcome: SyncOutcome,
    /// The sentence, already assembled.
    pub text: String,
}

/// Summarises a task from its destinations' last outcomes.
pub fn health_of(task: &SyncTask, statuses: &HashMap<Uuid, DestinationSyncStatus>) -> Health {
    if task.destinations.is_empty() {
        return Health {
            outcome: SyncOutcome::Never,
            text: "No destinations".into(),
        };
    }

    let outcomes: Vec<SyncOutcome> = task
        .destinations
        .iter()
        .map(|destination| {
            statuses
                .get(&destination.id)
                .map_or(SyncOutcome::Never, |status| status.outcome)
        })
        .collect();

    let count = |wanted: SyncOutcome| outcomes.iter().filter(|o| **o == wanted).count();
    let total = outcomes.len();

    // The ladder, worst first.
    if count(SyncOutcome::Running) > 0 {
        return Health {
            outcome: SyncOutcome::Running,
            text: "Syncing…".into(),
        };
    }
    if count(SyncOutcome::NeedsConfirmation) > 0 {
        return Health {
            outcome: SyncOutcome::NeedsConfirmation,
            text: "Needs confirmation".into(),
        };
    }
    let failed = count(SyncOutcome::Failed);
    if failed > 0 {
        return Health {
            outcome: SyncOutcome::Failed,
            text: format!("{failed} of {total} failed"),
        };
    }
    if count(SyncOutcome::Incomplete) > 0 {
        let in_use: i32 = task
            .destinations
            .iter()
            .filter_map(|destination| statuses.get(&destination.id))
            .map(|status| status.files_deferred)
            .sum();
        return Health {
            outcome: SyncOutcome::Incomplete,
            text: format!("{in_use} in use"),
        };
    }
    if count(SyncOutcome::Success) == total {
        return Health {
            outcome: SyncOutcome::Success,
            text: "All synced".into(),
        };
    }
    if count(SyncOutcome::Success) > 0 {
        // Some destinations have run and some have not — synced, but not all the way.
        return Health {
            outcome: SyncOutcome::Success,
            text: "Partly synced".into(),
        };
    }

    Health {
        outcome: SyncOutcome::Never,
        text: "Never run".into(),
    }
}

#[cfg(test)]
mod tests {
    use syncmaid_core::filtering::FilterRule;
    use syncmaid_core::model::{Destination, SyncStrategy};
    use syncmaid_core::triggers::Trigger;

    use super::*;

    fn task_with(count: usize) -> SyncTask {
        let destinations = (0..count)
            .map(|index| {
                Destination::new(
                    format!("D{index}"),
                    format!(r"D:\d{index}"),
                    [FilterRule::AllFiles],
                    SyncStrategy::Mirror,
                )
            })
            .collect::<Vec<_>>();
        SyncTask::new("T", r"C:\src", Trigger::Manual, destinations)
    }

    fn statuses(task: &SyncTask, outcomes: &[SyncOutcome]) -> HashMap<Uuid, DestinationSyncStatus> {
        task.destinations
            .iter()
            .zip(outcomes)
            .map(|(destination, outcome)| {
                (
                    destination.id,
                    DestinationSyncStatus::new(destination.id, *outcome),
                )
            })
            .collect()
    }

    #[test]
    fn a_task_with_no_destinations_says_so() {
        let health = health_of(&task_with(0), &HashMap::new());
        assert_eq!(SyncOutcome::Never, health.outcome);
        assert_eq!("No destinations", health.text);
    }

    #[test]
    fn a_destination_that_has_never_run_reads_as_never_run() {
        let task = task_with(2);
        assert_eq!("Never run", health_of(&task, &HashMap::new()).text);
    }

    #[test]
    fn running_outranks_everything_else() {
        let task = task_with(3);
        let outcomes = [
            SyncOutcome::Failed,
            SyncOutcome::Running,
            SyncOutcome::Success,
        ];
        assert_eq!(
            SyncOutcome::Running,
            health_of(&task, &statuses(&task, &outcomes)).outcome
        );
    }

    #[test]
    fn needing_confirmation_outranks_a_failure() {
        let task = task_with(2);
        let outcomes = [SyncOutcome::Failed, SyncOutcome::NeedsConfirmation];
        let health = health_of(&task, &statuses(&task, &outcomes));
        assert_eq!(SyncOutcome::NeedsConfirmation, health.outcome);
        assert_eq!("Needs confirmation", health.text);
    }

    #[test]
    fn a_failure_outranks_a_merely_incomplete_run_and_counts_itself() {
        let task = task_with(3);
        let outcomes = [
            SyncOutcome::Failed,
            SyncOutcome::Incomplete,
            SyncOutcome::Success,
        ];
        let health = health_of(&task, &statuses(&task, &outcomes));
        assert_eq!(SyncOutcome::Failed, health.outcome);
        assert_eq!("1 of 3 failed", health.text);
    }

    #[test]
    fn files_in_use_are_summed_across_destinations() {
        let task = task_with(2);
        let mut map = statuses(&task, &[SyncOutcome::Incomplete, SyncOutcome::Incomplete]);
        for (index, destination) in task.destinations.iter().enumerate() {
            map.get_mut(&destination.id).unwrap().files_deferred = index as i32 + 1;
        }

        let health = health_of(&task, &map);

        assert_eq!(SyncOutcome::Incomplete, health.outcome);
        assert_eq!("3 in use", health.text);
    }

    #[test]
    fn every_destination_succeeding_is_all_synced() {
        let task = task_with(2);
        let outcomes = [SyncOutcome::Success, SyncOutcome::Success];
        assert_eq!(
            "All synced",
            health_of(&task, &statuses(&task, &outcomes)).text
        );
    }

    #[test]
    fn some_succeeding_and_some_not_yet_run_is_partly_synced() {
        let task = task_with(2);
        let outcomes = [SyncOutcome::Success, SyncOutcome::Never];
        let health = health_of(&task, &statuses(&task, &outcomes));
        assert_eq!(SyncOutcome::Success, health.outcome);
        assert_eq!("Partly synced", health.text);
    }
}
