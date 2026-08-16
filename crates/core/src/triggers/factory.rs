//! Picking the runner a trigger needs.

use std::path::Path;
use std::sync::Arc;

use crate::io::FileSystem;
use crate::triggers::{
    CronSchedule, ManualTriggerSource, PollingWatchTriggerSource, ScheduledTriggerSource, Trigger,
    TriggerError, TriggerObserver, TriggerSource, WatchTriggerSource,
};

/// Builds the runner for a trigger, given where its source lives.
pub trait TriggerSourceFactory: Send + Sync {
    fn create(
        &self,
        trigger: &Trigger,
        source_path: &Path,
        observer: Arc<dyn TriggerObserver>,
    ) -> Result<Box<dyn TriggerSource>, TriggerError>;
}

/// The shipping factory.
pub struct DefaultTriggerSourceFactory {
    file_system: Arc<dyn FileSystem>,
}

impl DefaultTriggerSourceFactory {
    pub fn new(file_system: Arc<dyn FileSystem>) -> Self {
        Self { file_system }
    }
}

impl TriggerSourceFactory for DefaultTriggerSourceFactory {
    fn create(
        &self,
        trigger: &Trigger,
        source_path: &Path,
        observer: Arc<dyn TriggerObserver>,
    ) -> Result<Box<dyn TriggerSource>, TriggerError> {
        match trigger {
            Trigger::Manual => Ok(Box::new(ManualTriggerSource::new(observer))),

            Trigger::Scheduled { cron_expression } => {
                let schedule = CronSchedule::parse(cron_expression)
                    .map_err(|error| TriggerError::new(error.to_string()))?;
                Ok(Box::new(ScheduledTriggerSource::new(schedule, observer)))
            }

            Trigger::Watch { .. } => {
                let settle = trigger.settle_window().unwrap_or_default();
                // Windows change notifications are unreliable on mapped drives and UNC paths,
                // so a network source is walked instead — same quiet period, same behaviour
                // from the user's side.
                if crate::io::is_network(source_path) {
                    Ok(Box::new(PollingWatchTriggerSource::new(
                        Arc::clone(&self.file_system),
                        source_path,
                        settle,
                        observer,
                    )))
                } else {
                    Ok(Box::new(WatchTriggerSource::new(
                        Arc::clone(&self.file_system),
                        source_path,
                        settle,
                        observer,
                    )))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::io::InMemoryFileSystem;
    use crate::triggers::Notification;

    fn factory() -> DefaultTriggerSourceFactory {
        DefaultTriggerSourceFactory::new(Arc::new(InMemoryFileSystem::new()))
    }

    fn observer() -> Arc<dyn TriggerObserver> {
        Arc::new(|_: Notification| {})
    }

    #[test]
    fn a_manual_trigger_needs_nothing_running() {
        let mut source = factory()
            .create(&Trigger::Manual, Path::new(r"C:\src"), observer())
            .unwrap();

        source.start().unwrap();
        source.stop();
    }

    #[test]
    fn an_invalid_cron_expression_fails_to_build_rather_than_silently_never_running() {
        let built = factory().create(
            &Trigger::scheduled("not a cron pattern"),
            Path::new(r"C:\src"),
            observer(),
        );

        assert!(built.is_err());
    }

    #[test]
    fn a_watch_on_a_network_source_polls_instead_of_listening() {
        // Proven by behaviour rather than by type: a notify watcher on a path that does not
        // exist fails to start, while the poller happily starts and reports the outage.
        let mut polled = factory()
            .create(&Trigger::watch(), Path::new(r"\\server\share"), observer())
            .unwrap();
        assert!(
            polled.start().is_ok(),
            "the poller tolerates a share that is not there yet"
        );
        polled.stop();

        let root = tempfile::tempdir().unwrap();
        let mut listened = factory()
            .create(&Trigger::watch(), &root.path().join("gone"), observer())
            .unwrap();
        assert!(
            listened.start().is_err(),
            "a local watcher needs the folder to exist"
        );
    }

    #[test]
    fn a_local_watch_fires_on_a_change() {
        let root = tempfile::tempdir().unwrap();
        let (sender, notifications) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let observer: Arc<dyn TriggerObserver> = Arc::new(move |notification: Notification| {
            let _ = sender.lock().unwrap().send(notification);
        });

        // A real filesystem, because the watcher confirms the change by walking the source.
        let factory =
            DefaultTriggerSourceFactory::new(Arc::new(crate::io::PhysicalFileSystem::new()));
        let mut source = factory
            .create(&Trigger::Watch { settle_seconds: 1 }, root.path(), observer)
            .unwrap();
        source.start().unwrap();
        std::fs::write(root.path().join("a.txt"), b"a").unwrap();

        assert_eq!(
            Ok(Notification::Fired),
            notifications.recv_timeout(std::time::Duration::from_secs(10))
        );
        source.stop();
    }
}
