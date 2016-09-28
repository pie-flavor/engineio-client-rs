//! The various transports used to connect to an engine.io server.
//!
//! You generally don't want to use this directly, you'll probably
//! want to use higher level abstractions that merge the transports
//! under one API and support upgrading from one to another.
//!
//! See the modules for further documentation.

pub mod polling;
pub mod websocket;

use std::time::Duration;

/// Represents the transport configuration that is received
/// during the handshake.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Default, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub struct Config {
    pingInterval: u32,
    pingTimeout: u32,
    sid: String,
    upgrades: Vec<String>
}

impl Config {
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