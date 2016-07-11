//! The websocket transport.
//!
//! Websockets are much faster than HTTP long polling, though
//! much less stable. Lots of company firewalls block websocket
//! traffic, so this library (and engine.io) takes great care to
//! only use them when they can be used properly.

// TODO: Make use of this

use super::{append_eio_parameters, Config, Transport};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use ::{EngineError, EngineEvent, Packet};
use url::Url;
use ws::{Builder, CloseCode, Error as WsError, Factory, Handler, Message, Result as WsResult, Sender as WsSender, Settings};

const CALLBACK_POISONED: &'static str = "Websocket callback lock poisoned.";

/// The websockets transport.
pub struct Socket {
    ct_rx: Receiver<()>,
    is_paused: bool,
    sender: WsSender
}

impl Socket {
    /// Creates a new instance of a websocket transport from a given
    /// configuration and automatically connects to the endpoint.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callback: C`: Callback to call when asynchronous events are ready.
    /// - `cfg: Config`: A transport configuration used to initialize session.
    pub fn new<C: FnMut(EngineEvent) + Send + 'static>(mut url: Url, callback: C, cfg: Config) -> Socket {
        append_eio_parameters(&mut url, Some(cfg.sid()));

        let (ct_tx, ct_rx) = channel();
        let mut ws = Builder::new().with_settings(Settings {
            key_strict: true,
            ..Default::default()
        }).build(SocketHandler::new(callback)).expect("Failed to set up websocket.");
        let broadcaster = ws.broadcaster();

        ws.connect(url).expect("Failed to enqueue websocket connection.");
        thread::spawn(move || {
            ws.run().expect("Failed to run mio event loop.");
            let _ = ct_tx.send(());
        });

        Socket {
            ct_rx: ct_rx,
            is_paused: false,
            sender: broadcaster
        }
    }

    fn do_send(&mut self, msgs: Vec<Packet>) -> Result<(), EngineError> {
        for packet in msgs {
            try!(self.sender.send(packet));
        }
        Ok(())
    }
}

impl Debug for Socket {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        write!(formatter, "Socket {{ is_paused: {}, ... }}", self.is_paused)
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl Transport for Socket {
    fn close(&self) -> Result<(), EngineError> {
        self.sender.shutdown().map_err(|err| err.into())
    }

    fn pause(&self) -> Result<(), EngineError> {
        self.is_paused = true;
        Ok(())
    }

    fn send(&self, msgs: Vec<Packet>) -> Result<(), EngineError> {
        if !self.is_paused {
            try!(self.do_send(msgs))
        }
        Ok(())
    }

    fn start(&self) -> Result<(), EngineError> {
        self.is_paused = false;
        Ok(())
    }
}

struct SocketHandler<C>(Arc<Mutex<C>>);

impl<C> SocketHandler<C> {
    pub fn new(callback: C) -> Self {
        SocketHandler(Arc::new(Mutex::new(callback)))
    }
}

impl<C> ::std::clone::Clone for SocketHandler<C> { // #[derive(Clone)] doesn't work
    fn clone(&self) -> Self {
        SocketHandler(self.0.clone())
    }
}

impl<C> Factory for SocketHandler<C>
    where C: FnMut(EngineEvent) + Send + 'static {
    type Handler = Self;

    fn connection_made(&mut self, _: WsSender) -> Self::Handler {
        self.clone()
    }
}

impl<C> Handler for SocketHandler<C>
    where C: FnMut(EngineEvent) + Send + 'static {
    fn on_close(&mut self, _: CloseCode, _: &str) {
        let mut guard = self.0.lock().expect(CALLBACK_POISONED);
        guard.deref_mut()(EngineEvent::Disconnect);
    }

    fn on_error(&mut self, err: WsError) {
        let mut guard = self.0.lock().expect(CALLBACK_POISONED);
        guard.deref_mut()(EngineEvent::Error(&EngineError::WebSocket(err)));
    }

    fn on_message(&mut self, msg: Message) -> WsResult<()> {
        if let Message::Text(str) = msg {
            if let Ok(pck) = Packet::from_str(&str) {
                let mut guard = self.0.lock().expect(CALLBACK_POISONED);
                guard.deref_mut()(EngineEvent::Message(&pck));
            }
        }
        Ok(())
    }
}