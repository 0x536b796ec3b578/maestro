//! Glue that turns user handlers into restartable services.

use std::{future::Future, pin::Pin, sync::Arc};

use tokio_util::sync::CancellationToken;

use crate::{NetworkInterface, Result};

/// Protocol marker for [`TcpHandler`](crate::TcpHandler) services. See [`Service`].
#[derive(Debug)]
pub enum Tcp {}

/// Protocol marker for [`UdpHandler`](crate::UdpHandler) services. See [`Service`].
#[derive(Debug)]
pub enum Udp {}

/// Anything [`Supervisor::add`](crate::Supervisor::add) accepts.
///
/// Implemented for every [`TcpHandler`](crate::TcpHandler) (with `Kind = Tcp`) and every
/// [`UdpHandler`](crate::UdpHandler) (with `Kind = Udp`). The trait is sealed and cannot be
/// implemented outside this crate. If a type implements both handler
/// traits, pick one explicitly with `supervisor.add::<Tcp, _>(svc)`.
pub trait Service<Kind>: sealed::Sealed<Kind> {}

impl<K, T: sealed::Sealed<K>> Service<K> for T {}

/// What a running service instance gets from the supervisor.
#[derive(Clone)]
pub(crate) struct Context {
    pub(crate) iface: Arc<NetworkInterface>,
    pub(crate) shutdown: CancellationToken,
}

type ServiceFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

/// A type-erased service: a name plus a factory that starts one instance.
pub struct Entry {
    pub(crate) name: String,
    pub(crate) start: Box<dyn Fn(Context) -> ServiceFuture + Send + Sync>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

mod sealed {
    use super::{Entry, Tcp, Udp};
    use crate::{TcpHandler, UdpHandler, tcp, udp};
    use std::sync::Arc;

    pub trait Sealed<Kind> {
        fn into_entry(self) -> Entry;
    }

    impl<T: TcpHandler> Sealed<Tcp> for T {
        fn into_entry(self) -> Entry {
            let handler = Arc::new(self);
            Entry {
                name: handler.name().to_owned(),
                start: Box::new(move |ctx| Box::pin(tcp::serve(handler.clone(), ctx))),
            }
        }
    }

    impl<T: UdpHandler> Sealed<Udp> for T {
        fn into_entry(self) -> Entry {
            let handler = Arc::new(self);
            Entry {
                name: handler.name().to_owned(),
                start: Box::new(move |ctx| Box::pin(udp::serve(handler.clone(), ctx))),
            }
        }
    }
}

pub(crate) fn into_entry<K, S: Service<K>>(service: S) -> Entry {
    sealed::Sealed::into_entry(service)
}
