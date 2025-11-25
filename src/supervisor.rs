#[cfg(feature = "tracing")]
use tracing::{error, info, warn};

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    task::JoinSet,
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::network::NetworkInterface;
use crate::{Result, handler::Service};

/// Defines how a service should be restarted upon failure.
#[derive(Copy, Clone, Debug)]
pub struct RestartPolicy {
    /// Maximum number of restart attempts. `None` means infinite.
    pub max_attempts: Option<usize>,
    /// Initial delay before the first restart.
    pub base_delay: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_attempts: Some(5),
            base_delay: Duration::from_secs(1),
        }
    }
}

impl RestartPolicy {
    /// Calculates the delay for a specific attempt using exponential backoff.
    fn delay(&self, attempt: usize) -> Duration {
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1) as u32);
        (self.base_delay * factor).min(Duration::from_secs(60))
    }

    /// Sets the maximum number of restart attempts.
    pub fn with_max_attempts(mut self, attempts: usize) -> Self {
        self.max_attempts = Some(attempts);
        self
    }

    /// Sets the initial base delay for the backoff strategy.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }
}

/// The supervisor orchestrates the lifecycle of multiple services.
///
/// It handles startup, graceful shutdown, and automatic restarts based on the
/// provided [`RestartPolicy`].
pub struct Supervisor {
    iface: Arc<NetworkInterface>,
    policy: RestartPolicy,
    tasks: Vec<Box<dyn Task>>,
}

impl Supervisor {
    /// Creates a new supervisor bound to the specified network interface.
    pub fn new(iface: NetworkInterface) -> Self {
        Self {
            iface: Arc::new(iface),
            policy: RestartPolicy::default(),
            tasks: Vec::new(),
        }
    }

    /// Creates a new supervisor using a custom [`RestartPolicy`].
    pub fn with_policy(network_interface: NetworkInterface, restart_policy: RestartPolicy) -> Self {
        Self {
            iface: Arc::new(network_interface),
            policy: restart_policy,
            tasks: Vec::new(),
        }
    }

    /// Adds a service (TCP or UDP) to the supervisor.
    ///
    /// The service will be converted into a supervised task governed by the
    /// supervisor's restart policy.
    pub fn add<K, S>(&mut self, service: S)
    where
        S: Service<K>,
    {
        let task = service.into_task(self.iface.clone(), self.policy);
        self.tasks.push(task);
    }

    /// Runs all registered services.
    ///
    /// This method blocks until a termination signal (Ctrl+C) is received.
    /// It ensures a graceful shutdown of all services within a 5-second timeout.
    pub async fn run(self) -> Result<()> {
        let token = CancellationToken::new();
        let mut set = JoinSet::new();

        if self.tasks.is_empty() {
            #[cfg(feature = "tracing")]
            warn!("Supervisor started with no services. Exiting immediately.");
            return Ok(());
        }

        #[cfg(feature = "tracing")]
        info!("Supervisor starting {} services...", self.tasks.len());

        for task in self.tasks {
            let t = token.child_token();
            set.spawn(async move { task.run(t).await });
        }

        tokio::signal::ctrl_c().await?;
        println!();
        #[cfg(feature = "tracing")]
        info!("Shutdown signal received. Stopping all services...");
        token.cancel();

        let shutdown_future = async { while set.join_next().await.is_some() {} };

        if timeout(Duration::from_secs(5), shutdown_future)
            .await
            .is_err()
        {
            #[cfg(feature = "tracing")]
            error!("Grace period exceeded! Forcing shutdown of remaining services.");
            set.abort_all();
        } else {
            #[cfg(feature = "tracing")]
            info!("All services shut down gracefully.");
        }

        Ok(())
    }
}

/// Internal trait representing a runnable task.
pub trait Task: Send + Sync {
    /// Executes the task, respecting the cancellation token.
    fn run(&self, token: CancellationToken) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// A generic task that runs a factory closure with restart logic.
pub struct SupervisedTask<F> {
    #[cfg_attr(not(feature = "tracing"), allow(dead_code))]
    name: &'static str,
    policy: RestartPolicy,
    factory: Arc<F>,
}

impl<F> SupervisedTask<F>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync,
{
    /// Creates a new supervised task instance.
    pub fn new(name: &'static str, policy: RestartPolicy, factory: F) -> Self {
        Self {
            name,
            policy,
            factory: Arc::new(factory),
        }
    }
}

impl<F> Task for SupervisedTask<F>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync + 'static,
{
    fn run(&self, token: CancellationToken) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        #[cfg(feature = "tracing")]
        let name = self.name;
        let policy = self.policy;
        let factory = self.factory.clone();

        Box::pin(async move {
            let mut attempts = 0;

            loop {
                #[cfg(feature = "tracing")]
                info!("[{}] Starting service instance...", name);
                let future = factory();

                tokio::select! {
                    res = future => {
                        match res {
                            Ok(_) => {
                                #[cfg(feature = "tracing")]
                                info!("[{}] Service exited normally.", name);
                                break;
                            },
                            Err(e) => {
                                #[cfg(feature = "tracing")]
                                error!("[{}] Service crashed: {}", name, e);
                                #[cfg(not(feature = "tracing"))]
                                let _ = e;
                            }
                        }
                    }
                    _ = token.cancelled() => {
                        #[cfg(feature = "tracing")]
                        info!("[{}] Cancellation requested. Stopping.", name);
                        break;
                    }
                }

                attempts += 1;
                if let Some(max) = policy.max_attempts
                    && attempts >= max
                {
                    #[cfg(feature = "tracing")]
                    error!(
                        "[{}] Max restart attempts ({}) reached. Service is DEAD.",
                        name, max
                    );
                    break;
                }

                let delay = policy.delay(attempts);
                #[cfg(feature = "tracing")]
                warn!(
                    "[{}] Will restart in {:.1}s (Attempt {}/{:?})",
                    name,
                    delay.as_secs_f32(),
                    attempts,
                    policy.max_attempts
                );

                tokio::select! {
                    _ = sleep(delay) => {},
                    _ = token.cancelled() => break,
                }
            }
        })
    }
}
