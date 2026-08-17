//! Keeping every task's trigger running, and getting what it says onto the UI thread.
//!
//! A trigger runner fires from whatever thread its watcher or timer lives on, so nothing here
//! touches the view. Notifications go down a channel tagged with the task they came from, and
//! the window drains them where it can safely act on them.
//!
//! **A trigger that will not start degrades the task to manual-only rather than taking the app
//! down** — a bad cron expression or a source folder that is not there is a thing to say on the
//! card, not a reason to refuse to run. But it is said out loud: a task that silently never
//! runs is the worst of the three outcomes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use syncmaid_core::model::SyncTask;
use syncmaid_core::triggers::{
    Notification, Trigger, TriggerObserver, TriggerSource, TriggerSourceFactory,
};
use uuid::Uuid;

/// One notification, tagged with the task it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEvent {
    pub task_id: Uuid,
    pub notification: Notification,
}

/// How a (re)start went, for the card's trigger badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerStart {
    pub task_id: Uuid,
    /// The reason it will not run automatically, or `None` when it will.
    pub error: Option<String>,
}

/// A running trigger, with what it was built from — so a task edit that changed neither does
/// not needlessly tear down a watcher that is working.
struct Running {
    source: Box<dyn TriggerSource>,
    trigger: Trigger,
    source_path: String,
}

/// The live trigger runners, one per task.
pub struct TriggerHost {
    factory: Arc<dyn TriggerSourceFactory>,
    running: HashMap<Uuid, Running>,
    events: flume::Sender<TriggerEvent>,
}

impl TriggerHost {
    pub fn new(factory: Arc<dyn TriggerSourceFactory>) -> (Self, flume::Receiver<TriggerEvent>) {
        // Unbounded: a trigger must never block on a UI thread that is busy running the very
        // sync it asked for.
        let (events, receiver) = flume::unbounded();
        (
            Self {
                factory,
                running: HashMap::new(),
                events,
            },
            receiver,
        )
    }

    /// Starts what is new, restarts what changed, stops what is gone.
    ///
    /// Returns one entry per task it actually (re)started, so the caller can set or clear that
    /// task's badge and leave every other task's alone.
    pub fn reconcile(&mut self, tasks: &[SyncTask]) -> Vec<TriggerStart> {
        let live: Vec<Uuid> = tasks.iter().map(|task| task.id).collect();
        self.running.retain(|id, running| {
            let keep = live.contains(id);
            if !keep {
                running.source.stop();
            }
            keep
        });

        let restarting: Vec<&SyncTask> = tasks
            .iter()
            .filter(|task| self.needs_restart(task))
            .collect();

        restarting
            .into_iter()
            .map(|task| TriggerStart {
                task_id: task.id,
                error: self.start(task),
            })
            .collect()
    }

    /// Stops one task's trigger, for a task being deleted.
    pub fn stop(&mut self, task_id: Uuid) {
        if let Some(mut running) = self.running.remove(&task_id) {
            running.source.stop();
        }
    }

    /// Stops everything, for shutdown. `stop` only returns once an in-flight notification has
    /// been delivered, so after this nothing else arrives.
    pub fn stop_all(&mut self) {
        for (_, running) in self.running.drain() {
            let mut running = running;
            running.source.stop();
        }
    }

    fn needs_restart(&self, task: &SyncTask) -> bool {
        match self.running.get(&task.id) {
            // Neither what it watches nor how changed, so the runner that is working keeps
            // working — restarting a watcher loses whatever quiet period it had accumulated.
            Some(running) => {
                running.trigger != task.trigger || running.source_path != task.source_path
            }
            None => true,
        }
    }

    /// Builds and starts one task's trigger, replacing any it already had.
    fn start(&mut self, task: &SyncTask) -> Option<String> {
        self.stop(task.id);

        let observer: Arc<dyn TriggerObserver> = Arc::new(Forwarder {
            task_id: task.id,
            events: self.events.clone(),
        });

        let mut source =
            match self
                .factory
                .create(&task.trigger, Path::new(&task.source_path), observer)
            {
                Ok(source) => source,
                Err(error) => {
                    tracing::error!(task = %task.name, %error, "could not build the trigger");
                    return Some(error.to_string());
                }
            };

        if let Err(error) = source.start() {
            tracing::error!(task = %task.name, %error, "the trigger would not start");
            return Some(error.to_string());
        }

        self.running.insert(
            task.id,
            Running {
                source,
                trigger: task.trigger.clone(),
                source_path: task.source_path.clone(),
            },
        );
        None
    }
}

