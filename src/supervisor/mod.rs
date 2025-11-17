pub mod policy;

use std::{io::Result, sync::Arc, time::Duration};
use tokio::{
    task::JoinSet,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{
    NetworkInterface, RestartPolicy,
    worker::{Worker, WorkerRuntime, adapter::WorkerAdapter},
};

const GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Coordinates and supervises multiple network Runtimes.
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
    /// * `A` - The service adapter type implementing `WorkerAdapter`.
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
        A: WorkerAdapter<P>,
    {
        let worker = WorkerRuntime::new(
            adapter.into_worker(Arc::clone(&self.network_interface)),
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
                if let Err(e) = result {
                    error!("Worker task failed: {e:?}");
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
