//! What every trigger runner has in common.

/// Starts and stops a trigger.
///
/// The contract the notifier exists to keep: notifications are delivered **serially, in
/// decision order, never under the source's own lock, and never after [`TriggerSource::stop`]
/// returns** — a delivery already in flight completes before stop returns.
///
/// Nothing in the app calls `stop` around a run. A task's trigger stays live across its own
/// run, which costs a Move task exactly one extra no-op run afterwards and nothing at all for
/// the copying strategies. Suppressing the trigger instead would save one tree walk and add a
/// resume-failure path that has to surface as the card's trigger-error badge.
pub trait TriggerSource: Send {
    /// Begins watching. A failure here degrades the task to manual-only and shows on the card.
    fn start(&mut self) -> Result<(), TriggerError>;

    /// Stops watching. Returns only once any in-flight notification has been delivered.
    fn stop(&mut self);
}

/// The trigger is not working.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TriggerError(pub String);

impl TriggerError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
