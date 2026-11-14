//! Internal logging facade.
//!
//! The macros forward to [`tracing`] when the `tracing` feature is enabled and
//! expand to nothing otherwise, so call sites need no `#[cfg]` noise.

#[cfg(feature = "tracing")]
macro_rules! log_debug { ($($t:tt)*) => { ::tracing::debug!($($t)*) } }
#[cfg(feature = "tracing")]
macro_rules! log_info { ($($t:tt)*) => { ::tracing::info!($($t)*) } }
#[cfg(feature = "tracing")]
macro_rules! log_warn { ($($t:tt)*) => { ::tracing::warn!($($t)*) } }
#[cfg(feature = "tracing")]
macro_rules! log_error { ($($t:tt)*) => { ::tracing::error!($($t)*) } }

#[cfg(not(feature = "tracing"))]
macro_rules! log_debug {
    ($($t:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "tracing"))]
macro_rules! log_info {
    ($($t:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "tracing"))]
macro_rules! log_warn {
    ($($t:tt)*) => {
        ()
    };
}
#[cfg(not(feature = "tracing"))]
macro_rules! log_error {
    ($($t:tt)*) => {
        ()
    };
}

pub(crate) use {log_debug, log_error, log_info, log_warn};

/// Displays an error followed by its whole `source()` chain.
#[cfg(feature = "tracing")]
pub(crate) struct Chain<'a>(pub(crate) &'a (dyn std::error::Error + 'static));

#[cfg(feature = "tracing")]
impl std::fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(err) = source {
            write!(f, ": {err}")?;
            source = err.source();
        }
        Ok(())
    }
}
