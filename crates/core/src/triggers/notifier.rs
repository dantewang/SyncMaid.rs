//! Serialized, order-preserving delivery of trigger notifications.
//!
//! One shared pattern instead of four spot fixes: **decide under the owner's state gate,
//! deliver outside it, in decided order.** Owners enqueue while holding their gate and drain
//! after releasing it. That buys three guarantees:
//!
//! - a subscriber never runs under the owner's state gate, so a slow handler cannot block a
//!   watcher callback or a state transition;
//! - deliveries are serialized and land in enqueue order, so an `Error` and a `Recovered`
//!   decided in sequence can never be observed crossed;
//! - [`TriggerNotifier::invalidate`] plus [`TriggerNotifier::wait_for_idle`] preserve the stop
//!   contract — entries decided before a stop never deliver after it, and a delivery already
//!   in flight completes before stop returns.
//!
//! A subscriber may call the owner's stop from inside its own delivery: the idle barrier
//! recognizes the draining thread and passes straight through instead of deadlocking.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;

/// What a trigger tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    /// Run the task.
    Fired,
    /// The trigger is not working; the reason is English, from the OS.
    Error(String),
    /// It is working again.
    Recovered,
}

/// Receives notifications, outside any of the trigger's own locks.
pub trait TriggerObserver: Send + Sync {
    fn notify(&self, notification: Notification);
}

impl<F> TriggerObserver for F
where
    F: Fn(Notification) + Send + Sync,
{
    fn notify(&self, notification: Notification) {
        self(notification)
    }
}

/// See the module docs.
pub struct TriggerNotifier {
    observer: Arc<dyn TriggerObserver>,
    queue: Mutex<QueueState>,
    delivery: Mutex<Option<ThreadId>>,
    idle: Condvar,
}

#[derive(Default)]
struct QueueState {
    pending: VecDeque<(u64, Notification)>,
    epoch: u64,
}

impl TriggerNotifier {
    pub fn new(observer: Arc<dyn TriggerObserver>) -> Self {
        Self {
            observer,
            queue: Mutex::new(QueueState::default()),
            delivery: Mutex::new(None),
            idle: Condvar::new(),
        }
    }

    /// Queues a notification under the current epoch.
    ///
    /// Call while holding the owner's state gate, so queue order matches decision order.
    pub fn enqueue(&self, notification: Notification) {
        let mut queue = self.lock_queue();
        let epoch = queue.epoch;
        queue.pending.push_back((epoch, notification));
    }

    /// Drops every queued-but-undelivered entry.
    ///
    /// Call from stop while holding the owner's state gate, so notifications decided before
    /// the transition can never deliver after it.
    pub fn invalidate(&self) {
        self.lock_queue().epoch += 1;
    }

    /// Delivers queued entries in order until the queue is empty.
    ///
    /// Returns immediately when a drain is already active on another thread: that drainer
    /// re-checks the queue after releasing the gate, so entries enqueued here are never
    /// stranded — and never delivered concurrently or out of order. Call only after releasing
    /// the owner's state gate.
    pub fn drain(&self) {
        loop {
            {
                let mut delivery = self.lock_delivery();
                if delivery.is_some() {
                    // Someone is draining — theirs, or ours from an outer frame. Either way
                    // that drain's re-check picks up what we queued.
                    return;
                }
                *delivery = Some(std::thread::current().id());
            }

            self.deliver_all();

            {
                let mut delivery = self.lock_delivery();
                *delivery = None;
                self.idle.notify_all();
            }

            if self.lock_queue().pending.is_empty() {
                return;
            }
            // Entries raced in between the empty check and the gate release; claim again.
        }
    }

