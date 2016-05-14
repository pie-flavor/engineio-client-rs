//! An engine.io client library written in and for Rust
//!
//! This library tries to mimic the look and feel of the JS
//! client by using a similar syntax.

#![feature(custom_derive, mpsc_select)]

mod pool;
mod socket;
mod transports;

pub use pool::Pool;
pub use socket::Socket;

#[derive(Clone, Debug)]
pub struct Message<'a> {
    op_code: OpCode,
    payload: Payload<'a>
}

#[derive(Copy, Clone, Debug)]
pub enum OpCode {
    Open = 0,
    Close = 1,
    Ping = 2,
    Pong = 3,
    Message = 4,
    Upgrade = 5,
    Noop = 6
}

#[derive(Copy, Clone, Debug)]
pub enum Payload<'a> {
    Binary(&'a [u8]),

    String(&'a str)
}

pub enum SocketCreationError {
    MissingHost,

    IoError(::std::io::Error)
}
