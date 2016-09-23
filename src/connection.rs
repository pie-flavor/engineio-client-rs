use {Config, EngineError, Packet};
use futures::{BoxFuture, Future, Poll};
use futures::stream::{channel, Receiver, Sender, Stream};
use tokio_core::reactor::Handle;
use transports::Transport;

/// Represents an engine.io connection to a server.
pub struct Connection {
    receiver: Receiver<Packet, EngineError>
}

impl Connection {
    /// Asynchronously creates a new connection from the given configuration.
    pub fn new(config: Config, handle: &Handle) -> BoxFuture<Connection, EngineError> {
        let (tx, rx) = channel::<Packet, EngineError>();
        let handle = handle.clone();
        unimplemented!();
    }
}

impl Stream for Connection {
    type Item = Packet;
    type Error = EngineError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.receiver.poll()
    }
}