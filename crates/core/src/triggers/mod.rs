//! How a sync task is initiated: the persisted trigger data, and the runners that turn it into
//! `fired` events.
//!
//! Every runner delivers through [`TriggerNotifier`], which is what keeps the one contract they
//! all share: decide under the owner's state gate, deliver outside it, in decided order, and
//! never after `stop` returns.
//!
//! **A trigger stays live across its own task's run.** Nothing calls `stop` around a run. For a
//! Move task — the only kind that mutates its own source — that costs exactly one extra run
//! afterwards, which is a planner no-op and does not cascade. Mirror and Add-only never
//! self-trigger at all. Suppressing the trigger around runs would save one tree walk and add a
//! resume-failure path that has to surface as the card's trigger-error badge; it was weighed
//! and declined.

mod cron;
mod factory;
mod notifier;
mod polling;
mod scheduled;
mod snapshot;
mod source;
mod watch;

pub use cron::{CronParseError, CronSchedule};
pub use factory::{DefaultTriggerSourceFactory, TriggerSourceFactory};
pub use notifier::{Notification, TriggerNotifier, TriggerObserver};
pub use polling::{PollingWatchTriggerSource, DEFAULT_INTERVAL as DEFAULT_POLL_INTERVAL};
pub use scheduled::{ManualTriggerSource, Schedule, ScheduledTriggerSource};
pub use snapshot::TreeSnapshot;
pub use source::{TriggerError, TriggerSource};
pub use watch::WatchTriggerSource;

use serde::{Deserialize, Serialize};

/// Default quiet period for a watch trigger — long enough for typical multi-file save bursts.
pub const DEFAULT_SETTLE_SECONDS: i32 = 10;
/// Lower bound for the editor and for clamping hand-edited config.
pub const MIN_SETTLE_SECONDS: i32 = 1;
/// Upper bound for the editor and for clamping hand-edited config.
pub const MAX_SETTLE_SECONDS: i32 = 600;

/// A closed hierarchy so the persisted JSON shape is a fixed contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "PascalCase")]
pub enum Trigger {
    /// The task runs only when the user asks it to.
    #[serde(rename = "manual")]
    Manual,

    /// The task runs on a schedule described by a five-field cron expression, in local time.
    #[serde(rename = "scheduled")]
    Scheduled { cron_expression: String },

    /// The task runs whenever the source changes, once the source has been quiet for
    /// `settle_seconds`.
    ///
    /// Change notifications only say *something* changed, and programs write in bursts (an
    /// image, then its thumbnail, then metadata), so every fresh change restarts the wait and
    /// one burst syncs as one run.
    #[serde(rename = "watch")]
    Watch {
        #[serde(default = "default_settle_seconds")]
        settle_seconds: i32,
    },
}

fn default_settle_seconds() -> i32 {
    DEFAULT_SETTLE_SECONDS
}

impl Trigger {
    /// A watch trigger with the default quiet period.
    pub fn watch() -> Self {
        Self::Watch {
            settle_seconds: DEFAULT_SETTLE_SECONDS,
        }
    }

    /// A scheduled trigger. The expression is validated when the schedule is built, not here.
    pub fn scheduled(cron_expression: impl Into<String>) -> Self {
        Self::Scheduled {
            cron_expression: cron_expression.into(),
        }
    }

    /// The quiet period of a watch trigger, clamped to the valid range so hand-edited config
    /// cannot produce a zero or absurd window. `None` for other trigger kinds.
    pub fn settle_window(&self) -> Option<std::time::Duration> {
        match self {
            Self::Watch { settle_seconds } => Some(std::time::Duration::from_secs(
                (*settle_seconds).clamp(MIN_SETTLE_SECONDS, MAX_SETTLE_SECONDS) as u64,
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_round_trip_through_the_legacy_json_shape() {
        let cases = [
            (Trigger::Manual, r#"{"kind":"manual"}"#),
            (
                Trigger::scheduled("0 2 * * *"),
                r#"{"kind":"scheduled","CronExpression":"0 2 * * *"}"#,
            ),
            (Trigger::watch(), r#"{"kind":"watch","SettleSeconds":10}"#),
        ];

        for (trigger, json) in cases {
            assert_eq!(json, serde_json::to_string(&trigger).unwrap());
            assert_eq!(trigger, serde_json::from_str::<Trigger>(json).unwrap());
        }
    }

    #[test]
    fn a_bare_legacy_watch_trigger_loads_with_the_default_settle_period() {
        let trigger: Trigger = serde_json::from_str(r#"{"kind":"watch"}"#).unwrap();
        assert_eq!(Trigger::watch(), trigger);
    }

    #[test]
    fn a_hand_edited_settle_period_is_clamped_rather_than_rejected() {
        let absurd: Trigger =
            serde_json::from_str(r#"{"kind":"watch","SettleSeconds":99999}"#).unwrap();
        assert_eq!(
            Some(std::time::Duration::from_secs(600)),
            absurd.settle_window()
        );

        let zero: Trigger = serde_json::from_str(r#"{"kind":"watch","SettleSeconds":0}"#).unwrap();
        assert_eq!(
            Some(std::time::Duration::from_secs(1)),
            zero.settle_window()
        );
    }

    #[test]
    fn only_a_watch_trigger_has_a_settle_window() {
        assert_eq!(None, Trigger::Manual.settle_window());
        assert_eq!(None, Trigger::scheduled("* * * * *").settle_window());
    }
}
