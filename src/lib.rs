//! An engine.io client library written in and for Rust.

#![allow(dead_code)]
#![warn(missing_docs)]
#![feature(io)]

#![cfg_attr(release, deny(warnings))]

extern crate futures;
extern crate rand;
extern crate rustc_serialize;
extern crate tokio_core;
extern crate tokio_request;
extern crate url;
extern crate ws;

mod builder;
mod connection;
mod packet;
pub mod transports;

use std::io::Error;

use futures::BoxFuture;
use tokio_core::reactor::Handle;
use url::Url;

pub use builder::Builder;
pub use connection::{Receiver, Sender};
pub use packet::{OpCode, Packet, Payload};

/// Creates an engine.io connection to the given endpoint.
pub fn connect(url: &Url, h: &Handle) -> BoxFuture<(Sender, Receiver), Error> {
    Builder::new(url.clone()).build(h)
}

/// Creates an engine.io connection to the given endpoint.
pub fn connect_str(url: &str, h: &Handle) -> BoxFuture<(Sender, Receiver), Error> {
    Builder::new_with_str(url).build(h)
}