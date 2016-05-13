//! A socket.io client library written in and for Rust
//!
//! This library tries to mimic the look and feel of the JS
//! client by using a similar syntax.

#![feature(custom_derive, mpsc_select)]

mod pool;
mod socket;
mod transports;

pub use pool::Pool;
pub use socket::Socket;

pub enum Message<'a> {
    Binary(&'a [u8]),

    String(&'a str)
}

pub enum SocketCreationError {
    MissingHost,

    IoError(::std::io::Error)
}
