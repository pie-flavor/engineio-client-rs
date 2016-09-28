//! The HTTP long polling transport.
//!
//! HTTP long polling is comparatively slow but very reliable.
//! Engine.io always sets up a connection using long polling and
//! upgrates to web sockets, if possible.

use std::cell::RefCell;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Cursor, Error, ErrorKind};
use std::mem;
use std::rc::Rc;
use std::vec::IntoIter;

use {Config as ConnectionConfig, Packet, OpCode, Payload};

use futures::{self, Async, BoxFuture, Future, Poll};
use futures::stream::Stream;
use rand::{Rng, weak_rng, XorShiftRng};
use rustc_serialize::json;
use tokio_core::reactor::Handle;
use tokio_request as http;
use transports::Config as TransportConfig;

const HANDSHAKE_BINARY_RECEIVED: &'static str = "Received binary packet when string packet was expected in session initialization.";
const HANDSHAKE_PACKET_MISSING: &'static str = "Expected at least one packet as part of the handshake.";
const TRANSPORT_PAUSED: &'static str = "Transport is paused. Unpause it before sending packets again.";

thread_local!(static RNG: RefCell<XorShiftRng> = RefCell::new(weak_rng()));

/// Represents the receiving half of an HTTP long polling connection.
#[derive(Debug)]
#[must_use = "Receiver doesn't check for packets unless polled."]
pub struct Receiver {
    inner: Rc<Inner>,
    state: State
}

/// Represents the sending half of an HTTP long polling connection.
#[derive(Debug)]
pub struct Sender {
    inner: Rc<Inner>,
    is_paused: bool
}

/// Common inner state of both `Sender` and `Receiver`.
#[derive(Clone)]
struct Inner {
    conn_cfg: ConnectionConfig,
    handle: Handle,
    tp_cfg: TransportConfig
}

/// Inner state of a receiver.
enum State {
    /// The connection is closed.
    Closed,

    /// Placeholder state when no future is running.
    Empty,

    /// We've got packets to yield.
    Ready(IntoIter<Packet>),

    /// We're currently waiting for a response from the server.
    Waiting(BoxFuture<Vec<Packet>, Error>)
}

/// Asynchronously creates a new long polling connection to the given endpoint.
///
/// This method performs a handshake and then connects to the server.
pub fn connect(config: ConnectionConfig, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
    let fut = poll(&config, None, &handle)
        .and_then(|packets| {
            if packets.len() == 0 {
                return Err(Error::new(ErrorKind::InvalidData, HANDSHAKE_PACKET_MISSING));
            }

            match *packets[0].payload() {
                Payload::String(ref str) => json::decode(str).map_err(|err| Error::new(ErrorKind::InvalidData, err)),
                Payload::Binary(_) => Err(Error::new(ErrorKind::InvalidData, HANDSHAKE_BINARY_RECEIVED))
            }
        })
        .map(move |tc| connect_with_config(config, tc, handle));
    Box::new(fut) // .boxed() requires Send, which we don't have
}

/// Creates a polling connection to the given endpoint using the given transport configuration.
///
/// This method does not perform the handshake to obtain the [`Config`](../struct.Config.html).
pub fn connect_with_config(conn_cfg: ConnectionConfig,
                           tp_cfg: TransportConfig,
                           handle: Handle)
                           -> (Sender, Receiver) {
    let data = Rc::new(Inner {
        conn_cfg: conn_cfg,
        handle: handle,
        tp_cfg: tp_cfg
    });
    let tx = Sender {
        inner: data.clone(),
        is_paused: false
    };
    let rx = Receiver {
        inner: data,
        state: State::Empty
    };

    (tx, rx)
}

impl Receiver {
    /// Gets the underlying transport configuration.
    pub fn transport_config(&self) -> &TransportConfig {
        &self.inner.tp_cfg
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match mem::replace(&mut self.state, State::Empty) {
                State::Closed => return Ok(Async::Ready(None)),
                State::Empty => {
                    let fut = poll(&self.inner.conn_cfg, Some(&self.inner.tp_cfg), &self.inner.handle);
                    self.state = State::Waiting(fut);
                },
                State::Ready(mut packets) => {
                    match packets.next() {
                        Some(e) => {
                            if e.opcode() != OpCode::Close {
                                self.state = State::Ready(packets);
                                return Ok(Async::Ready(Some(e)));
                            } else {
                                self.state = State::Closed;
                            }
                        },
                        None => self.state = State::Empty,
                    }
                },
                State::Waiting(mut fut) => {
                    match try!(fut.poll()) {
                        Async::Ready(packets) => self.state = State::Ready(packets.into_iter()),
                        Async::NotReady => {
                            self.state = State::Waiting(fut);
                            return Ok(Async::NotReady);
                        }
                    }
                }
            }
        }
    }
}

