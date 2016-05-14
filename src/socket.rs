extern crate url;
extern crate ws;

use pool::Pool;
use self::url::Url;
use self::ws::Handler;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, SyncSender, sync_channel};
use std::thread::{Builder, JoinHandle};
use super::{Message, SocketCreationError};

/// An instance of a socket.io socket.
pub struct Socket {
    handlers: Arc<Mutex<HashMap<String, Vec<Box<FnMut(Message) + 'static + Send>>>>>,
    namespace: String,
    rooms: HashSet<String>
}

impl Socket {
    pub fn new(pool: &mut Pool) -> Result<Socket, SocketCreationError> {
        let callbacks = Arc::new(Mutex::new(HashMap::new()));
        let cb_clone = callbacks.clone();

        let handler = move || {
            //let callbacks = cb_clone;
            let (ev_tx, ev_rx) = channel::<Message>();

            select! {
                _ = ev_rx.recv() => return ()
            }
        };

        Ok(Socket {
            handlers: callbacks,
            namespace: String::new(),
            rooms: HashSet::new()
        })
    }

    pub fn connect<U: Borrow<Url>>(url: U) {
    }

    pub fn enter<R: Borrow<str>>(&mut self, room: R) -> bool {
        self.rooms.insert(room.borrow().to_owned())
    }

    pub fn disconnect(&mut self) {

    }

    pub fn leave<R: Borrow<str>>(&mut self, room: R) -> bool {
        self.rooms.remove(room.borrow())
    }

    pub fn on<M: Borrow<str>, H: FnMut(Message) + 'static + Send>(mut self, msg: M, handler: H) -> Socket {
        self.register(msg, handler);
        self
    }

    pub fn register<M: Borrow<str>, H: FnMut(Message) + 'static + Send>(&mut self, msg: M, handler: H) {
        self.handlers.lock().expect("Failed to acquire handler lock.")
                     .entry(msg.borrow().to_owned())
                     .or_insert(Vec::new())
                     .push(Box::new(handler));
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.disconnect();
    }
}

pub struct SocketHandler;

impl Handler for SocketHandler {

}