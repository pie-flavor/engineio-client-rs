use std::borrow::Borrow;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::{Arc, Mutex};
use ::{Callbacks, EngineError, EngineEvent, HANDLER_LOCK_POISONED, Packet};
use connection::{Connection, State};
use eventual::{Async, Future};
use url::Url;

/// An instance of an engine.io connection.
#[derive(Clone)]
pub struct Client {
    connection: Connection,
    handlers: Callbacks
}

impl Client {
    /// Initializes a new client.
    pub fn new() -> Client {
        Client {
            connection: Connection::new(),
            handlers: Arc::new(Mutex::new(Vec::new()))
        }
    }

    /// Initializes a new client and connects to the given endpoint.
    pub fn with_url<U: Borrow<Url>>(url: &U) -> Future<Client, EngineError> {
        let mut c = Client::new();
        c.connect(url).map(move |_| c)
    }

    /// Connects to the given endpoint, if the client isn't already connected.
    ///
    /// ## Returns
    /// A future whose result is `true` if the client wasn't connected
    /// before and a new connection has been established, otherwise `false`.
    /// In the case of `false` the future returns instantly without an async
    /// computation in the background.
    pub fn connect<U: Borrow<Url>>(&mut self, url: &U) -> Future<bool, EngineError> {
        if !self.is_connected() {
            self.connection.connect_with_default_if_none(url.borrow().clone(), self.handlers.clone())
                           .map(|_| true)
        } else {
            Future::of(false)
        }
    }

    /// Gets the underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Disconnects the client from the endpoint.
    pub fn disconnect(&mut self) -> Future<bool, EngineError> {
        if self.is_connected() {
            self.connection.disconnect().map(|_| true)
        } else {
            Future::of(false)
        }
    }

    /// Returns whether the client is connected or not.
    pub fn is_connected(&self) -> bool {
        self.connection.state() == State::Connected
    }

    /// Registers a callback for event receival.
    pub fn register<H: FnMut(EngineEvent) + 'static + Send>(&self, handler: H) {
        self.handlers.lock().expect(HANDLER_LOCK_POISONED).push(Box::new(handler));
    }

    /// Sends a packet to the other endpoint.
    ///
    /// ## Remarks
    /// The method buffers the packet when one tries to send a
    /// packet while a connection upgrade is taking place.
    pub fn send(&mut self, packet: Packet) -> Future<(), EngineError> {
        self.send_all(vec![packet])
    }

    /// Sends all given packets to the other endpoint.
    ///
    /// ## Remarks
    /// The method buffers the packet when one tries to send a
    /// packet while a connection upgrade is taking place.
    pub fn send_all(&mut self, packets: Vec<Packet>) -> Future<(), EngineError> {
        self.connection.send_all(packets)
    }
}

impl Debug for Client {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(formatter, "Client {{ connection: {:?}, ... }}", self.connection)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.disconnect().await();
    }
}