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
//!
//! Also, it is **very important** that the callbacks never panic. A panicing
//! callback causes the Pool to go down and disconnect every other connection
//! as well.

extern crate ws;

use self::ws::*;
use self::ws::Result as WsResult;
use socket::SocketHandler;
use std::collections::HashMap;
use std::marker::{Send, Sync};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, Sender as ChSender, SyncSender, sync_channel};
use std::thread::{JoinHandle, spawn};

/// A socket.io connection pool. It can be cloned to be sent across
/// thread boundaries.
#[derive(Clone)]
pub struct Pool {
    cb_tx: ChSender<Arc<Mutex<HashMap<String, Vec<Box<FnMut(Message) + 'static + Send>>>>>>,
    //thread_data: Option<(SyncSender<()>, JoinHandle<()>)>,
    ws: Arc<Mutex<WebSocket<SocketFactory>>>
}

impl Pool {
    pub fn new() -> WsResult<Pool> {
        Pool::with_settings(Default::default())
    }

    pub fn with_settings(settings: Settings) -> WsResult<Pool> {
        let (cb_tx, cb_rx) = channel();
        let ws = try!(Builder::new().with_settings(settings).build(SocketFactory { cb_rx: cb_rx }));

        Ok(Pool {
            cb_tx: cb_tx,
            ws: Arc::new(Mutex::new(ws))
        })
    }
}

unsafe impl Send for Pool { }

unsafe impl Sync for Pool { }

struct SocketFactory {
    cb_rx: Receiver<Arc<Mutex<HashMap<String, Vec<Box<FnMut(Message) + 'static + Send>>>>>>
}

impl Factory for SocketFactory {
    type Handler = SocketHandler;

    fn connection_made(&mut self, output: Sender) -> Self::Handler {
        SocketHandler {

        }
    }
}