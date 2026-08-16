//! Serializes a task's runs and coalesces bursts of trigger requests.
//!
//! Two runs of one task must never overlap: they would race on the same files, and a Mirror
//! destination would delete as orphans whatever the other just wrote. But requests do arrive in
//! bursts — a watch trigger fires, the user hits Run now, a schedule comes round — so a request
//! that lands mid-run is **absorbed into a single follow-up** rather than queued one-for-one.
//!
//! The gate is also why a task can trigger itself without cascading. A Move run mutates its own
//! source, so its live watcher fires once afterwards; that follow-up finds nothing to do,
//! because planning is idempotent, and the trigger goes quiet again. The cost is exactly one
//! extra no-op run and it does not compound.

use std::collections::HashSet;
use std::sync::Mutex;

use syncmaid_core::sync::CancellationToken;
use uuid::Uuid;

/// What a run needs to start.
#[derive(Debug, Clone)]
pub struct RunStart {
    /// Shared across a whole drain, so cancelling once stops the follow-ups too.
    pub cancellation: CancellationToken,
    /// The mass deletions the user approved, unioned across every request this run absorbed.
    ///
    /// One-shot: never persisted, and cleared once handed out.
    pub confirmed_mass_deletes: HashSet<Uuid>,
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct RunGate {
    state: Mutex<GateState>,
}

#[derive(Debug, Default)]
struct GateState {
    run_active: bool,
    has_pending_run: bool,
    /// Cleared when the task is being deleted or the app is shutting down.
    refusing: bool,
    /// Set by a cancel, so the drain stops instead of picking up the follow-up.
    cancelled_until_idle: bool,
    pending_confirmed_mass_deletes: HashSet<Uuid>,
    cancellation: Option<CancellationToken>,
}

impl RunGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks for a run.
    ///
    /// `Some` means the caller owns the drain loop and must keep calling [`RunGate::next`] until
    /// it returns `None`. `None` means an active run absorbed the request, or the gate is
    /// refusing.
    pub fn request(&self, confirmed_mass_deletes: HashSet<Uuid>) -> Option<RunStart> {
        let mut state = self.lock();
        if state.refusing {
            return None;
        }

        // Union rather than replace: an approval given for one request must survive being
        // folded into the run that actually performs it.
        state
            .pending_confirmed_mass_deletes
            .extend(confirmed_mass_deletes);

        if state.run_active {
            state.has_pending_run = true;
            return None;
        }

        state.run_active = true;
        state.cancelled_until_idle = false;
        let cancellation = CancellationToken::new();
        state.cancellation = Some(cancellation.clone());
        Some(RunStart {
            cancellation,
            confirmed_mass_deletes: std::mem::take(&mut state.pending_confirmed_mass_deletes),
        })
    }

    /// Called once a run finishes. `Some` when a coalesced request is waiting.
    pub fn next(&self) -> Option<RunStart> {
        let mut state = self.lock();
        if !state.has_pending_run || state.cancelled_until_idle {
            state.run_active = false;
            state.has_pending_run = false;
            state.cancellation = None;
            state.pending_confirmed_mass_deletes.clear();
            return None;
        }

        state.has_pending_run = false;
        // The same token: cancelling once has to stop the follow-ups as well.
        let cancellation = state.cancellation.clone().unwrap_or_default();
        Some(RunStart {
            cancellation,
            confirmed_mass_deletes: std::mem::take(&mut state.pending_confirmed_mass_deletes),
        })
    }

    /// Stops the current run and any follow-up it had absorbed.
    pub fn cancel(&self) {
        let mut state = self.lock();
        state.cancelled_until_idle = true;
        if let Some(cancellation) = &state.cancellation {
            cancellation.cancel();
        }
    }

    /// True while a run is in flight.
    pub fn is_running(&self) -> bool {
        self.lock().run_active
    }

