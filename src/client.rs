use std::borrow::Borrow;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::{Arc, Mutex};
use ::{Callbacks, EngineEvent, HANDLER_LOCK_POISONED};
use connection::{Connection, ConnectionState};
use url::Url;

/// An instance of an engine.io connection.
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
    pub fn with_url<U: Borrow<Url>>(url: &U) -> Client {
        let mut c = Client::new();
        c.connect(url);
        c
    }

    /// Connects to the given endpoint, if the client isn't already connected.
    ///
    /// ## Returns
    /// `true` if the client wasn't connected before and a new connection has
    /// been established, otherwise `false`.
    pub fn connect<U: Borrow<Url>>(&mut self, url: &U) -> bool {
        if !self.is_connected() {
            self.connection.connect_with_default_if_none(url.borrow().clone(), self.handlers.clone());
            true
        } else {
            false
        }
    }

    /// Gets the underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Disconnects the client from the endpoint.
    pub fn disconnect(&mut self) {
        self.connection.disconnect();
    }

    /// Returns whether the client is connected or not.
    pub fn is_connected(&self) -> bool {
        self.connection.state() == ConnectionState::Connected
    }

    /// Registers a callback for event receival.
    pub fn register<H: FnMut(EngineEvent) + 'static + Send>(&self, handler: H) {
        self.handlers.lock().expect(HANDLER_LOCK_POISONED).push(Box::new(handler));
    }
}

impl Debug for Client {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(formatter, "Client {{ connection: {:?}, ... }}", self.connection)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.disconnect();
    }
}