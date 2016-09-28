//! The websocket transport.
//!
//! Websockets are fast, but not as reliable as HTTP long polling
//! in regards to company firewalls, etc. This is why engine.io sets
//! up connections using HTTP long polling and then switches over to
//! websockets if possible.

use {Config as ConnectionConfig};
use transports::Config as TransportConfig;
use ws;

pub struct Receiver;

pub struct Sender;