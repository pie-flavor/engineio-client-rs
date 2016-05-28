#![allow(dead_code)]

mod polling;
mod websocket;

use std::fmt::Debug;
use std::sync::mpsc::Sender;
use std::time::Duration;
use ::EngineError;
use packet::Packet;

pub use self::polling::Polling;
pub use self::websocket::*;

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
    fn close(&mut self, Sender<Result<(), EngineError>>) -> Result<(), EngineError>;

    /// Pauses the transport so that the buffers are flushed and
    /// no more messages are sent.
    fn pause(&mut self) -> Result<(), EngineError>;

    /// Sends a list of messages through the transport.
    fn send(&mut self, Vec<Packet>) -> Result<(), EngineError>;

    /// Restarts the transport when it has been paused.
    fn start(&mut self) -> Result<(), EngineError>;
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