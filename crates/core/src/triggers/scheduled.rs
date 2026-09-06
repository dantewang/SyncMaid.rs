//! The manual and cron-driven trigger runners.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::{DateTime, Local};

use crate::triggers::{
    CronSchedule, Notification, TriggerError, TriggerNotifier, TriggerObserver, TriggerSource,
};

/// When the next run is due.
///
/// A seam, so the worker's timing behaviour can be tested in milliseconds instead of waiting
/// for a real cron occurrence.
pub trait Schedule: Send + Sync {
    /// The next moment strictly after `after`, or `None` when there is no future occurrence.
    fn next_occurrence_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>>;
}

impl Schedule for CronSchedule {
    fn next_occurrence_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        CronSchedule::next_occurrence_after(self, after)
    }
}

/// Nothing runs in the background; `Run now` is the only path.
pub struct ManualTriggerSource {
    notifier: Arc<TriggerNotifier>,
}

impl ManualTriggerSource {
    pub fn new(observer: Arc<dyn TriggerObserver>) -> Self {
        Self {
            notifier: Arc::new(TriggerNotifier::new(observer)),
        }
    }

    /// Fires once, for anything that drives a manual task programmatically.
    pub fn fire(&self) {
        self.notifier.enqueue(Notification::Fired);
        self.notifier.drain();
    }
}

impl TriggerSource for ManualTriggerSource {
    fn start(&mut self) -> Result<(), TriggerError> {
        Ok(())
    }

    fn stop(&mut self) {}
}

/// Fires on a schedule, in local wall-clock time.
///
/// One worker waits until the next occurrence and recomputes afterwards, so a long-running
/// schedule never accumulates drift. A wakeup that arrives early re-arms instead of firing; one
/// that arrives late — the machine slept through several occurrences — fires **once** and
/// re-arms ahead, because missed runs are not queued.
pub struct ScheduledTriggerSource {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    schedule: Arc<dyn Schedule>,
    notifier: TriggerNotifier,
    state: Mutex<bool>,
    wake: Condvar,
}

/// How long to park when the schedule has no future occurrence at all. The worker still has to
/// notice a stop.
const NO_OCCURRENCE_PARK: Duration = Duration::from_secs(60);

impl ScheduledTriggerSource {
    pub fn new(schedule: CronSchedule, observer: Arc<dyn TriggerObserver>) -> Self {
        Self::with_schedule(Arc::new(schedule), observer)
    }

    pub fn with_schedule(schedule: Arc<dyn Schedule>, observer: Arc<dyn TriggerObserver>) -> Self {
        Self {
            shared: Arc::new(Shared {
                schedule,
                notifier: TriggerNotifier::new(observer),
                state: Mutex::new(false),
                wake: Condvar::new(),
            }),
            worker: None,
        }
    }

    /// The next time this will fire, for the card's "next run in 2 h" badge.
    pub fn next_occurrence(&self) -> Option<DateTime<Local>> {
        self.shared.schedule.next_occurrence_after(Local::now())
    }
}

impl TriggerSource for ScheduledTriggerSource {
    fn start(&mut self) -> Result<(), TriggerError> {
        if self.worker.is_some() {
            return Ok(());
        }

        *lock(&self.shared.state) = true;
        let shared = Arc::clone(&self.shared);
        let worker = std::thread::Builder::new()
            .name("syncmaid-schedule".into())
            .spawn(move || shared.run())
            .map_err(|error| TriggerError::new(format!("could not start the schedule: {error}")))?;

        self.worker = Some(worker);
        Ok(())
    }

