//! An engine.io client library written in and for Rust.
//!
//! This library tries to mimic the look and feel of the JS
//! client by using a similar syntax.

#![feature(custom_derive, io, mpsc_select)]

extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate rustc_serialize;
extern crate url;
extern crate ws;

mod client;
mod connection;
mod error;
mod packet;
mod transports;

pub use client::Client;
pub use error::EngineError;
pub use packet::{OpCode, Packet, Payload};

const HANDLER_LOCK_POISONED: &'static str = "Failed to acquire handler lock.";

#[derive(Debug)]
pub enum EngineEvent<'a> {
    /// Fired when an engine.io connection is made.
    Connect,

    /// Fired when an engine.io connection could not be established.
    ConnectError(&'a EngineError),

    /// Fired when the connection is disconnected.
    Disconnect,

    /// Fired when the connection is disconnected due to an error.
    Error(&'a EngineError),

    /// Fired when a message is sent over the connection.
    Message(&'a Packet)
}