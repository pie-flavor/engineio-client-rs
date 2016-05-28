//! The websocket transport.
//!
//! Websockets are much faster than HTTP long polling, though
//! much less stable. Lots of company firewalls block websocket
//! traffic, so this library (and engine.io) takes great care to
//! only use them when they can be used properly.

use super::*;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use ::{EngineError, EngineEvent, OpCode, Packet};
use url::Url;
use ws::{Builder, connect, Error as WsError, Factory, Handler, Message, Result as WsResult, Sender as WsSender, Settings, WebSocket};

const CALLBACK_POISONED: &'static str = "Websocket callback lock poisoned.";

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
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    /// - `cfg: Config`: A transport configuration used to initialize session.
    pub fn new<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C, cfg: Config) -> Socket {
        let (ct_tx, ct_rx) = channel();
        let ws = Builder::new().with_settings(Settings {
            key_strict: true,
            ..Default::default()
        }).build(SocketHandler(Arc::new(Mutex::new(callback)))).expect("Failed to set up websocket.");
        let broadcaster = ws.broadcaster();
        ws.connect(url).expect("Failed to set up websocket connection.");

        thread::spawn(move || {
            ws.run().expect("Failed to run mio event loop.");
            let _ = ct_tx.send(());
        });

        Socket {
            ct_rx: ct_rx,
            is_paused: false,
            sender: ws.broadcaster()
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
        write!(formatter, "Socket {{ ... }}")
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.sender.shutdown();
        let _ = self.ct_rx.recv();
    }
}

impl Transport for Socket {
    fn close(&mut self, tx: Sender<Result<(), EngineError>>) -> Result<(), EngineError> {
        try!(self.do_send(vec![Packet::with_str(OpCode::Close, "")]));
        self.sender.shutdown();
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.is_paused = true;
    }

    fn send(&mut self, msgs: Vec<Packet>) -> Result<(), EngineError> {
        if !self.is_paused {
            try!(self.do_send(msgs))
        }
        Ok(())
    }

    fn start(&mut self) -> Result<(), EngineError> {
        self.is_paused = false;
    }
}

struct SocketHandler<C>(Arc<Mutex<C>>);

impl<C> Factory for SocketHandler<C>
    where C: FnMut(EngineEvent) + Send + 'static {
    type Handler = Self;

    fn connection_made(&mut self, output: WsSender) -> Self::Handler {
        SocketHandler(self.0.clone())
    }
}

impl<C> Handler for SocketHandler<C>
    where C: FnMut(EngineEvent) + Send + 'static {
    fn on_error(&mut self, err: WsError) {
        let func = self.0.lock().expect(CALLBACK_POISONED);
        func(EngineEvent::Error(&EngineError::WebSocket(err)));
    }

    fn on_message(&mut self, msg: Message) -> WsResult<()> {
        if let Message::Text(str) = msg {
            if let Ok(pck) = Packet::from_str(&str) {
                let func = self.0.lock().expect(CALLBACK_POISONED);
                func(EngineEvent::Message(&pck));
            }
        }
        Ok(())
    }
}