impl Sender {
    /// Returns whether the transport currently is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    /// Pauses the transport.
    pub fn pause(&mut self) {
        self.is_paused = true;
    }

    /// Sends a packet to the server.
    pub fn send(&self, packets: Vec<Packet>) -> BoxFuture<(), Error> {
        if !self.is_paused {
            send(&self.inner.conn_cfg, &self.inner.tp_cfg, &self.inner.handle, packets)
        } else {
            futures::failed(Error::new(
                ErrorKind::InvalidInput,
                TRANSPORT_PAUSED
            )).boxed()
        }
    }

    /// Gets the underlying transport configuration.
    pub fn transport_config(&self) -> &TransportConfig {
        &self.inner.tp_cfg
    }

    /// Unpauses the transport.
    pub fn unpause(&mut self) {
        self.is_paused = false
    }
}

impl Debug for Inner {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        fmt.debug_struct("Inner")
           .field("conn_cfg", &self.conn_cfg)
           .field("tp_cfg", &self.tp_cfg)
           .finish()
    }
}

impl Debug for State {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        match *self {
            State::Closed => fmt.debug_tuple("Closed").finish(),
            State::Empty => fmt.debug_tuple("Empty").finish(),
            State::Ready(ref iter) => fmt.debug_tuple("Ready")
                                         .field(&iter)
                                         .finish(),
            State::Waiting(_) => fmt.debug_tuple("Waiting").finish()
        }
    }
}

fn close(conn_cfg: &ConnectionConfig, tp_cfg: &TransportConfig, handle: &Handle) -> BoxFuture<(), Error> {
    send(
        conn_cfg,
        tp_cfg,
        handle,
        vec![Packet::with_string(OpCode::Close, String::default())]
    )
}

fn poll(conn_cfg: &ConnectionConfig,
        tp_cfg: Option<&TransportConfig>,
        handle: &Handle)
        -> BoxFuture<Vec<Packet>, Error> {
    prepare_request(http::get(&conn_cfg.url), conn_cfg, tp_cfg)
        .send(handle.clone())
        .and_then(|resp| resp.ensure_success())
        .and_then(|resp| Packet::from_reader_all(&mut Cursor::new(Vec::<u8>::from(resp))))
        .boxed()
}

fn prepare_request(mut request: http::Request, conn_cfg: &ConnectionConfig, tp_cfg: Option<&TransportConfig>) -> http::Request {
    if let Some(cfg) = tp_cfg {
        request = request.param("sid", &cfg.sid)
                         .timeout(cfg.ping_timeout());
    }
    request.param("EIO", "3")
           .param("transport", "polling")
           .param("t", &RNG.with(|rc| rc.borrow_mut().gen_ascii_chars().take(7).collect::<String>()))
           .param("b64", "1")
           .headers(conn_cfg.extra_headers.clone())
}

fn send(conn_cfg: &ConnectionConfig,
        tp_cfg: &TransportConfig,
        handle: &Handle,
        packets: Vec<Packet>)
        -> BoxFuture<(), Error> {
    let capacity = packets.iter().fold(0usize, |val, p| {
        val + p.compute_payload_length(false)
    });
    let mut buf = Cursor::new(vec![0; capacity]);
    for packet in packets {
        if let Err(err) = packet.write_payload_to(&mut buf) {
            return futures::failed(err).boxed();
        }
    }

    let r = http::post(&conn_cfg.url).body(buf.into_inner());
    prepare_request(r, conn_cfg, Some(tp_cfg))
        .send(handle.clone())
        .and_then(|resp| resp.ensure_success())
        .map(|_| ())
        .boxed()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use packet::*;

    use futures::stream::Stream;
    use tokio_core::reactor::{Core, Handle};
    use url::Url;

    #[test]
    fn connection() {
        println!("Done");
    }
}