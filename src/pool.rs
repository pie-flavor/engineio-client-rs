//! This library uses [ws-rs](https://github.com/housleyjk/ws-rs) as underlying
//! websocket implementation. `ws-rs` in turn makes use of [mio](https://github.com/carllerche/mio)
//! to allow asynchronous callbacks. Every time a new websocket instance is created,
//! a new mio event loop and a separate thread are also created.
//!
//! To avoid the overhead of creating lots of threads, the library uses a pool to
//! manage the sockets. Creating a pool creates a mio event loop which does the I/O
//! and runs the callbacks.
//!
//! If the callbacks are computationally intensive, consider using multiple pools
//! or separate worker threads and channels to avoid blocking the event loop for
//! too long.

extern crate ws;

use self::ws::*;
use socket::SocketHandler;
use std::marker::{Send, Sync};

/// A socket.io connection pool.
pub struct Pool {
}

impl Pool {
}

unsafe impl Send for Pool { }

unsafe impl Sync for Pool { }

struct SocketFactory {

}

impl Factory for SocketFactory {
    type Handler = SocketHandler;

    fn connection_made(&mut self, output: Sender) -> Self::Handler {
        SocketHandler {

        }
    }
}