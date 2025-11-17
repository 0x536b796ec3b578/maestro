use std::time::Duration;

/// Defines the behavior for restarting failed worker tasks.
///
/// A [`RestartPolicy`] controls how the [`Supervisor`] responds when a worker exits unexpectedly.
///
/// # Details
/// * **max_attempts** - Maximum number of restart attempts per worker (`None` = unlimited).
/// * **base_delay** - Initial delay between attempts; increases exponentially on repeated failures.
///
/// This prevents "restart storms" by spacing out retries while keeping recovery automatic.
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
    /// * `base_delay`: 1 sec
    ///
    /// This configuration balances resilience and stability. Workers are retried
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
        // Exponential backoff: base_delay * 2^(attempt-1)
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1) as u32);
        let delay = self.base_delay * factor;
        delay.min(Duration::from_secs(60))
    }
}
