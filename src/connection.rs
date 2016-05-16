use pool::Pool;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, SyncSender, sync_channel};
use super::{CALLBACK_DICTIONARY_POISONED, Packet};
use url::Url;
use ws::{Handler, Handshake, Result as WsResult, Sender};

pub mod events {
    pub const CLOSE: &'static str = "close";

    pub const MESSAGE: &'static str = "message";

    pub const OPEN: &'static str = "open";
}

pub type Callbacks = HashMap<String, Vec<Box<FnMut(Packet) + 'static + Send>>>;

/// An instance of an engine.io connection.
pub struct Connection<'a> {
    handlers: Arc<Mutex<Callbacks>>,
    namespace: String,
    pool: &'a Pool,
    rooms: HashSet<String>
}

impl<'a> Connection<'a> {
    pub fn new(pool: &'a Pool) -> Connection<'a> {
        let callbacks = Arc::new(Mutex::new(HashMap::new()));

        Connection {
            handlers: callbacks,
            namespace: "/".to_owned(),
            pool: pool,
            rooms: HashSet::new()
        }
    }

    pub fn connect<U: Borrow<Url>>(url: &U) {
    }

    pub fn enter<R: Borrow<str>>(&mut self, room: &R) -> bool {
        self.rooms.insert(room.borrow().to_owned())
    }

    pub fn disconnect(&mut self) {
    }

    pub fn leave<R: Borrow<str>>(&mut self, room: &R) -> bool {
        self.rooms.remove(room.borrow())
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn on<M: Borrow<str>, H: FnMut(Packet) + 'static + Send>(mut self, msg: &M, handler: H) -> Connection<'a> {
        self.register(msg, handler);
        self
    }

    pub fn register<M: Borrow<str>, H: FnMut(Packet) + 'static + Send>(&mut self, msg: &M, handler: H) {
        self.handlers.lock().expect("Failed to acquire handler lock.")
                     .entry(msg.borrow().to_owned())
                     .or_insert(Vec::new())
                     .push(Box::new(handler));
    }
}

impl<'a> Drop for Connection<'a> {
    fn drop(&mut self) {
        self.disconnect();
    }
}

pub struct SocketHandler {
    callback_array: Arc<Mutex<Vec<(Url, Arc<Mutex<Callbacks>>)>>>,
    callbacks: Option<Arc<Mutex<Callbacks>>>,
    client: Sender
}

impl SocketHandler {
    pub fn new(callbacks: Arc<Mutex<Vec<(Url, Arc<Mutex<Callbacks>>)>>>, client: Sender) -> SocketHandler {
        SocketHandler {
            callback_array: callbacks,
            callbacks: None,
            client: client
        }
    }

    fn set_callbacks(&mut self, shake: &Handshake) {
        let callbacks = self.callback_array.lock().expect(CALLBACK_DICTIONARY_POISONED);
    }
}

impl Handler for SocketHandler {
    fn on_open(&mut self, shake: Handshake) -> WsResult<()> {
        self.set_callbacks(&shake);
        Ok(())
    }
}