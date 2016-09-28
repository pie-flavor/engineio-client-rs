//! The HTTP long polling transport.
//!
//! HTTP long polling is comparatively slow but very reliable.
//! Engine.io always sets up a connection using long polling and
//! upgrates to web sockets, if possible.

use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Cursor, Error, ErrorKind};
use std::rc::Rc;

use {Config as ConnectionConfig, Packet, Payload};
use futures::{self, BoxFuture, Future, Poll};
use futures::stream::{self, Stream};
use rustc_serialize::json;
use tokio_core::reactor::Handle;
use tokio_request as http;
use transports::Config as TransportConfig;

const HANDSHAKE_BINARY_RECEIVED: &'static str = "Received binary packet when string packet was expected in session initialization.";
const HANDSHAKE_PACKET_MISSING: &'static str = "Expected at least one packet as part of the handshake.";
const ONESHOT_COMPLETE_DROPPED: &'static str = "Complete was dropped before it was completed. This is a bug. Please contact the library authors of engineio-rs.";
const TRANSPORT_PAUSED: &'static str = "Transport is paused. Unpause it before sending packets again.";

/// Represents the receiving half of an HTTP long polling connection.
pub struct Receiver(Rc<Inner>, stream::Receiver<Packet, Error>);

/// Represents the sending half of an HTTP long polling connection.
#[derive(Debug)]
pub struct Sender(Rc<Inner>, bool);

/// Common inner state of both `Sender` and `Receiver`.
#[derive(Clone)]
struct Inner {
    conn_cfg: ConnectionConfig,
    handle: Handle,
    tp_cfg: TransportConfig
}

/// Asynchronously creates a new long polling connection to the given endpoint.
pub fn connect(config: ConnectionConfig, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
    let f = handshake(&config, &handle)
        .map(move |tc| connect_with_config(config, tc, handle));
    Box::new(f) // .boxed() requires Send, which we don't have
}

/// Asynchronously creates a polling connection to the given endpoint using
/// the given transport configuration.
pub fn connect_with_config(conn_cfg: ConnectionConfig,
                           tp_cfg: TransportConfig,
                           handle: Handle)
                           -> (Sender, Receiver) {
    let (tx, rx) = stream::channel();
    handle.spawn(start_polling(conn_cfg.clone(), tp_cfg.clone(), handle.clone(), tx));
    let data = Rc::new(Inner {
        conn_cfg: conn_cfg,
        handle: handle,
        tp_cfg: tp_cfg
    });
    (Sender(data.clone(), false), Receiver(data, rx))
}

impl Receiver {
    /// Gets the underlying transport configuration.
    pub fn transport_config(&self) -> &TransportConfig {
        &self.0.tp_cfg
    }
}

impl Debug for Receiver {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        fmt.debug_tuple("Receiver")
            .field(&self.0)
            .finish()
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.1.poll()
    }
}

impl Sender {
    /// Returns whether the transport currently is paused.
    pub fn is_paused(&self) -> bool {
        self.1
    }

    /// Pauses the transport.
    pub fn pause(&mut self) {
        self.1 = true;
    }

    /// Sends a packet to the server.
    pub fn send(&self, packets: Vec<Packet>) -> BoxFuture<(), Error> {
        if !self.1 {
            send(&self.0.conn_cfg, &self.0.tp_cfg, &self.0.handle, packets)
        } else {
            futures::failed(Error::new(
                ErrorKind::InvalidInput,
                TRANSPORT_PAUSED
            )).boxed()
        }
    }

    /// Gets the underlying transport configuration.
    pub fn transport_config(&self) -> &TransportConfig {
        &self.0.tp_cfg
    }

    /// Unpauses the transport.
    pub fn unpause(&mut self) {
        self.1 = false
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

fn handshake(config: &ConnectionConfig, handle: &Handle) -> BoxFuture<TransportConfig, Error> {
    poll(config, None, handle).and_then(|packets| {
        if packets.len() == 0 {
            return Err(Error::new(ErrorKind::InvalidData, HANDSHAKE_PACKET_MISSING));
        }

        match *packets[0].payload() {
            Payload::String(ref str) => json::decode(str).map_err(|err| Error::new(ErrorKind::InvalidData, err)),
            Payload::Binary(_) => Err(Error::new(ErrorKind::InvalidData, HANDSHAKE_BINARY_RECEIVED))
        }
    }).boxed()
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

fn prepare_request(mut request: Request, conn_cfg: &::Config, tp_cfg: Option<&Config>) -> Request {
    if let Some(cfg) = tp_cfg {
        request = request.param("sid", &cfg.sid)
                         .timeout(cfg.ping_timeout());
    }
    request = request.param("EIO", "3")
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

fn start_polling(conn_cfg: ConnectionConfig,
                 tp_cfg: TransportConfig,
                 handle: Handle,
                 sender: stream::Sender<Packet, Error>)
                 -> Box<Future<Item=(), Error=()>> {
    let fut = poll(&conn_cfg, Some(&tp_cfg), &handle).then(|res| {
        match res {
            Ok(packets) => {
                fn send_packets(mut packets: Vec<Packet>,
                                sender: stream::Sender<Packet, Error>)
                                -> BoxFuture<stream::Sender<Packet, Error>, ()> {
                    // Make the borrow checker happy
                    let maybe_packet = {
                        packets.drain(..1).nth(0)
                    };
                    if let Some(first) = maybe_packet {
                        sender.send(Ok(first))
                              .map_err(|_| ())
                              .and_then(move |s| send_packets(packets, s))
                              .boxed()
                    } else {
                        futures::finished(sender).boxed()
                    }
                }

                send_packets(packets, sender).boxed()
            },
            Err(err) => {
                 sender.send(Err(err))
                       .map_err(|_| ())
                       .then(|_| Err(())) // Stop the polling
                       .boxed()
            }
        }.and_then(move |sender| start_polling(conn_cfg, tp_cfg, handle, sender))
    });

    Box::new(fut)
}