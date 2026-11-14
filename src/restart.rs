use std::time::Duration;

/// How the [`Supervisor`](crate::Supervisor) restarts a service that fails.
///
/// Restarts back off exponentially: `base_delay`, then `2 × base_delay`,
/// `4 × base_delay`, … up to `max_delay`. If an instance stays up for at
/// least `max_delay`, it counts as healthy and the restart counter resets.
///
/// # Examples
///
/// ```
/// use maestro_rs::RestartPolicy;
/// use std::time::Duration;
///
/// let policy = RestartPolicy::default()
///     .with_max_restarts(10)
///     .with_base_delay(Duration::from_millis(200));
/// assert_eq!(policy.max_restarts(), Some(10));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    max_restarts: Option<u32>,
    base_delay: Duration,
    max_delay: Duration,
}

impl Default for RestartPolicy {
    /// Up to 5 restarts, starting at 1 s and capped at 60 s.
    fn default() -> Self {
        Self {
            max_restarts: Some(5),
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
        }
    }
}

impl RestartPolicy {
    /// A policy that never restarts: the first failure is final.
    #[must_use]
    pub fn never() -> Self {
        Self::default().with_max_restarts(0)
    }

    /// Gives up after `n` consecutive restarts.
    #[must_use]
    pub const fn with_max_restarts(mut self, n: u32) -> Self {
        self.max_restarts = Some(n);
        self
    }

    /// Restarts forever.
    #[must_use]
    pub const fn with_unlimited_restarts(mut self) -> Self {
        self.max_restarts = None;
        self
    }

    /// Delay before the first restart.
    #[must_use]
    pub const fn with_base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Upper bound on the backoff delay. This is also how long an instance
    /// must stay up before the restart counter resets.
    #[must_use]
    pub const fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Maximum consecutive restarts, or `None` for unlimited.
    #[must_use]
    pub const fn max_restarts(&self) -> Option<u32> {
        self.max_restarts
    }

    /// Delay before the first restart.
    #[must_use]
    pub const fn base_delay(&self) -> Duration {
        self.base_delay
    }

    /// Upper bound on the backoff delay.
    #[must_use]
    pub const fn max_delay(&self) -> Duration {
        self.max_delay
    }

    /// Whether restart number `restart` (1-based) is allowed.
    pub(crate) fn allows(&self, restart: u32) -> bool {
        self.max_restarts.is_none_or(|max| restart <= max)
    }

    /// Backoff before restart number `restart` (1-based).
    pub(crate) fn delay(&self, restart: u32) -> Duration {
        let factor = 2u32.saturating_pow(restart.saturating_sub(1));
        self.base_delay
            .checked_mul(factor)
            .unwrap_or(Duration::MAX)
            .min(self.max_delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_doubles_until_capped() {
        let p = RestartPolicy::default();
        let secs: Vec<u64> = (1..=8).map(|n| p.delay(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 32, 60, 60]);
    }

    #[test]
    fn delay_does_not_overflow() {
        let p = RestartPolicy::default()
            .with_base_delay(Duration::MAX)
            .with_max_delay(Duration::MAX);
        assert_eq!(p.delay(u32::MAX), Duration::MAX);
    }

    #[test]
    fn max_restarts_counts_restarts_not_starts() {
        let p = RestartPolicy::default().with_max_restarts(2);
        assert!(p.allows(1));
        assert!(p.allows(2));
        assert!(!p.allows(3));
        assert!(!RestartPolicy::never().allows(1));
        assert!(
            RestartPolicy::default()
                .with_unlimited_restarts()
                .allows(u32::MAX)
        );
    }
}