    fn stop(&mut self) {
        {
            // Decided under the state gate, so nothing queued before now can deliver after.
            let mut running = lock(&self.shared.state);
            *running = false;
            self.shared.notifier.invalidate();
        }
        self.shared.wake.notify_all();

        // The quiescence barrier: a delivery already in flight finishes before stop returns.
        self.shared.notifier.wait_for_idle();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ScheduledTriggerSource {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    fn run(&self) {
        loop {
            let now = Local::now();
            let Some(next) = self.schedule.next_occurrence_after(now) else {
                if !self.park(NO_OCCURRENCE_PARK) {
                    return;
                }
                continue;
            };

            // A `next` already in the past saturates to zero, which is what makes a missed
            // occurrence fire immediately rather than waiting a negative amount of time.
            let wait = (next - now).to_std().unwrap_or(Duration::ZERO);
            if !self.park(wait) {
                return;
            }

            // An early wakeup re-arms rather than firing: the occurrence has not arrived, and
            // running now would be the wrong wall-clock time.
            if Local::now() < next {
                continue;
            }

            {
                let running = lock(&self.state);
                if !*running {
                    return;
                }
                self.notifier.enqueue(Notification::Fired);
            }
            self.notifier.drain();

            // Recomputed from *now*, so a machine that slept through several occurrences fires
            // once and picks up ahead rather than working through the backlog.
        }
    }

    /// Waits up to `duration`, returning false once stopped.
    fn park(&self, duration: Duration) -> bool {
        let mut running = lock(&self.state);
        if !*running {
            return false;
        }
        if duration.is_zero() {
            return true;
        }
        let (guard, _) = self
            .wake
            .wait_timeout(running, duration)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        running = guard;
        *running
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::*;

    fn channel_observer() -> (Arc<dyn TriggerObserver>, mpsc::Receiver<Notification>) {
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let observer = move |notification: Notification| {
            let _ = lock(&sender).send(notification);
        };
        (Arc::new(observer), receiver)
    }

    /// One occurrence at a fixed moment, then quiet — the shape of "one run, then nothing".
    ///
    /// The moment is pinned on the first question and handed back to every later one, the way a
    /// real schedule answers. The worker re-arms after an early or spurious wakeup by asking
    /// again, and a stub that invented a fresh occurrence per question would push the run a day
    /// out and lose it.
    struct FiresSoon {
        delay: chrono::Duration,
        occurrence: Mutex<Option<DateTime<Local>>>,
    }

    impl Schedule for FiresSoon {
        fn next_occurrence_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
            let mut occurrence = lock(&self.occurrence);
            let fires_at = *occurrence.get_or_insert(after + self.delay);
            if after < fires_at {
                Some(fires_at)
            } else {
                // Asked again once the occurrence has passed: far enough away that the worker
                // just parks, so a run that already fired is never handed out twice.
                Some(after + chrono::Duration::hours(24))
            }
        }
    }

    fn fires_soon(delay_ms: i64) -> Arc<FiresSoon> {
        Arc::new(FiresSoon {
            delay: chrono::Duration::milliseconds(delay_ms),
            occurrence: Mutex::new(None),
        })
    }

    /// One occurrence that is already overdue when the worker first asks — the shape of a machine
    /// waking from sleep. The worker fires straight away without re-arming, so answering by count
    /// is safe here.
    struct FiresLate {
        overdue_by: chrono::Duration,
        asked: AtomicUsize,
    }

    impl Schedule for FiresLate {
        fn next_occurrence_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
            if self.asked.fetch_add(1, Ordering::SeqCst) == 0 {
                Some(after - self.overdue_by)
            } else {
                // Far enough away that the worker just parks.
                Some(after + chrono::Duration::hours(24))
            }
        }
    }

    fn fires_late(overdue_ms: i64) -> Arc<FiresLate> {
        Arc::new(FiresLate {
            overdue_by: chrono::Duration::milliseconds(overdue_ms),
            asked: AtomicUsize::new(0),
        })
    }

    #[test]
    fn a_manual_trigger_fires_only_when_asked() {
        let (observer, notifications) = channel_observer();
        let mut source = ManualTriggerSource::new(observer);
        source.start().unwrap();

        assert!(
            notifications.try_recv().is_err(),
            "nothing runs in the background"
        );

        source.fire();
        assert_eq!(Notification::Fired, notifications.try_recv().unwrap());

        source.stop();
    }

    #[test]
    fn a_schedule_fires_when_its_occurrence_arrives() {
        let (observer, notifications) = channel_observer();
        let mut source = ScheduledTriggerSource::with_schedule(fires_soon(60), observer);

        source.start().unwrap();
        let fired = notifications.recv_timeout(Duration::from_secs(5));
        source.stop();

        assert_eq!(Ok(Notification::Fired), fired);
        assert!(notifications.try_recv().is_err(), "one occurrence, one run");
    }

    #[test]
    fn an_occurrence_missed_while_the_machine_slept_fires_once_and_rearms_ahead() {
        let (observer, notifications) = channel_observer();
        // The occurrence is already in the past, as it would be after a laptop wakes up.
        let mut source = ScheduledTriggerSource::with_schedule(fires_late(90_000), observer);

        source.start().unwrap();
        let fired = notifications.recv_timeout(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(80));
        source.stop();

        assert_eq!(Ok(Notification::Fired), fired);
        assert!(
            notifications.try_recv().is_err(),
            "missed runs are not queued: a sleeping machine costs one run, not a backlog"
        );
    }

    #[test]
    fn stopping_before_an_occurrence_delivers_nothing() {
        let (observer, notifications) = channel_observer();
        let mut source = ScheduledTriggerSource::with_schedule(fires_soon(60_000), observer);

        source.start().unwrap();
        source.stop();

        assert!(notifications.try_recv().is_err());
    }

    #[test]
    fn stop_returns_only_after_a_fire_in_flight_has_quiesced() {
        let entered = Arc::new(Mutex::new(false));
        let finished = Arc::new(Mutex::new(false));
        let observer = {
            let entered = Arc::clone(&entered);
            let finished = Arc::clone(&finished);
            Arc::new(move |_: Notification| {
                *entered.lock().unwrap() = true;
                std::thread::sleep(Duration::from_millis(120));
                *finished.lock().unwrap() = true;
            })
        };

        let mut source = ScheduledTriggerSource::with_schedule(fires_soon(40), observer);
        source.start().unwrap();

        // Wait until the delivery is definitely running, then stop mid-flight.
        while !*entered.lock().unwrap() {
            std::thread::sleep(Duration::from_millis(5));
        }
        source.stop();

        assert!(
            *finished.lock().unwrap(),
            "stop must not return while a notification is still being delivered"
        );
    }

    #[test]
    fn stopping_a_source_that_never_started_is_harmless() {
        let (observer, _) = channel_observer();
        let mut source = ScheduledTriggerSource::with_schedule(fires_soon(60_000), observer);

        source.stop();
        source.stop();
    }

    #[test]
    fn a_schedule_with_no_future_occurrence_parks_instead_of_spinning() {
        struct Never;
        impl Schedule for Never {
            fn next_occurrence_after(&self, _: DateTime<Local>) -> Option<DateTime<Local>> {
                None
            }
        }

        let (observer, notifications) = channel_observer();
        let mut source = ScheduledTriggerSource::with_schedule(Arc::new(Never), observer);

        source.start().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        source.stop();

        assert!(
            notifications.try_recv().is_err(),
            "the 30th of February never comes"
        );
    }

    #[test]
    fn a_real_cron_schedule_reports_its_next_occurrence_for_the_card_badge() {
        let (observer, _) = channel_observer();
        let source =
            ScheduledTriggerSource::new(CronSchedule::parse("0 2 * * *").unwrap(), observer);

        assert!(source.next_occurrence().is_some());
    }
}
