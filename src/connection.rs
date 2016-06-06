use super::*;
use ::HANDLER_LOCK_POISONED;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, RwLock};
use transports::*;
use url::Url;

const CONNECTION_STATE_POISONED: &'static str = "Failed to mutably lock connection state rw-lock.";

/// Represents a connection to an engine.io server over a
/// variety of transports.
///
/// This struct manages the connection setup and upgrade from
/// long polling to websockets. It is also responsible for enqueueing
/// the messages while a transport is paused and upgraded and
/// sending the buffered messages when the upgrade is finished.
///
/// Right now this does nothing but forward all messages to
/// the long polling transport.
pub struct Connection {
    callbacks: Option<Callbacks>,
    cfg: Arc<RwLock<Option<Config>>>,
    transport: Option<Polling>,
    state: Arc<RwLock<ConnectionState>>,
    url: Option<Url>
}

impl Connection {
    /// Initializes a new connection to the specified endpoint.
    ///
    /// The path (default: `/engine.io/` for engine.io transports and
    /// `/socket.io/` for socket.io transports) must already be set.
    pub fn new() -> Connection {
        Connection {
            callbacks: None,
            cfg: Arc::new(RwLock::new(None)),
            transport: None,
            state: Arc::new(RwLock::new(ConnectionState::Pending)),
            url: None
        }
    }

    /// Closes the current connection, if one is present, and opens up
    /// a new one to the specified URL.
    pub fn connect(&mut self, url: Url, callbacks: Callbacks) {
        assert!(!url.cannot_be_a_base(), "URL must be able to be a base.");
        assert!(url.scheme() == "http" || url.scheme() == "https", "Url must be an HTTP or HTTPS url.");
        assert!(!url.path().is_empty(), "Path must be set.");

        self.callbacks = Some(callbacks.clone());
        self.url = Some(url.clone());
        if let Some(mut transport) = self.transport.take() {
            let _ = transport.close();
        }
        self.transport = Some(create_connection(url, callbacks, self.state.clone(), self.cfg.clone()));
    }

    /// Initializes a new connection to the `/engine.io/`-path of the specified endpoint.
    pub fn connect_with_default(&mut self, url: Url, callbacks: Callbacks) {
        self.connect_with_path(url, "/engine.io/", callbacks);
    }

    /// Initializes a new connection to the default path if there isn't
    /// one already inside the URL.
    pub fn connect_with_default_if_none(&mut self, url: Url, callbacks: Callbacks) {
        if url.path().is_empty() {
            self.connect_with_default(url, callbacks);
        } else {
            self.connect(url, callbacks);
        }
    }

    /// Initializes a new connection to the specified path of the endpoint.
    pub fn connect_with_path(&mut self, mut url: Url, path: &str, callbacks: Callbacks) {
        url.set_path(path);
        self.connect(url, callbacks);
    }

    /// Disconnects the connection.
    pub fn disconnect(&mut self) {
        if let Some(mut transport) = self.transport.take() {
            let _ = transport.close();
        }
    }

    /// Sends a packet to the other endpoint.
    ///
    /// ## Remarks
    /// The method buffers the packet when one tries to send a
    /// packet while a connection upgrade is taking place.
    pub fn send(&mut self, packet: Packet) -> Result<(), EngineError> {
        if let Some(ref mut transport) = self.transport {
            transport.send(vec![packet])
        } else {
            Err(EngineError::invalid_state("Connection was not connected."))
        }
    }

    /// Gets the connection state.
    pub fn state(&self) -> ConnectionState {
        *self.state.read().expect(CONNECTION_STATE_POISONED).deref()
    }
}

impl Debug for Connection {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(
            formatter,
            "Connection {{ callbacks: ..., cfg: {:?}, transport: {:?}, state: {:?}, url: {:?} }}",
            self.cfg, self.transport, self.state, self.url
        )
    }
}

/// Represents the state a connection is in.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub enum ConnectionState {
    /// The connection is not connected.
    Disconnected,

    /// The connection is up and running and messages can be exchanged.
    Connected,

    /// The connection hasn't been set up yet.
    Pending
}

impl Default for ConnectionState {
    fn default() -> Self {
        ConnectionState::Pending
    }
}

fn create_connection(url: Url, callbacks: Callbacks, state: Arc<RwLock<ConnectionState>>, cfg: Arc<RwLock<Option<Config>>>) -> Polling {
    Polling::new(url.clone(), move |ev| {
        let on_disconnect = || {
            {
                let mut state_val = state.write().expect(CONNECTION_STATE_POISONED);
                *state_val = ConnectionState::Disconnected;
            }
            for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).deref_mut() {
                func(EngineEvent::Disconnect);
            }
        };

        match ev {
            EngineEvent::Connect(c) => {
                {
                    let mut cfg_val = cfg.write().expect("Failed to lock configuration lock.");
                    *cfg_val = Some(c.clone_custom());
                } {
                    let mut state_val = state.write().expect(CONNECTION_STATE_POISONED);
                    *state_val = ConnectionState::Connected;
                }
                for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).deref_mut() {
                    func(EngineEvent::Connect(c));
                }
            },
            EngineEvent::ConnectError(err) => {
                for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).deref_mut() {
                    func(EngineEvent::ConnectError(err));
                }
            },
            EngineEvent::Disconnect => on_disconnect(),
            EngineEvent::Error(ref err) => {
                {
                    let mut state_val = state.write().expect(CONNECTION_STATE_POISONED);
                    *state_val = ConnectionState::Disconnected;
                }
                for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).deref_mut() {
                    func(EngineEvent::Error(err));
                }
            },
            EngineEvent::Message(ref pck) => {
                match pck.opcode() {
                    OpCode::Close => on_disconnect(),
                    OpCode::Message | OpCode::Pong => {
                        for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).deref_mut() {
                            func(EngineEvent::Message(pck));
                        }
                    },
                    OpCode::Noop => {},
                    o @ OpCode::Open |
                    o @ OpCode::Ping |
                    o @ OpCode::Upgrade => unreachable!("Given opcode {:?} should never reach the connection struct.", o)
                }
            },
            _ => unreachable!()
        }
    })
}