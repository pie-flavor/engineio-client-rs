//! An engine.io client library written in and for Rust.
//!
//! This library tries to mimic the look and feel of the JS
//! client by using a similar syntax.

#![feature(custom_derive, mpsc_select)]

extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate rustc_serialize;
extern crate url;
extern crate ws;

mod connection;
mod error;
mod packet;
mod transports;

pub use connection::Connection;
pub use error::EngineError;
pub use packet::{OpCode, Packet, Payload};

const HANDLER_LOCK_POISONED: &'static str = "Failed to acquire handler lock.";