pub(crate) mod adapter;

use async_trait::async_trait;
use tokio::{select, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{RestartPolicy, runtime::Runtime};

/// Represents an asynchronous worker that manages a [`Runtime`] lifecycle.
///
/// A `Worker` encapsulates the execution logic for a runtime instance,
/// allowing it to be started, supervised, and gracefully stopped via a
/// [`CancellationToken`].
#[async_trait]
pub(crate) trait Worker: Send {
    /// Runs the worker until completion or cancellation.
    async fn run(&self, token: CancellationToken);
}

/// Supervises a [`Runtime`] with an automatic restart policy.
///
/// A `WorkerRuntime` wraps a service implementation and enforces a restart
/// strategy defined by [`RestartPolicy`]. It continuously runs the service,
/// restarts it on failure, and stops when a cancellation request is received.
pub(crate) struct WorkerRuntime<R: Runtime> {
    inner: R,
    policy: RestartPolicy,
}

impl<R> WorkerRuntime<R>
where
    R: Runtime + Send + Sync + 'static,
{
    /// Creates a new [`WorkerRuntime`] instance.
    ///
    /// # Parameters
    /// - `inner`: The [`Runtime`] to supervise.
    /// - `policy`: The restart policy to apply when the service fails.
    pub(crate) fn new(inner: R, policy: RestartPolicy) -> Self {
        Self { inner, policy }
    }
}

#[async_trait]
impl<R> Worker for WorkerRuntime<R>
where
    R: Runtime + Send + Sync + 'static,
{
    /// Executes the supervised service under a restart policy.
    ///
    /// The worker:
    /// - Runs the inner service via [`Runtime::serve`].
    /// - Restarts it on failure according to the configured [`RestartPolicy`].
    /// - Stops when the cancellation token is triggered.
    async fn run(&self, token: CancellationToken) {
        debug!("Worker `{}` started supervision loop.", self.inner.name());

        let mut attempts = 0usize;

        loop {
            let restart_needed = select! {
                res = self.inner.serve() => match res {
                        Ok(()) => {
                            debug!("Runtime `{}` exited normally.", self.inner.name());
                            false
                        }
                        Err(e) => {
                            error!("Runtime `{}` error: {e:?}", self.inner.name());
                            true
                        }
                    },
                _ = token.cancelled() => {
                    info!("Runtime `{}` interruption requested.", self.inner.name());
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
                error!(
                    "Runtime `{}` reached maximum restart attempts ({max}). Aborting.",
                    self.inner.name()
                );
                break;
            }

            let delay = self.policy.delay_for_attempt(attempts);
            warn!(
                "Runtime `{}` restarting in {:?}...",
                self.inner.name(),
                delay
            );

            select! {
                _ = sleep(delay) => {},
                _ = token.cancelled() => {
                    info!("Cancellation requested during restart delay for `{}`.", self.inner.name());
                    break;
                }
            }
        }

        info!("Worker `{}` has finished.", self.inner.name());
    }
}
