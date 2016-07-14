use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::{Arc, Mutex, Weak};
use ::{EngineError, EngineEvent, HANDLER_LOCK_POISONED, Packet};
use connection::{Connection, State};
use eventual::{Async, Future};
use url::Url;
use uuid::Uuid;

type Callbacks = Arc<Mutex<CallbacksDictionary>>;
type CallbacksDictionary = HashMap<Uuid, Box<FnMut(EngineEvent) + 'static + Send>>;

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
            handlers: Arc::new(Mutex::new(HashMap::new()))
        }
    }

    /// Initializes a new client and connects to the given endpoint.
    pub fn with_url<U: Borrow<Url>>(url: &U) -> Future<Client, EngineError> {
        let c = Client::new();
        c.connect(url).map(move |_| c)
    }

    /// Connects to the given endpoint, if the client isn't already connected.
    ///
    /// ## Returns
    /// A future whose result is `true` if the client wasn't connected
    /// before and a new connection has been established, otherwise `false`.
    /// In the case of `false` the future returns instantly without an async
    /// computation in the background.
    pub fn connect<U: Borrow<Url>>(&self, url: &U) -> Future<bool, EngineError> {
        if self.state() != State::Connected {
            let handlers = self.handlers.clone();
            let callback_b = Box::new(move |ev: EngineEvent| {
                for func in handlers.lock().expect(HANDLER_LOCK_POISONED).values_mut() {
                    func(ev.clone());
                }
            });
            self.connection.connect_with_default_if_none(url.borrow().clone(), callback_b)
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
    pub fn disconnect(&self) -> Future<bool, EngineError> {
        if self.state() == State::Connected {
            self.connection.disconnect().map(|_| true)
        } else {
            Future::of(false)
        }
    }

    /// Registers a callback for event receival.
    pub fn register<H: FnMut(EngineEvent) + 'static + Send>(&self, handler: H) -> Registration {
        let uuid = Uuid::new_v4();
        self.handlers.lock().expect(HANDLER_LOCK_POISONED).insert(uuid.clone(), Box::new(handler));
        Registration(Arc::downgrade(&self.handlers), uuid)
    }

    /// Sends a packet to the other endpoint.
    ///
    /// ## Remarks
    /// The method buffers the packet when one tries to send a
    /// packet while a connection upgrade is taking place.
    pub fn send(&self, packet: Packet) -> Future<(), EngineError> {
        self.send_all(vec![packet])
    }

    /// Sends all given packets to the other endpoint.
    ///
    /// ## Remarks
    /// The method buffers the packet when one tries to send a
    /// packet while a connection upgrade is taking place.
    pub fn send_all(&self, packets: Vec<Packet>) -> Future<(), EngineError> {
        self.connection.send_all(packets)
    }

    /// Gets the connection state.
    pub fn state(&self) -> State {
        self.connection.state()
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

/// Represents a callback registration. Use this to unregister
/// a previously registered engine.io callback.
///
/// The callback is _not_ unregistered when this struct is dropped.
#[derive(Clone)]
pub struct Registration(Weak<Mutex<CallbacksDictionary>>, Uuid);

impl Registration {
    /// Unregisters the callback from the engine.io client.
    pub fn unregister(self) {
        if let Some(dict) = self.0.upgrade() {
            dict.lock().expect(HANDLER_LOCK_POISONED).remove(&self.1);
        }
    }
}

impl Debug for Registration {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(formatter, "Registration(..., {:?})", self.1)
    }
}