    /// Refuses further requests — the task is being deleted, or the app is closing.
    pub fn refuse_further_requests(&self) {
        let mut state = self.lock();
        state.refusing = true;
        state.cancelled_until_idle = true;
        if let Some(cancellation) = &state.cancellation {
            cancellation.cancel();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(count: usize) -> Vec<Uuid> {
        (0..count).map(|_| Uuid::new_v4()).collect()
    }

    /// Drives the loop the way a caller does, counting how many runs actually happen.
    fn drain(gate: &RunGate, mut during_run: impl FnMut(usize)) -> Vec<RunStart> {
        let Some(mut start) = gate.request(HashSet::new()) else {
            return Vec::new();
        };
        let mut runs = Vec::new();
        loop {
            during_run(runs.len());
            runs.push(start.clone());
            match gate.next() {
                Some(next) => start = next,
                None => break,
            }
        }
        runs
    }

    #[test]
    fn a_single_request_runs_once() {
        let gate = RunGate::new();

        let runs = drain(&gate, |_| {});

        assert_eq!(1, runs.len());
        assert!(
            !gate.is_running(),
            "the gate is free again once the drain ends"
        );
    }

    #[test]
    fn a_request_that_lands_mid_run_is_absorbed_rather_than_refused() {
        let gate = RunGate::new();
        let start = gate
            .request(HashSet::new())
            .expect("the first request drives the drain");

        assert!(
            gate.request(HashSet::new()).is_none(),
            "the active run takes it"
        );
        assert!(gate.is_running());

        drop(start);
        assert!(gate.next().is_some(), "and the drain picks it up");
        assert!(gate.next().is_none());
    }

    #[test]
    fn a_burst_of_requests_costs_exactly_one_follow_up() {
        let gate = RunGate::new();

        let runs = drain(&gate, |index| {
            if index == 0 {
                // A watcher firing repeatedly while the first run is still going.
                for _ in 0..10 {
                    assert!(gate.request(HashSet::new()).is_none());
                }
            }
        });

        assert_eq!(
            2,
            runs.len(),
            "ten requests during a run are one follow-up, not ten — otherwise a busy folder \
             queues runs faster than they finish"
        );
    }

    #[test]
    fn an_approval_given_mid_run_survives_into_the_run_that_performs_it() {
        let gate = RunGate::new();
        let [first, second] = ids(2)[..] else {
            unreachable!()
        };

        let runs = drain(&gate, |index| {
            if index == 0 {
                gate.request(HashSet::from([first]));
                gate.request(HashSet::from([second]));
            }
        });

        assert_eq!(2, runs.len());
        assert!(runs[0].confirmed_mass_deletes.is_empty());
        assert_eq!(
            HashSet::from([first, second]),
            runs[1].confirmed_mass_deletes,
            "approvals are unioned, so neither confirmation is silently dropped"
        );
    }

    #[test]
    fn an_approval_is_one_shot() {
        let gate = RunGate::new();
        let approved = Uuid::new_v4();

        let start = gate.request(HashSet::from([approved])).unwrap();
        assert_eq!(HashSet::from([approved]), start.confirmed_mass_deletes);
        assert!(gate.next().is_none());

        let again = gate.request(HashSet::new()).unwrap();
        assert!(
            again.confirmed_mass_deletes.is_empty(),
            "a confirmation covers the run it was given for and no other"
        );
    }

    #[test]
    fn cancelling_stops_the_follow_up_as_well_as_the_run() {
        let gate = RunGate::new();

        let runs = drain(&gate, |index| {
            if index == 0 {
                gate.request(HashSet::new());
                gate.cancel();
            }
        });

        assert_eq!(
            1,
            runs.len(),
            "the absorbed request is dropped, not run after a stop"
        );
        assert!(runs[0].cancellation.is_cancelled());
        assert!(!gate.is_running());
    }

    #[test]
    fn every_run_in_a_drain_shares_one_cancellation() {
        let gate = RunGate::new();

        let runs = drain(&gate, |index| {
            if index == 0 {
                gate.request(HashSet::new());
            }
        });

        assert_eq!(2, runs.len());
        runs[0].cancellation.cancel();
        assert!(
            runs[1].cancellation.is_cancelled(),
            "otherwise stopping a task would only stop the run you happened to catch"
        );
    }

    #[test]
    fn a_refusing_gate_starts_nothing() {
        let gate = RunGate::new();
        gate.refuse_further_requests();

        assert!(gate.request(HashSet::new()).is_none());
    }

    #[test]
    fn refusing_mid_run_cancels_it_and_drops_the_follow_up() {
        let gate = RunGate::new();

        let runs = drain(&gate, |index| {
            if index == 0 {
                gate.request(HashSet::new());
                gate.refuse_further_requests();
            }
        });

        assert_eq!(1, runs.len());
        assert!(runs[0].cancellation.is_cancelled());
    }

    #[test]
    fn the_gate_is_reusable_after_a_cancelled_drain() {
        let gate = RunGate::new();
        drain(&gate, |index| {
            if index == 0 {
                gate.cancel();
            }
        });

        let restarted = gate
            .request(HashSet::new())
            .expect("a cancel is not a permanent refusal");
        assert!(!restarted.cancellation.is_cancelled());
    }
}
