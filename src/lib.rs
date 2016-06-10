//! An engine.io client library written in and for Rust.
//!
//! The first goal of this library is to reach feature-complete
//! status and full interoperability with the JS implementation.
//! Performance is not one of the main goals, although it can be
//! expected that there will be improvements once the library
//! is working.
//! In practice, this means that this library creates and destroys
//! lots of background threads to perform all kinds of tasks.
//! This will improve once better async IO (gogo #1081) is there.

#![crate_name = "engineio"]
#![crate_type = "lib"]
#![feature(custom_derive, io, mpsc_select)]

pub extern crate eventual;
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

use std::sync::{Arc, Mutex};

pub use client::Client;
pub use connection::Connection;
pub use error::EngineError;
pub use packet::{OpCode, Packet, Payload};

const HANDLER_LOCK_POISONED: &'static str = "Failed to acquire handler callbacks lock.";

pub type Callbacks = Arc<Mutex<Vec<Box<FnMut(EngineEvent) + 'static + Send>>>>;

/// An event that can occur within a connection.
#[derive(Debug)]
pub enum EngineEvent<'a> {
    /// Fired when an engine.io connection is made.
    Connect(&'a transports::Config),

    /// Fired when an engine.io connection could not be established.
    ConnectError(&'a EngineError),

    /// Fired when the connection is disconnected.
    Disconnect,

    /// Fired when the connection is disconnected due to an error.
    Error(&'a EngineError),

    /// Fired when a message is sent over the connection.
    Message(&'a Packet),

    #[doc(hidden)]
    __Nonexhaustive(Void)
}

#[doc(hidden)]
#[derive(Debug)]
pub enum Void {}