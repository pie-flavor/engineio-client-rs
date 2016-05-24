use std::borrow::Borrow;
use std::sync::{Arc, Mutex};
use ::{EngineEvent, HANDLER_LOCK_POISONED};
use connection::Connection;
use url::Url;

pub type Callbacks = Arc<Mutex<Vec<Box<FnMut(EngineEvent) + 'static + Send>>>>;

/// An instance of an engine.io connection.
pub struct Client {
    connection: Option<Connection>,
    handlers: Callbacks
}

impl Client {
    pub fn new() -> Client {
        Client {
            connection: None,
            handlers: Arc::new(Mutex::new(Vec::new()))
        }
    }

    pub fn connect<U: Borrow<Url>>(_: &U) {
        unimplemented!()
    }

    pub fn disconnect(&mut self) {
        unimplemented!()
    }

    pub fn reconnect(&mut self) {
        unimplemented!()
    }

    pub fn register<H: FnMut(EngineEvent) + 'static + Send>(&mut self, handler: H) {
        self.handlers.lock().expect(HANDLER_LOCK_POISONED).push(Box::new(handler));
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.disconnect();
    }
}