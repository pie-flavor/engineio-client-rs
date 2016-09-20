//! An engine.io client library written in and for Rust.
//!
//! The first goal of this library is to reach feature-complete
//! status and full interoperability with the JS implementation.
//! Performance is always being worked on, though not focused on
//! primarily. Major improvements in that area can come once the
//! library is working properly and stable.

#![crate_name = "engineio"]
#![crate_type = "lib"]
#![feature(never_type)]
//#![deny(dead_code, missing_docs, unused_variables)]

extern crate futures;
#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate rustc_serialize;
extern crate tokio_core;
extern crate tokio_request;
extern crate url;
extern crate ws;

mod connection;
mod error;
mod handler;
mod packet;
mod transports;