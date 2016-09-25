use std::cell::RefCell;
use std::fmt::Debug;
use std::time::Duration;

use packet::Packet;
use rand::{Rng, weak_rng, XorShiftRng};
use tokio_request::Request;

mod polling;

thread_local!(static RNG: RefCell<XorShiftRng> = RefCell::new(weak_rng()));

/// Represents an engine.io transport.
///
/// A transport is a way of connecting an engine.io server to
/// a client. At the moment, this library supports HTTP long
/// polling (which is used to initialize a connection) and
/// web sockets. Since using web sockets is not always possible,
/// the upgrade to will only be done if both parties can really
/// communicate over the socket.
pub trait Transport : Debug {
    /// Asynchronously closes the transport.
    fn close(&self) -> ();

    /// Pauses the transport so that the buffers are flushed and
    /// no more messages are sent.
    fn pause(&self) -> ();

    /// Sends a list of messages through the transport.
    fn send(&self, Vec<Packet>) -> ();

    /// Restarts the transport when it has been paused.
    fn start(&self) -> ();
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Default, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub struct Config {
    pingInterval: u32,
    pingTimeout: u32,
    sid: String,
    upgrades: Vec<String>
}

impl Config {
    pub fn ping_interval(&self) -> Duration {
        Duration::from_millis(self.pingInterval as u64)
    }

    pub fn ping_timeout(&self) -> Duration {
        Duration::from_millis(self.pingTimeout as u64)
    }

    pub fn sid(&self) -> &str {
        &self.sid
    }

    pub fn upgrades(&self) -> &[String] {
        &self.upgrades
    }
}

fn prepare_request(mut request: Request, conn_cfg: &::Config, tp_cfg: Option<&Config>) -> Request {
    request = request.param("EIO", "3")
                     .param("transport", "polling")
                     .param("t", &RNG.with(|rc| rc.borrow_mut().gen_ascii_chars().take(7).collect::<String>()))
                     .param("b64", "1");
    if let Some(cfg) = tp_cfg {
        request = request.param("sid", &cfg.sid);
    }
    if let Some(ref headers) = conn_cfg.extra_headers {
        for (key, value) in headers.iter() {
            request = request.header(key, value);
        }
    }
    if let Some(ref ua) = conn_cfg.user_agent {
        request = request.header("User-Agent", ua);
    }
    request
}