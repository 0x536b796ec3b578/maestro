use std::{fmt, future::Future, pin::pin, sync::Arc, time::Duration};

use tokio::{task::JoinSet, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::{
    Error, NetworkInterface, RestartPolicy, Result,
    log::{log_error, log_info, log_warn},
    service::{self, Context, Entry, Service},
};

const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs a set of TCP/UDP services on one network interface and keeps them alive.
///
/// When a service fails (for example, it cannot bind or a UDP socket errors),
/// it is restarted according to its [`RestartPolicy`]. On shutdown every
/// service stops accepting new work and gets [`with_shutdown_timeout`] to
/// finish in-flight work. After that it is aborted.
///
/// [`with_shutdown_timeout`]: Self::with_shutdown_timeout
///
/// # Examples
///
/// ```no_run
/// # use maestro_rs::{NetworkInterface, Supervisor, UdpHandler};
/// # use std::{net::SocketAddr, sync::Arc};
/// # struct Dns;
/// # impl UdpHandler for Dns {
/// #     fn name(&self) -> &str { "dns" }
/// #     fn port(&self) -> u16 { 53 }
/// #     async fn on_packet(&self, _: &[u8], _: &Arc<tokio::net::UdpSocket>, _: SocketAddr) {}
/// # }
/// # #[tokio::main] async fn main() -> maestro_rs::Result<()> {
/// let mut supervisor = Supervisor::new(NetworkInterface::from_name("eth0")?);
/// supervisor.add(Dns);
/// supervisor.run().await // until Ctrl+C / SIGTERM
/// # }
/// ```
pub struct Supervisor {
    iface: Arc<NetworkInterface>,
    policy: RestartPolicy,
    shutdown_timeout: Duration,
    services: Vec<Entry>,
}

impl Supervisor {
    /// Creates a supervisor whose services bind to `iface`.
    #[must_use]
    pub fn new(iface: NetworkInterface) -> Self {
        Self {
            iface: Arc::new(iface),
            policy: RestartPolicy::default(),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
            services: Vec::new(),
        }
    }

    /// Sets the restart policy applied to every service. Defaults to [`RestartPolicy::default`].
    #[must_use]
    pub fn with_restart_policy(mut self, policy: RestartPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Sets how long services may take to finish in-flight work on shutdown
    /// before they are aborted. Defaults to 5 s.
    #[must_use]
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// The interface services bind to.
    #[must_use]
    pub fn interface(&self) -> &NetworkInterface {
        &self.iface
    }

    /// Registers a [`TcpHandler`](crate::TcpHandler) or [`UdpHandler`](crate::UdpHandler).
    ///
    /// Nothing is bound until [`run`](Self::run) or [`run_until`](Self::run_until) is called.
    pub fn add<K, S: Service<K>>(&mut self, service: S) -> &mut Self {
        self.services.push(service::into_entry(service));
        self
    }

    /// Runs all services until Ctrl+C, SIGTERM (Unix), or a console close or
    /// system shutdown event (Windows).
    ///
    /// # Errors
    ///
    /// See [`run_until`](Self::run_until).
    pub async fn run(self) -> Result<()> {
        self.run_until(shutdown_signal()).await
    }

    /// Runs all services until `signal` completes, then shuts them down gracefully.
    ///
    /// Returns immediately with `Ok(())` if no services were added.
    ///
    /// # Errors
    ///
    /// Returns [`Error::AllServicesFailed`] if every service gives up (per its
    /// [`RestartPolicy`]) before `signal` completes.
    pub async fn run_until(self, signal: impl Future<Output = ()>) -> Result<()> {
        if self.services.is_empty() {
            log_warn!("supervisor has no services; nothing to run");
            return Ok(());
        }
        log_info!(services = self.services.len(), interface = %self.iface, "supervisor starting");

        let shutdown = CancellationToken::new();
        let mut running = JoinSet::new();
        for entry in self.services {
            let ctx = Context {
                iface: self.iface.clone(),
                shutdown: shutdown.clone(),
            };
            running.spawn(supervise(entry, ctx, self.policy));
        }

        let mut signal = pin!(signal);
        loop {
            tokio::select! {
                () = &mut signal => break,
                joined = running.join_next() => match joined {
                    None => {
                        log_error!("all services have stopped");
                        return Err(Error::AllServicesFailed);
                    }
                    Some(Err(e)) => log_error!(error = %e, "service supervisor task panicked"),
                    Some(Ok(())) => {}
                },
            }
        }

        log_info!("shutting down");
        shutdown.cancel();
        let drain = async { while running.join_next().await.is_some() {} };
        if tokio::time::timeout(self.shutdown_timeout, drain)
            .await
            .is_err()
        {
            log_warn!(timeout = ?self.shutdown_timeout, "shutdown timed out; aborting remaining services");
            running.shutdown().await;
        } else {
            log_info!("all services stopped gracefully");
        }
        Ok(())
    }
}

impl fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Supervisor")
            .field("interface", &self.iface.name())
            .field("policy", &self.policy)
            .field("shutdown_timeout", &self.shutdown_timeout)
            .field(
                "services",
                &self.services.iter().map(|e| &e.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Runs one service, restarting it per `policy` until shutdown or it gives up.
async fn supervise(entry: Entry, ctx: Context, policy: RestartPolicy) {
    let name = entry.name.as_str();
    let shutdown = ctx.shutdown.clone();
    let mut restarts = 0u32;

    loop {
        let started = Instant::now();
        let result = (entry.start)(ctx.clone()).await;
        if shutdown.is_cancelled() {
            log_info!(service = name, "stopped");
            return;
        }
        match result {
            Ok(()) => log_warn!(service = name, "exited unexpectedly"),
            Err(e) => log_error!(service = name, error = %crate::log::Chain(&e), "failed"),
        }

        if started.elapsed() >= policy.max_delay() {
            restarts = 0;
        }
        restarts = restarts.saturating_add(1);
        if !policy.allows(restarts) {
            log_error!(
                service = name,
                restarts = restarts - 1,
                "giving up; restart limit reached"
            );
            return;
        }

        let delay = policy.delay(restarts);
        log_warn!(service = name, attempt = restarts, ?delay, "restarting");
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = shutdown.cancelled() => return,
        }
    }
}

/// Completes on Ctrl+C, on SIGTERM (Unix), or when the console window is
/// closed or the system shuts down (Windows).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            log_error!(error = %e, "cannot listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };

    tokio::select! {
        () = ctrl_c => log_info!("received Ctrl+C"),
        () = platform_terminate() => log_info!("received termination request"),
    }
}

#[cfg(unix)]
async fn platform_terminate() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            sigterm.recv().await;
        }
        Err(e) => {
            log_error!(error = %e, "cannot listen for SIGTERM");
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(windows)]
async fn platform_terminate() {
    use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
    match (ctrl_close(), ctrl_shutdown()) {
        (Ok(mut close), Ok(mut shutdown)) => {
            tokio::select! {
                _ = close.recv() => {}
                _ = shutdown.recv() => {}
            }
        }
        (Err(e), _) | (_, Err(e)) => {
            log_error!(error = %e, "cannot listen for console close/shutdown events");
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(not(any(unix, windows)))]
async fn platform_terminate() {
    std::future::pending::<()>().await;
}
