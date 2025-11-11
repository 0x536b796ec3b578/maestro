use std::{io::Result, sync::Arc, time::Duration};
use tokio::{
    task::JoinSet,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{NetworkInterface, ServiceAdapter, Worker, WorkerService};

const GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Coordinates and monitors multiple service workers.
///
/// The [`Supervisor`] manages the lifecycle of all `Worker's` instances,
/// handling startup, graceful shutdown, and automatic restarts based on
/// the configured [`RestartPolicy`].
pub struct Supervisor {
    /// Shared reference to the network interface layer.
    network_interface: Arc<NetworkInterface>,

    /// Policy governing worker restarts on failure.
    restart_policy: RestartPolicy,

    /// Managed worker instances.
    workers: Vec<Box<dyn Worker + Send>>,
}

impl Supervisor {
    /// Creates a new [`Supervisor`] with the default [`RestartPolicy`].
    ///
    /// # Example
    /// ```rust,no_run
    /// use maestro::Supervisor;
    ///
    /// let mut supervisor = Supervisor::new(network_interface);
    /// ```
    pub fn new(network_interface: NetworkInterface) -> Self {
        Self {
            network_interface: Arc::new(network_interface),
            restart_policy: RestartPolicy::default(),
            workers: vec![],
        }
    }

    /// Creates a new [`Supervisor`] using a custom [`RestartPolicy`].
    ///
    /// # Example
    /// ```rust,no_run
    /// use std::time::Duration;
    /// use maestro::{RestartPolicy, Supervisor};
    ///
    /// let restart_policy = RestartPolicy::default()
    ///    .with_max_attempts(3)
    ///    .with_delay(Duration::from_secs(5));
    /// let mut supervisor = Supervisor::with_policy(network_interface, restart_policy);
    /// ```
    pub fn with_policy(network_interface: NetworkInterface, restart_policy: RestartPolicy) -> Self {
        Self {
            network_interface: Arc::new(network_interface),
            restart_policy,
            workers: vec![],
        }
    }

    /// Registers a service adapter as a managed worker.
    ///
    /// # Type Parameters
    /// * `P` - The protocol implemented by the adapter (e.g. [`crate::Tcp`] or [`crate::Udp`]).
    /// * `A` - The service adapter type implementing `ServiceAdapter`.
    ///
    /// # Example
    /// ```rust,no_run
    /// use maestro::{Supervisor, Udp};
    ///
    /// supervisor.add(MyUdpService);         // type inferred
    /// supervisor.add<Udp, _>(MyUdpService); // explicit turbofish
    /// ```
    pub fn add<P, A>(&mut self, adapter: A)
    where
        A: ServiceAdapter<P>,
    {
        let worker = WorkerService::new(
            adapter.to_service(Arc::clone(&self.network_interface)),
            self.restart_policy,
        );
        self.workers.push(Box::new(worker));
    }

    /// Runs all registered workers and supervises their execution.
    ///
    /// # Behavior
    /// * Each worker runs as an independent Tokio task.
    /// * When interrupted (Ctrl+C), all workers receive a [`CancellationToken`].
    /// * Tasks that don't shut down within the grace period are force-aborted.
    pub async fn run(self) -> Result<()> {
        let shutdown = CancellationToken::new();
        let mut tasks = JoinSet::new();

        for worker in self.workers {
            let token = shutdown.child_token();
            tasks.spawn(async move { worker.run(token).await });
        }

        tokio::signal::ctrl_c().await?;
        println!();
        info!("Received Ctrl+C. Shutdown signal sent to workers.");
        shutdown.cancel();

        let graceful_shutdown = async {
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok(_) => info!("Worker exited cleanly."),
                    Err(e) => error!("Worker task failed: {e:?}"),
                }
            }
        };

        let start = Instant::now();
        match timeout(GRACE_PERIOD, graceful_shutdown).await {
            Ok(_) => info!("All workers shut down gracefully in {:?}.", start.elapsed()),
            Err(_) => {
                warn!(
                    "Grace period ({:?}) expired. Aborting remaining tasks.",
                    GRACE_PERIOD
                );
                tasks.abort_all();
            }
        }

        Ok(())
    }
}

/// Defines the behavior for restarting failed worker tasks.
///
/// A [`RestartPolicy`] controls how the [`Supervisor`] responds when a worker exits unexpectedly.
///
/// # Details
/// * **max_attempts** - Maximum number of restart attempts per worker (`None` = unlimited).
/// * **base_delay** - Initial delay between attempts; increases exponentially on repeated failures.
///
/// This prevents restart storms by spacing out retries while keeping recovery automatic.
#[derive(Copy, Clone)]
pub struct RestartPolicy {
    pub(crate) max_attempts: Option<usize>,
    pub(crate) base_delay: Duration,
}

impl Default for RestartPolicy {
    /// Provides a sensible default restart strategy.
    ///
    /// # Default Values
    /// * `max_attempts`: 5
    /// * `base_delay`: 1 second
    ///
    /// This configuration balances resilience and stability — workers are retried
    /// a few times with a short initial delay before the supervisor gives up.
    fn default() -> Self {
        RestartPolicy {
            max_attempts: Some(5),
            base_delay: Duration::from_secs(1),
        }
    }
}

impl RestartPolicy {
    /// Sets the maximum number of restart attempts before a worker is abandoned.
    ///
    /// # Example
    /// ```rust,no_run
    /// use maestro::RestartPolicy;
    /// let policy = RestartPolicy::default().with_max_attempts(10);
    /// ```
    pub fn with_max_attempts(mut self, attempts: usize) -> Self {
        self.max_attempts = Some(attempts);
        self
    }

    /// Sets the base delay duration between worker restart attempts.
    ///
    /// This value serves as the starting point for exponential backoff delays.
    ///
    /// # Example
    /// ```rust,no_run
    /// use std::time::Duration;
    /// use maestro::RestartPolicy;
    /// let policy = RestartPolicy::default().with_delay(Duration::from_secs(2));
    /// ```
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Computes the exponential backoff delay for a given restart attempt.
    ///
    /// The delay doubles on each subsequent failure, up to a 60-second cap.
    /// This prevents tight restart loops that can overload the system.
    pub(crate) fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1) as u32);
        let delay = self.base_delay * factor;
        delay.min(Duration::from_secs(60))
    }
}
