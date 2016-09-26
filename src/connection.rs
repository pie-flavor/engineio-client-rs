use std::io::Error;

use {Config, Packet};
use futures::{BoxFuture, Poll};
use futures::stream::{self, Stream};
use tokio_core::reactor::Handle;

/// The receiving half of an engine.io connection.
pub struct Receiver;

/// The sending half of an engine.io connection.
pub struct Sender;

/// Creates a new engine.io connection using the given configuration.
pub fn connect(config: Config, handle: Handle) -> BoxFuture<(Sender, Receiver), Error> {
    unimplemented!();
}