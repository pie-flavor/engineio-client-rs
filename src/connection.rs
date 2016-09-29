use std::io::Error;

use {Packet};
use transports::Data;

use futures::{BoxFuture, Poll};
use futures::stream::{self, Stream};
use tokio_core::reactor::Handle;
use url::Url;

/// Creates a new engine.io connection using the given configuration.
pub fn connect(config: Config, handle: Handle) -> BoxFuture<(Sender, Receiver), Error> {
    unimplemented!();
}

/// Creates a new engine.io connection using the given configuration.
///
/// Since this method also accepts transport configuration, it also allows
/// reopening a broken connection after downtime.
pub fn connect_with_data(conn_cfg: Config, tp_cfg: Data, handle: Handle) -> BoxFuture<(Sender, Receiver), Error> {
    unimplemented!();
}

/// Contains the configuration for creating a new connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Extra headers to pass during each request.
    pub extra_headers: Vec<(String, String)>,
    /// The engine.io endpoint.
    pub url: Url
}

/// The receiving half of an engine.io connection.
pub struct Receiver;

/// The sending half of an engine.io connection.
pub struct Sender;