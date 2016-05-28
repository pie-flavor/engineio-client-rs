use super::*;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use transports::*;
use url::Url;

/// Represents a connection to an engine.io server over a
/// variety of transports.
///
/// This struct manages the connection setup and upgrade from
/// long polling to websockets. It is also responsible for enqueueing
/// the messages while a transport is paused and upgraded and
/// sending the buffered messages when the upgrade is finished.
pub struct Connection {
    callbacks: Callbacks,
    p_buf: Vec<Packet>,
    transport: Option<Box<Transport>>,
    url: Url
}

impl Connection {
    /// Initializes a new connection to the specified endpoint.
    ///
    /// The path (default: `/engine.io/` for engine.io transports and
    /// `/socket.io/` for socket.io transports) must already be set.
    pub fn new(url: Url, callbacks: Callbacks) -> Connection {
        assert!(!url.cannot_be_a_base(), "URL must be able to be a base.");
        assert!(url.scheme() == "http" || url.scheme() == "https", "Url must be a HTTP or HTTPS url.");
        assert!(!url.path().is_empty(), "Path must be set.");

        Connection {
            callbacks: callbacks,
            p_buf: Vec::new(),
            transport: None,
            url: url
        }
    }

    /// Initializes a new connection to the `/engine.io/`-path of the specified endpoint.
    pub fn with_default(url: Url, callbacks: Callbacks) -> Connection {
        Connection::with_path(url, "/engine.io/", callbacks)
    }

    /// Initializes a new connection to the default path if there isn't
    /// one already inside the URL.
    pub fn with_default_if_none(url: Url, callbacks: Callbacks) -> Connection {
        if url.path().is_empty() {
            Connection::with_default(url, callbacks)
        } else {
            Connection::new(url, callbacks)
        }
    }

    /// Initializes a new connection to the specified path of the endpoint.
    pub fn with_path(mut url: Url, path: &str, callbacks: Callbacks) -> Connection {
        url.set_path(path);
        Connection::new(url, callbacks)
    }

    /// Sends a packet to the other endpoint.
    ///
    /// ## Remarks
    /// The method buffers the packet when one tries to send a
    /// packet while a connection upgrade is taking place.
    pub fn send(&self, packet: Packet) -> Result<(), EngineError> {
        assert!(self.transport.is_some(), "Cannot send the packet. Connection must be connected before sending packets.");
        unimplemented!()
    }
}

impl Debug for Connection {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(formatter, "Connection {{ p_buf: {:?}, url: {}, ... }}", self.p_buf, self.url)
    }
}