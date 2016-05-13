extern crate url;

use pool::Pool;
use self::url::Url;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, SyncSender, sync_channel};
use std::thread::{Builder, JoinHandle};
use super::{Message, SocketCreationError};

/// An instance of a socket.io socket.
pub struct Socket<'a> {
    handlers: Arc<Mutex<HashMap<String, Vec<Box<FnMut(Message) + 'a>>>>>,
    namespace: String,
    rooms: HashSet<String>,
    thread_data: Option<(SyncSender<()>, JoinHandle<()>)>
}

impl<'a> Socket<'a> {
    pub fn new(pool: &mut Pool) -> Result<Socket<'a>, SocketCreationError> {
        let (ct_tx, ct_rx) = sync_channel(0);
        let callbacks = Arc::new(Mutex::new(HashMap::new()));
        let cb_clone = callbacks.clone();

        let handler = move || {
            //let callbacks = cb_clone;
            let (ev_tx, ev_rx) = channel::<Message>();

            select! {
                _ = ct_rx.recv() => return (),
                _ = ev_rx.recv() => return ()
            }
        };

        let join_handle = try!(Builder::new().name("Socket.io worker thread".to_owned())
                                             .spawn(handler)
                                             .map_err(|err| SocketCreationError::IoError(err)));
        Ok(Socket {
            handlers: callbacks,
            namespace: String::new(),
            rooms: HashSet::new(),
            thread_data: Some((ct_tx, join_handle))
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

    pub fn on<M: Borrow<str>, H: FnMut(Message) + 'a>(mut self, msg: M, handler: H) -> Socket<'a> {
        self.register(msg, handler);
        self
    }

    pub fn register<M: Borrow<str>, H: FnMut(Message) + 'a>(&mut self, msg: M, handler: H) {
        self.handlers.lock().expect("Failed to acquire handler lock.")
                     .entry(msg.borrow().to_owned())
                     .or_insert(Vec::new())
                     .push(Box::new(handler));
    }
}

impl<'a> Drop for Socket<'a> {
    fn drop(&mut self) {
        let thread_data = self.thread_data.take().expect("Option dance for join handle failed.");
        thread_data.0.send(()).expect("Socket.io cancellation channel was disconnected.");
        thread_data.1.join().expect("Waiting for the worker thread to shut down failed.");
    }
}

pub struct SocketHandler;

impl Handler for SocketHandler {

}