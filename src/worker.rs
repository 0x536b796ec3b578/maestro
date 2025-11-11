use async_trait::async_trait;
use tokio::{select, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{RestartPolicy, Service};

/// Represents an asynchronous worker that runs and manages a service lifecycle.
///
/// A `Worker` encapsulates the execution logic for a service instance, allowing
/// the runtime to start, supervise, and gracefully stop it when a cancellation
/// signal is received.
#[async_trait]
pub(crate) trait Worker: Send {
    /// Runs the worker until completion or cancellation.
    async fn run(&self, token: CancellationToken);
}

/// Supervises a [`Service`] with an automatic restart policy.
///
/// A `WorkerService` wraps a service implementation and enforces a restart
/// strategy defined by [`RestartPolicy`]. It continuously runs the service,
/// restarts it on failure, and stops when a cancellation request is received.
pub(crate) struct WorkerService<S: Service> {
    inner: S,
    policy: RestartPolicy,
}

impl<S> WorkerService<S>
where
    S: Service + Send + Sync + 'static,
{
    /// Creates a new [`WorkerService`] instance.
    ///
    /// # Parameters
    /// - `inner`: The [`Service`] to supervise.
    /// - `policy`: The restart policy to apply when the service fails.
    pub(crate) fn new(inner: S, policy: RestartPolicy) -> Self {
        Self { inner, policy }
    }
}

/// Use self.inner.name() to improve logs by displaying the service on which the worker is working.
#[async_trait]
impl<S> Worker for WorkerService<S>
where
    S: Service + Send + Sync + 'static,
{
    /// Executes the supervised service under a restart policy.
    ///
    /// The worker:
    /// - Runs the inner service via [`Service::serve`].
    /// - Restarts it on failure according to the configured [`RestartPolicy`].
    /// - Stops when the cancellation token is triggered.
    async fn run(&self, token: CancellationToken) {
        debug!("Worker started supervision loop.");

        let mut attempts = 0usize;

        loop {
            let restart_needed = select! {
                res = self.inner.serve() => match res {
                        Ok(()) => {
                            debug!("Service exited normally.");
                            false
                        }
                        Err(e) => {
                            error!("Service error: {e:?}");
                            true
                        }
                    },
                _ = token.cancelled() => {
                    info!("Service interruption requested.");
                    break;
                }
            };

            if !restart_needed {
                break;
            }

            attempts = attempts.saturating_add(1);

            if let Some(max) = self.policy.max_attempts
                && attempts >= max
            {
                error!("Maximum number of restart attempts ({max}) reached. Abort.");
                break;
            }

            let delay = self.policy.delay_for_attempt(attempts);
            warn!("Service restarting in {:?}...", delay);

            select! {
                _ = sleep(delay) => {},
                _ = token.cancelled() => {
                    info!("Cancellation requested during restart delay.");
                    break;
                }
            }
        }

        info!("Worker has finished.");
    }
}
