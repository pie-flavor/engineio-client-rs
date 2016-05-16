//! An engine.io client library written in and for Rust
//!
//! This library tries to mimic the look and feel of the JS
//! client by using a similar syntax.

#![feature(custom_derive)]

extern crate rustc_serialize;
extern crate url;
extern crate ws;

mod connection;
mod packet;
mod pool;
mod transports;

pub use connection::Connection;
pub use packet::{OpCode, Packet, ParseError, Payload};
pub use pool::Pool;

const CALLBACK_DICTIONARY_POISONED: &'static str = "Failed to acquire mutex around callback dictionary. It was poisoned.";