impl Drop for TriggerHost {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// Tags a notification with its task and puts it on the channel.
struct Forwarder {
    task_id: Uuid,
    events: flume::Sender<TriggerEvent>,
}

impl TriggerObserver for Forwarder {
    fn notify(&self, notification: Notification) {
        // A closed channel means the window is gone, which is not this thread's problem.
        let _ = self.events.send(TriggerEvent {
            task_id: self.task_id,
            notification,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use syncmaid_core::model::Destination;
    use syncmaid_core::triggers::TriggerError;

    use super::*;

    /// Records what it was asked to build, and can be told to refuse.
    #[derive(Default)]
    struct RecordingFactory {
        built: Mutex<Vec<Trigger>>,
        refuse: bool,
        started: Arc<Mutex<usize>>,
        stopped: Arc<Mutex<usize>>,
    }

    struct Recorded {
        observer: Arc<dyn TriggerObserver>,
        started: Arc<Mutex<usize>>,
        stopped: Arc<Mutex<usize>>,
    }

    impl TriggerSource for Recorded {
        fn start(&mut self) -> Result<(), TriggerError> {
            *self.started.lock().unwrap() += 1;
            // Proves the observer reaches the channel with the right task id.
            self.observer.notify(Notification::Recovered);
            Ok(())
        }

        fn stop(&mut self) {
            *self.stopped.lock().unwrap() += 1;
        }
    }

    impl TriggerSourceFactory for RecordingFactory {
        fn create(
            &self,
            trigger: &Trigger,
            _source_path: &Path,
            observer: Arc<dyn TriggerObserver>,
        ) -> Result<Box<dyn TriggerSource>, TriggerError> {
            if self.refuse {
                return Err(TriggerError::new("no such folder"));
            }
            self.built.lock().unwrap().push(trigger.clone());
            Ok(Box::new(Recorded {
                observer,
                started: Arc::clone(&self.started),
                stopped: Arc::clone(&self.stopped),
            }))
        }
    }

    fn task(name: &str, trigger: Trigger) -> SyncTask {
        SyncTask::new(name, r"C:\src", trigger, Vec::<Destination>::new())
    }

    #[test]
    fn every_task_gets_its_trigger_and_notifications_carry_its_id() {
        let factory = Arc::new(RecordingFactory::default());
        let (mut host, events) = TriggerHost::new(Arc::clone(&factory) as Arc<_>);
        let tasks = vec![
            task("Photos", Trigger::watch()),
            task("Nightly", Trigger::Manual),
        ];

        let started = host.reconcile(&tasks);

        assert_eq!(2, started.len());
        assert!(started.iter().all(|start| start.error.is_none()));

        let seen: Vec<Uuid> = events.drain().map(|event| event.task_id).collect();
        assert_eq!(vec![tasks[0].id, tasks[1].id], seen);
    }

    #[test]
    fn a_trigger_that_will_not_start_is_reported_rather_than_thrown() {
        // The task degrades to manual-only. Silently never running is the worst outcome of the
        // three, so the failure comes back for the card to show.
        let factory = Arc::new(RecordingFactory {
            refuse: true,
            ..Default::default()
        });
        let (mut host, _events) = TriggerHost::new(factory);

        let started = host.reconcile(&[task("Photos", Trigger::watch())]);

        assert_eq!(
            vec![Some("no such folder".to_owned())],
            started
                .into_iter()
                .map(|start| start.error)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_task_whose_trigger_did_not_change_keeps_the_runner_it_has() {
        // Restarting a watcher throws away the quiet period it has accumulated, so an edit that
        // touched neither the trigger nor the source must not touch the watcher either.
        let factory = Arc::new(RecordingFactory::default());
        let started_count = Arc::clone(&factory.started);
        let (mut host, _events) = TriggerHost::new(Arc::clone(&factory) as Arc<_>);
        let mut tasks = vec![task("Photos", Trigger::watch())];

        host.reconcile(&tasks);
        // A destination was added: nothing the trigger cares about.
        tasks[0].destinations.push(Destination::new(
            "D",
            r"D:\d",
            [syncmaid_core::filtering::FilterRule::AllFiles],
            syncmaid_core::model::SyncStrategy::Mirror,
        ));
        let second = host.reconcile(&tasks);

        assert!(second.is_empty(), "nothing was restarted");
        assert_eq!(1, *started_count.lock().unwrap());
    }

    #[test]
    fn changing_the_trigger_or_the_source_rebuilds_the_runner() {
        let factory = Arc::new(RecordingFactory::default());
        let (mut host, _events) = TriggerHost::new(Arc::clone(&factory) as Arc<_>);
        let mut tasks = vec![task("Photos", Trigger::watch())];

        host.reconcile(&tasks);
        tasks[0].trigger = Trigger::scheduled("0 2 * * *");
        host.reconcile(&tasks);
        tasks[0].source_path = r"C:\elsewhere".into();
        host.reconcile(&tasks);

        assert_eq!(
            vec![
                Trigger::watch(),
                Trigger::scheduled("0 2 * * *"),
                Trigger::scheduled("0 2 * * *")
            ],
            factory.built.lock().unwrap().clone()
        );
    }

    #[test]
    fn a_task_that_is_gone_has_its_trigger_stopped() {
        let factory = Arc::new(RecordingFactory::default());
        let stopped = Arc::clone(&factory.stopped);
        let (mut host, _events) = TriggerHost::new(Arc::clone(&factory) as Arc<_>);
        let tasks = vec![task("Photos", Trigger::watch())];

        host.reconcile(&tasks);
        host.reconcile(&[]);

        assert_eq!(1, *stopped.lock().unwrap());
    }

    #[test]
    fn dropping_the_host_stops_everything() {
        // Nothing may fire into a window that is being torn down.
        let factory = Arc::new(RecordingFactory::default());
        let stopped = Arc::clone(&factory.stopped);
        {
            let (mut host, _events) = TriggerHost::new(Arc::clone(&factory) as Arc<_>);
            host.reconcile(&[task("A", Trigger::watch()), task("B", Trigger::watch())]);
        }

        assert_eq!(2, *stopped.lock().unwrap());
    }
}