    /// Blocks until any in-flight delivery completes — the stop barrier.
    ///
    /// Call after releasing the owner's state gate: a delivering subscriber may need that gate
    /// to finish.
    pub fn wait_for_idle(&self) {
        let current = std::thread::current().id();
        let mut delivery = self.lock_delivery();
        while let Some(drainer) = *delivery {
            if drainer == current {
                // A subscriber calling stop from inside its own delivery. Waiting for itself
                // would deadlock; the delivery it is inside is by definition still in flight.
                return;
            }
            delivery = self
                .idle
                .wait(delivery)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn deliver_all(&self) {
        loop {
            let notification = {
                let mut queue = self.lock_queue();
                let Some((epoch, notification)) = queue.pending.pop_front() else {
                    return;
                };
                if epoch != queue.epoch {
                    continue; // Stale: decided before an invalidate.
                }
                notification
            };

            // The drain runs on watcher and timer callbacks. Nothing may escape it — a panic
            // here would take down a thread the OS handed us.
            let _ = catch_unwind(AssertUnwindSafe(|| self.observer.notify(notification)));
        }
    }

    fn lock_queue(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_delivery(&self) -> std::sync::MutexGuard<'_, Option<ThreadId>> {
        self.delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// Records what it was told, in order.
    struct Recorder {
        seen: Mutex<Vec<Notification>>,
    }

    impl Recorder {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                seen: Mutex::new(Vec::new()),
            })
        }

        fn seen(&self) -> Vec<Notification> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl TriggerObserver for Recorder {
        fn notify(&self, notification: Notification) {
            self.seen.lock().unwrap().push(notification);
        }
    }

    fn error(message: &str) -> Notification {
        Notification::Error(message.into())
    }

    #[test]
    fn nothing_is_delivered_before_the_drain() {
        let recorder = Recorder::new();
        let notifier = TriggerNotifier::new(recorder.clone());

        notifier.enqueue(Notification::Fired);

        assert!(
            recorder.seen().is_empty(),
            "the owner is still holding its state gate"
        );

        notifier.drain();
        assert_eq!(vec![Notification::Fired], recorder.seen());
    }

    #[test]
    fn deliveries_land_in_the_order_they_were_decided() {
        let recorder = Recorder::new();
        let notifier = TriggerNotifier::new(recorder.clone());

        notifier.enqueue(error("the share went away"));
        notifier.enqueue(Notification::Recovered);
        notifier.enqueue(Notification::Fired);
        notifier.drain();

        assert_eq!(
            vec![
                error("the share went away"),
                Notification::Recovered,
                Notification::Fired
            ],
            recorder.seen(),
            "an Error and a Recovered decided in sequence can never be observed crossed"
        );
    }

    #[test]
    fn invalidate_drops_what_was_decided_before_it() {
        let recorder = Recorder::new();
        let notifier = TriggerNotifier::new(recorder.clone());

        notifier.enqueue(Notification::Fired);
        notifier.invalidate();
        notifier.drain();

        assert!(
            recorder.seen().is_empty(),
            "a fire decided before stop must not land after it"
        );
    }

    #[test]
    fn entries_after_an_invalidate_are_delivered_normally() {
        let recorder = Recorder::new();
        let notifier = TriggerNotifier::new(recorder.clone());

        notifier.enqueue(Notification::Fired);
        notifier.invalidate();
        notifier.enqueue(Notification::Recovered);
        notifier.drain();

        assert_eq!(vec![Notification::Recovered], recorder.seen());
    }

    #[test]
    fn a_delivery_may_enqueue_and_the_same_drain_delivers_it() {
        struct Reentrant {
            notifier: Mutex<Option<Arc<TriggerNotifier>>>,
            seen: Mutex<Vec<Notification>>,
        }

        impl TriggerObserver for Reentrant {
            fn notify(&self, notification: Notification) {
                self.seen.lock().unwrap().push(notification.clone());
                if notification == Notification::Error("first".into()) {
                    let guard = self.notifier.lock().unwrap();
                    let notifier = guard.as_ref().unwrap();
                    notifier.enqueue(Notification::Recovered);
                    // A nested drain must not deliver concurrently; the outer one picks it up.
                    notifier.drain();
                }
            }
        }

        let observer = Arc::new(Reentrant {
            notifier: Mutex::new(None),
            seen: Mutex::new(Vec::new()),
        });
        let notifier = Arc::new(TriggerNotifier::new(observer.clone()));
        *observer.notifier.lock().unwrap() = Some(notifier.clone());

        notifier.enqueue(error("first"));
        notifier.drain();

        assert_eq!(
            vec![error("first"), Notification::Recovered],
            observer.seen.lock().unwrap().clone()
        );
    }

    #[test]
    fn a_delivery_may_invalidate_and_wait_without_deadlock() {
        struct StopsFromInside {
            notifier: Mutex<Option<Arc<TriggerNotifier>>>,
            finished: Mutex<bool>,
        }

        impl TriggerObserver for StopsFromInside {
            fn notify(&self, _: Notification) {
                let guard = self.notifier.lock().unwrap();
                let notifier = guard.as_ref().unwrap();
                notifier.invalidate();
                notifier.wait_for_idle(); // Would deadlock without the same-thread check.
                *self.finished.lock().unwrap() = true;
            }
        }

        let observer = Arc::new(StopsFromInside {
            notifier: Mutex::new(None),
            finished: Mutex::new(false),
        });
        let notifier = Arc::new(TriggerNotifier::new(observer.clone()));
        *observer.notifier.lock().unwrap() = Some(notifier.clone());

        notifier.enqueue(Notification::Fired);
        notifier.drain();

        assert!(*observer.finished.lock().unwrap());
    }

    #[test]
    fn a_panicking_subscriber_does_not_escape_the_drain() {
        struct Panics;
        impl TriggerObserver for Panics {
            fn notify(&self, _: Notification) {
                panic!("a subscriber blew up");
            }
        }

        let notifier = TriggerNotifier::new(Arc::new(Panics));
        notifier.enqueue(Notification::Fired);

        // The drain runs on a watcher callback; a panic escaping it would take that thread out.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        notifier.drain();
        std::panic::set_hook(previous);
    }

    #[test]
    fn a_second_drainer_returns_instead_of_delivering_concurrently() {
        struct Blocks {
            entered: mpsc::Sender<()>,
            release: Mutex<mpsc::Receiver<()>>,
            deliveries: Mutex<usize>,
        }

        impl TriggerObserver for Blocks {
            fn notify(&self, _: Notification) {
                *self.deliveries.lock().unwrap() += 1;
                let _ = self.entered.send(());
                let _ = self.release.lock().unwrap().recv();
            }
        }

        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let observer = Arc::new(Blocks {
            entered,
            release: Mutex::new(release_rx),
            deliveries: Mutex::new(0),
        });
        let notifier = Arc::new(TriggerNotifier::new(observer.clone()));

        notifier.enqueue(Notification::Fired);
        let first = {
            let notifier = notifier.clone();
            std::thread::spawn(move || notifier.drain())
        };
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the first drain started");

        // A second drain arriving while the first is mid-delivery must not deliver anything.
        notifier.enqueue(Notification::Recovered);
        notifier.drain();
        assert_eq!(1, *observer.deliveries.lock().unwrap());

        release.send(()).unwrap();
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the re-check picked it up");
        release.send(()).unwrap();
        first.join().unwrap();

        assert_eq!(
            2,
            *observer.deliveries.lock().unwrap(),
            "nothing was stranded"
        );
    }
}
