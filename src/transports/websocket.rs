//! The websocket transport.
//!
//! Websockets are fast, but not as reliable as HTTP long polling
//! in regards to company firewalls, etc. This is why engine.io sets
//! up connections using HTTP long polling and then switches over to
//! websockets if possible.

use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Cursor, Error, ErrorKind};
use std::mem;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use {Packet, OpCode};
use connection::Config;
use transports::{Data, TRANSPORT_PAUSED};

use futures::{Async, Future, Poll};
use futures::stream::Stream;
use ws::{self, CloseCode, Message};

const CONNECTION_CLOSED_BEFORE_HANDSHAKE: &'static str = "Connection was closed by the server before the handshake could've taken place.";

/// Create a new websocket connection to the given endpoint.
///
/// ## Panics
/// Panics when the thread used to drive the websockets cannot
/// be spawned.
pub fn connect(conn_cfg: &Config, tp_cfg: &Data) -> Connect {
    let mut conn_cfg = conn_cfg.clone();
    let tp_cfg = tp_cfg.clone();
    let (sender_tx, sender_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    thread::Builder::new()
        .name("engine.io websocket thread".to_owned())
        .spawn(move || {
            tp_cfg.apply_to(&mut conn_cfg.url);
            conn_cfg.url.query_pairs_mut()
                        .append_pair("transport", "websocket");

            ws::connect(conn_cfg.url.to_string(), move |sender| {
                let _ = sender_tx.send(sender.clone());
                Handler {
                    tx: event_tx.clone(),
                    ws: sender
                }
            }).expect("Failed to create the websocket.");
        })
        .expect("Failed to start websocket thread.");

    Connect(Some(ConnectState::WaitingForSender(sender_rx, event_rx)))
}

/// The future that sets up the websocket connection.
#[derive(Debug)]
#[must_use = "Futures do nothing unless polled."]
pub struct Connect(Option<ConnectState>);

/// The receiving half of the engine.io websocket connection.
#[derive(Debug)]
#[must_use = "Receiver doesn't check for packets unless polled."]
pub struct Receiver(ReceiverState);

/// The sending half of the engine.io websocket connection.
#[derive(Debug)]
pub struct Sender {
    is_paused: bool,
    sender: ws::Sender
}

/// The internal state of the future that sets up the connection.
enum ConnectState {
    /// The future has sent the engine.io handshake, must wait for the response, though.
    WaitingForResponse(ws::Sender, mpsc::Receiver<Event>),

    /// The future is waiting for the sender to arrive from the websockets library.
    WaitingForSender(mpsc::Receiver<ws::Sender>, mpsc::Receiver<Event>)
}

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
    tx: mpsc::Sender<Event>,
    ws: ws::Sender
}

/// Internal state of a receiver.
enum ReceiverState {
    /// The connection is not open.
    Closed,

    /// The receiver is connected to the server and is receiving messages.
    Receiving(mpsc::Receiver<Event>)
}

impl Future for Connect {
    type Item = (Sender, Receiver);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::ConnectState::*;
        use std::sync::mpsc::TryRecvError;

        loop {
            match self.0.take().expect("Cannot poll Connect twice.") {
                WaitingForResponse(ws_sender, ev_recv) => {
                    match ev_recv.try_recv() {
                        Ok(Event::Close) => {
                            return Err(Error::new(ErrorKind::ConnectionRefused, CONNECTION_CLOSED_BEFORE_HANDSHAKE));
                        },
                        Ok(Event::Error(err)) => {
                            return Err(Error::new(ErrorKind::Other, err));
                        }
                        Ok(Event::Packet(pck)) => {
                            if pck.opcode() == OpCode::Pong && pck.payload().as_str() == Some("probe") {
                                let tx = Sender {
                                    is_paused: false,
                                    sender: ws_sender
                                };
                                let rx = Receiver(ReceiverState::Receiving(ev_recv));

                                return Ok(Async::Ready((tx, rx)));
                            } else {
                                self.0 = Some(WaitingForResponse(ws_sender, ev_recv));
                                return Ok(Async::NotReady);
                            }
                        }
                        Err(err @ TryRecvError::Disconnected) => {
                            return Err(Error::new(ErrorKind::Other, err));
                        },
                        Err(TryRecvError::Empty) => {
                            self.0 = Some(WaitingForResponse(ws_sender, ev_recv));
                            return Ok(Async::NotReady);
                        }
                    }
                },
                WaitingForSender(ws_recv, ev_recv) => {
                    match ws_recv.try_recv() {
                        Ok(ws) => {
                            self.0 = Some(WaitingForResponse(ws, ev_recv))
                        },
                        Err(err @ TryRecvError::Disconnected) => {
                            return Err(Error::new(ErrorKind::Other, err));
                        },
                        Err(TryRecvError::Empty) => {
                            self.0 = Some(WaitingForSender(ws_recv, ev_recv));
                            return Ok(Async::NotReady);
                        }
                    }
                }
            }
        }
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = ws::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match mem::replace(&mut self.0, ReceiverState::Closed) {
                ReceiverState::Closed => {
                    return Ok(Async::Ready(None));
                },
                ReceiverState::Receiving(rx) => {
                    match rx.try_recv() {
                        Ok(Event::Close) | Err(TryRecvError::Disconnected) => self.0 = ReceiverState::Closed,
                        Ok(Event::Error(err)) => {
                            self.0 = ReceiverState::Closed;
                            return Err(err);
                        },
                        Ok(Event::Packet(pck)) => {
                            self.0 = ReceiverState::Receiving(rx);
                            return Ok(Async::Ready(Some(pck)));
                        },
                        Err(TryRecvError::Empty) => {
                            self.0 = ReceiverState::Receiving(rx);
                            return Ok(Async::NotReady);
                        }
                    }
                }
            }
        }
    }
}

impl Sender {
    /// Closes the connection to the server.
    pub fn close(self) {
        let _ = self.sender.send(Packet::empty(OpCode::Close));
        let _ = self.sender.close(CloseCode::Normal);
    }

    /// Returns whether the transport currently is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    /// Pauses the transport.
    pub fn pause(&mut self) {
        self.is_paused = true;
    }

    /// Sends packets to the server.
    pub fn send(&self, packets: Vec<Packet>) -> Result<(), ws::Error> {
        if !self.is_paused {
            for packet in packets {
                try!(self.sender.send(packet));
            }
            Ok(())
        } else {
            Err(Error::new(ErrorKind::InvalidInput, TRANSPORT_PAUSED).into())
        }
    }

    /// Unpauses the transport.
    pub fn unpause(&mut self) {
        self.is_paused = false;
    }
}

impl ws::Handler for Handler {
    fn on_close(&mut self, _: CloseCode, _: &str) {
        let _ = self.tx.send(Event::Close);
    }

    fn on_error(&mut self, err: ws::Error) {
        let _ = self.tx.send(Event::Error(err));
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

impl Debug for ConnectState {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        match *self {
            ConnectState::WaitingForResponse(_, _) => fmt.debug_tuple("WaitingForResponse"),
            ConnectState::WaitingForSender(_, _) => fmt.debug_tuple("WaitingForSender")
        }.finish()
    }
}

impl Debug for ReceiverState {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        match *self {
            ReceiverState::Closed => fmt.debug_tuple("Closed"),
            ReceiverState::Receiving(_) => fmt.debug_tuple("Receiving")
        }.finish()
    }
}

impl From<Packet> for ws::Message {
    fn from(p: Packet) -> Self {
        ws::Message::Text(p.to_string())
    }
}