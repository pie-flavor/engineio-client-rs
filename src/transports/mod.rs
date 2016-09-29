//! The various transports used to connect to an engine.io server.
//!
//! You generally don't want to use this directly, you'll probably
//! want to use higher level abstractions that merge the transports
//! under one API and support upgrading from one to another.
//!
//! See the modules for further documentation.

pub mod polling;
pub mod websocket;

use std::cell::RefCell;
use std::time::Duration;

use rand::{Rng, weak_rng, XorShiftRng};
use url::Url;

const TRANSPORT_PAUSED: &'static str = "Transport is paused. Unpause it before sending packets again.";

thread_local!(static RNG: RefCell<XorShiftRng> = RefCell::new(weak_rng()));

/// Represents the transport configuration that is received
/// during the handshake.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Default, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub struct Data {
    pingInterval: u32,
    pingTimeout: u32,
    sid: String,
    upgrades: Vec<String>
}

impl Data {
    /// Modifies the given URL with the information in this struct.
    pub fn apply_to(&self, url: &mut Url) {
        url.query_pairs_mut()
           .append_pair("EIO", "3")
           .append_pair("sid", &self.sid)
           .append_pair("t", &RNG.with(|rc| rc.borrow_mut().gen_ascii_chars().take(7).collect::<String>()))
           .append_pair("b64", "1");
    }

    /// Gets the interval that states how often the server shall be pinged.
    pub fn ping_interval(&self) -> Duration {
        Duration::from_millis(self.pingInterval as u64)
    }

    /// The ping timeout.
    pub fn ping_timeout(&self) -> Duration {
        Duration::from_millis(self.pingTimeout as u64)
    }

    /// The current engine.io session ID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Available upgrades.
    pub fn upgrades(&self) -> &[String] {
        &self.upgrades
    }
}