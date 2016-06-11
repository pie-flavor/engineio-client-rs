use super::*;
use ::HANDLER_LOCK_POISONED;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, RwLock};
use eventual::{Async, Future};
use transports::*;
use url::Url;

const CONFIG_POISONED: &'static str = "Failed to mutably lock connection config rw-lock.";
const CONNECTION_STATE_POISONED: &'static str = "Failed to mutably lock connection state rw-lock.";
const STATE_POISONED: &'static str = "Failed to lock internal state.";

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
#[derive(Clone, Debug)]
pub struct Connection(Arc<Mutex<ConnectionState>>);

impl Connection {
    /// Initializes a new connection to the specified endpoint.
    ///
    /// The path (default: `/engine.io/` for engine.io transports and
    /// `/socket.io/` for socket.io transports) must already be set.
    pub fn new() -> Connection {
        Connection(Arc::new(Mutex::new(ConnectionState {
            callbacks: None,
            cfg: Arc::new(RwLock::new(None)),
            transport: None,
            state: Arc::new(RwLock::new(State::Pending)),
            url: None
        })))
    }

    /// Closes the current connection, if one is present, and opens up
    /// a new one to the specified URL.
    pub fn connect(&self, url: Url, callbacks: Callbacks) -> Future<(), EngineError> {
        assert!(!url.cannot_be_a_base(), "URL must be able to be a base.");
        assert!(url.scheme() == "http" || url.scheme() == "https", "Url must be an HTTP or HTTPS url.");
        assert!(!url.path().is_empty(), "Path must be set.");

        let (connection_state, cfg) = {
            let s = self.0.lock().expect(STATE_POISONED);
            (s.state.clone(), s.cfg.clone())
        };
        let state = self.0.clone();

        create_connection(url.clone(), callbacks.clone(), connection_state, cfg).and_then(move |conn| {
            let mut state = state.lock().expect(STATE_POISONED);
            state.callbacks = Some(callbacks);
            {
                *state.cfg.write().expect(CONFIG_POISONED) = Some(conn.cfg().clone());
            }
            state.url = Some(url);

            if let Some(mut transport) = mem::replace(&mut state.transport, Some(conn)) {
                transport.close().fire();
            }
            Ok(())
        })
    }

    /// Initializes a new connection to the `/engine.io/`-path of the specified endpoint.
    pub fn connect_with_default(&self, url: Url, callbacks: Callbacks) -> Future<(), EngineError> {
        self.connect_with_path(url, "/engine.io/", callbacks)
    }

    /// Initializes a new connection to the default path if there isn't
    /// one already inside the URL.
    pub fn connect_with_default_if_none(&self, url: Url, callbacks: Callbacks) -> Future<(), EngineError> {
        if url.path().is_empty() {
            self.connect_with_default(url, callbacks)
        } else {
            self.connect(url, callbacks)
        }
    }

    /// Initializes a new connection to the specified path of the endpoint.
    pub fn connect_with_path(&self, mut url: Url, path: &str, callbacks: Callbacks) -> Future<(), EngineError> {
        url.set_path(path);
        self.connect(url, callbacks)
    }

    /// Gets the connection config.
    pub fn config(&self) -> Option<Config> {
        let internal_state = self.0.lock().expect(STATE_POISONED);
        let guard = internal_state.cfg.read().expect(CONFIG_POISONED);
        guard.clone()
    }

    /// Disconnects the connection.
    ///
    /// ## Returns
    /// The return value of the future indicates whether the
    /// connection really has been closed or whether no operation
    /// has been performed because there was no connection to
    /// disconnect in the first place.
    pub fn disconnect(&self) -> Future<bool, EngineError> {
        if let Some(mut transport) = self.0.lock().expect(STATE_POISONED).transport.take() {
            transport.close().map(|_| true)
        } else {
            Future::of(false)
        }
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
        if let Some(ref mut transport) = self.0.lock().expect(STATE_POISONED).transport {
            transport.send(packets)
        } else {
            Future::error(EngineError::invalid_state("Connection was not connected."))
        }
    }

    /// Gets the connection state.
    pub fn state(&self) -> State {
        let internal_state = self.0.lock().expect(STATE_POISONED);
        let guard = internal_state.state.read().expect(CONNECTION_STATE_POISONED);
        *guard.deref()
    }
}

/// Represents the state a connection is in.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub enum State {
    /// The connection is not connected.
    Disconnected,

    /// The connection is up and running and messages can be exchanged.
    Connected,

    /// The connection hasn't been set up yet.
    Pending
}

impl Default for State {
    fn default() -> Self {
        State::Pending
    }
}

struct ConnectionState {
    callbacks: Option<Callbacks>,
    cfg: Arc<RwLock<Option<Config>>>,
    transport: Option<Polling>,
    state: Arc<RwLock<State>>,
    url: Option<Url>
}

impl Debug for ConnectionState {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(
            formatter,
            "Connection {{ callbacks: ..., cfg: {:?}, transport: {:?}, state: {:?}, url: {:?} }}",
            self.cfg, self.transport, self.state, self.url
        )
    }
}

fn create_connection(url: Url, callbacks: Callbacks, state: Arc<RwLock<State>>, cfg: Arc<RwLock<Option<Config>>>) -> Future<Polling, EngineError> {
    Polling::new(url.clone(), move |ev| {
        let on_disconnect = || {
            {
                let mut state_val = state.write().expect(CONNECTION_STATE_POISONED);
                *state_val = State::Disconnected;
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
                    *state_val = State::Connected;
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
                    *state_val = State::Disconnected;
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