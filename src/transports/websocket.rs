//! The websocket transport.
//!
//! Websockets are fast, but not as reliable as HTTP long polling
//! in regards to company firewalls, etc. This is why engine.io sets
//! up connections using HTTP long polling and then switches over to
//! websockets if possible.

use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Cursor, Error, ErrorKind};
use std::sync::mpsc::{TryRecvError};
use std::thread;

use packet::{Packet, OpCode};
use connection::Config;
use transports::Data;

use futures::{self, Async, Future, Poll};
use futures::stream::Stream;
use tokio_core::channel as core;
use tokio_core::reactor::Handle;
use ws::{self, CloseCode, Message};

const CONNECTION_CLOSED_BEFORE_HANDSHAKE: &'static str = "Connection was closed by the server before the handshake could've taken place.";
const HANDSHAKE_PAYLOAD: &'static str = "probe";

/// Create a new websocket connection to the given endpoint.
///
/// ## Panics
/// Panics when the thread used to drive the websockets cannot
/// be spawned (very rare).
pub fn connect(conn_cfg: Config, tp_cfg: Data, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
    fn _connect(mut conn_cfg: Config, tp_cfg: Data, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
        let (sender_tx, sender_rx) = core::channel(&handle).unwrap();
        let (event_tx, event_rx) = core::channel(&handle).unwrap();

        thread::Builder::new()
            .name("Engine.io websocket thread".to_owned())
            .spawn(move || {
                tp_cfg.apply_to(&mut conn_cfg.url);
                conn_cfg.url.query_pairs_mut()
                            .append_pair("transport", "websocket");

                ws::connect(conn_cfg.url.to_string(), move |sender| {
                    let _ = sender_tx.send(sender.clone());
                    Handler {
                        tx: event_tx.clone(), // FnMut closure
                        ws: sender
                    }
                }).expect("Failed to create websocket.");
            })
            .expect("Failed to start websocket thread.");

        WaitForSender(Some((sender_rx, event_rx)))
            .map_err(|err| Error::new(ErrorKind::Other, err))
            .and_then(|data| {
                try!(data.0.send(Packet::with_str(OpCode::Ping, HANDSHAKE_PAYLOAD))
                           .map_err(|ws_err| Error::new(ErrorKind::Other, ws_err)));
                Ok(data)
            })
            .and_then(|data| WaitForHandshake(Some(data)))
            .boxed()
    }

    Box::new(futures::lazy(move || _connect(conn_cfg, tp_cfg, handle)))
}

/// The sending half of the engine.io websocket connection.
#[derive(Clone, Debug)]
pub struct Sender(ws::Sender);

/// The receiving half of the engine.io websocket connection.
#[must_use = "Receiver doesn't check for packets unless polled."]
pub struct Receiver(Option<core::Receiver<Event>>);

/// Information about an incoming websocket event.
#[derive(Debug)]
enum Event {
    /// The websocket has been closed.
    Close,

    /// An error occured in the websocket and thus it will be closed.
    Error(ws::Error),

    /// A websocket message was received.
    Packet(Packet)
}

/// A struct for implementing the websocket handler.
struct Handler {
    tx: core::Sender<Event>,
    ws: ws::Sender
}

/// The future that waits for the sender to be created.
#[must_use = "Futures do nothing unless polled."]
struct WaitForSender(Option<(core::Receiver<ws::Sender>, core::Receiver<Event>)>);

/// The future that sets up the websocket connection.
#[must_use = "Futures do nothing unless polled."]
struct WaitForHandshake(Option<(ws::Sender, core::Receiver<Event>)>);

impl Debug for Receiver {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        fmt.debug_tuple("Receiver").finish()
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = ws::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let mut rx = self.0.take().expect("Cannot poll Receiver twice.");

            match rx.poll() {
                Ok(Async::Ready(Some(event))) => {
                    match event {
                        Event::Close => return Ok(Async::Ready(None)),
                        Event::Error(err) => return Err(err),
                        Event::Packet(pck) => {
                            self.0 = Some(rx);
                            return Ok(Async::Ready(Some(pck)));
                        }
                    }
                },
                Ok(Async::Ready(None)) => {
                    return Ok(Async::Ready(None));
                },
                Ok(Async::NotReady) => {
                    self.0 = Some(rx);
                    return Ok(Async::NotReady);
                },
                Err(err) => {
                    return Err(err.into());
                }
            }
        }
    }
}

impl Sender {
    /// Closes the connection to the server.
    pub fn close(self) -> Result<(), ws::Error> {
        try!(self.0.send(Packet::empty(OpCode::Close)));
        try!(self.0.close(CloseCode::Normal));
        Ok(())
    }

    /// Sends packets to the server.
    pub fn send(&self, packets: Vec<Packet>) -> Result<(), ws::Error> {
        for packet in packets {
            try!(self.0.send(packet));
        }
        Ok(())
    }
}

impl ws::Handler for Handler {
    fn on_close(&mut self, _: CloseCode, _: &str) {
        let _ = self.tx.send(Event::Close);
    }

    fn on_error(&mut self, err: ws::Error) {
        let _ = self.tx.send(Event::Error(err));
        let _ = self.ws.close(CloseCode::Error);
    }

    fn on_message(&mut self, msg: Message) -> Result<(), ws::Error> {
        Packet::from_reader(&mut Cursor::new(msg.into_data()))
            .map_err(|err| err.into())
            .and_then(|pck| {
                self.tx.send(Event::Packet(pck))
                       .map_err(|err| Box::new(err).into())
            })
    }
}

impl Future for WaitForSender {
    type Item = (ws::Sender, core::Receiver<Event>);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (mut ws_rx, ev_rx) = self.0.take().expect("Cannot poll WaitForSender twice.");

        match ws_rx.poll() {
            Ok(Async::Ready(Some(sender))) => Ok(Async::Ready((sender, ev_rx))),
            Ok(Async::Ready(None)) => Err(Error::new(ErrorKind::Other, TryRecvError::Disconnected)),
            Ok(Async::NotReady) => {
                self.0 = Some((ws_rx, ev_rx));
                Ok(Async::NotReady)
            },
            Err(err) => Err(err)
        }
    }
}

impl Future for WaitForHandshake {
    type Item = (Sender, Receiver);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (sender, mut ev_rx) = self.0.take().expect("Cannot poll WaitForHandshake twice.");

        match ev_rx.poll() {
            Ok(Async::Ready(Some(event))) => {
                match event {
                    Event::Close => Err(Error::new(ErrorKind::ConnectionRefused, CONNECTION_CLOSED_BEFORE_HANDSHAKE)),
                    Event::Error(err) => Err(Error::new(ErrorKind::Other, err)),
                    Event::Packet(ref pck) => {
                        if pck.opcode() == OpCode::Pong && pck.payload().as_str() == Some(HANDSHAKE_PAYLOAD) {
                            let tx = Sender(sender);
                            let rx = Receiver(Some(ev_rx));
                            Ok(Async::Ready((tx, rx)))
                        } else {
                            Err(Error::new(ErrorKind::InvalidData, "Received incorrect handshake response."))
                        }
                    }
                }
            },
            Ok(Async::Ready(None)) => Err(Error::new(ErrorKind::Other, TryRecvError::Disconnected)),
            Ok(Async::NotReady) => {
                self.0 = Some((sender, ev_rx));
                Ok(Async::NotReady)
            },
            Err(err) => Err(err)
        }
    }
}

impl From<Packet> for ws::Message {
    fn from(p: Packet) -> Self {
        ws::Message::Text(p.to_string())
    }
}