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
//! Also, it is **very important** that the callbacks never panic. A panicking
//! callback causes the Pool to go down and disconnect every other connection
//! as well.

use connection::{Callbacks, SocketHandler};
use std::marker::{Send, Sync};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{JoinHandle, spawn};
use super::CALLBACK_DICTIONARY_POISONED;
use url::Url;
use ws::*;
use ws::Result as WsResult;

/// A socket.io connection pool. It can be cloned to be sent across
/// thread boundaries.
pub struct Pool {
    callbacks: Arc<Mutex<Vec<(Url, Arc<Mutex<Callbacks>>)>>>,
    controller: Option<Sender>,
    thread_handle: Option<JoinHandle<WsResult<WebSocket<SocketFactory>>>>
}

impl Pool {
    pub fn new() -> WsResult<Pool> {
        Pool::with_settings(Default::default())
    }

    pub fn with_settings(settings: Settings) -> WsResult<Pool> {
        let callbacks = Arc::new(Mutex::new(Vec::new()));
        let ws = try!(Builder::new().with_settings(settings).build(SocketFactory(callbacks.clone())));

        Ok(Pool {
            callbacks: callbacks,
            controller: Some(ws.broadcaster()),
            thread_handle: Some(spawn(move || ws.run()))
        })
    }

    pub fn queue_connection(&self, url: Url, cb: Arc<Mutex<Callbacks>>) -> WsResult<()> {
        match self.controller {
            Some(ref controller) => {
                let mut callbacks = self.callbacks.lock().expect(CALLBACK_DICTIONARY_POISONED);
                callbacks.push((url.clone(), cb));
                controller.connect(url)
            },
            None => panic!("Cannot queue a new connection. Pool was already shut down.")
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(controller) = self.controller.take() {
            controller.shutdown().unwrap();
        }
        if let Some(jh) = self.thread_handle.take() {
            jh.join().unwrap().unwrap();
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

unsafe impl Send for Pool { }

unsafe impl Sync for Pool { }

struct SocketFactory(Arc<Mutex<Vec<(Url, Arc<Mutex<Callbacks>>)>>>);

impl Factory for SocketFactory {
    type Handler = SocketHandler;

    fn connection_made(&mut self, output: Sender) -> Self::Handler {
        SocketHandler::new(self.0.clone(), output)
    }
}