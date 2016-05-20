use std::borrow::Borrow;
use std::sync::{Arc, Mutex};
use ::{EngineError, HANDLER_LOCK_POISONED};
use packet::Packet;
use url::Url;

pub type Callbacks = Arc<Mutex<Vec<Box<FnMut(ConnectionEvent) + 'static + Send>>>>;

#[derive(Debug)]
pub enum ConnectionEvent<'a> {
    /// Fired when an engine.io connection is made.
    Connect,

    /// Fired when an engine.io connection could not be established.
    ConnectError(&'a EngineError),

    /// Fired when the connection is disconnected.
    Disconnect,

    /// Fired when the connection is disconnected due to an error.
    Error(&'a EngineError),

    /// Fired when a message is sent over the connection.
    Message(&'a Packet)
}

/// An instance of an engine.io connection.
pub struct Connection {
    handlers: Callbacks
}

impl Connection {
    pub fn new() -> Connection {
        Connection {
            handlers: Arc::new(Mutex::new(Vec::new()))
        }
    }

    pub fn connect<U: Borrow<Url>>(_: &U) {
    }

    pub fn disconnect(&mut self) {
    }

    pub fn register<H: FnMut(ConnectionEvent) + 'static + Send>(&mut self, handler: H) {
        self.handlers.lock().expect(HANDLER_LOCK_POISONED).push(Box::new(handler));
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.disconnect();
    }
}