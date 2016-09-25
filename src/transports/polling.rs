//! The HTTP long polling transport.
//!
//! HTTP long polling is comparatively slow but very reliable.
//! Engine.io always sets up a connection using long polling and
//! upgrates to web sockets, if possible.

use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Cursor, Error as IoError, ErrorKind};
use std::sync::Arc;

use {Config as ConnectionConfig, EngineError, Packet, Payload};
use futures::{self, BoxFuture, Future, Poll};
use futures::stream::{self, Receiver as StreamReceiver, Stream};
use rustc_serialize::json::decode;
use tokio_core::reactor::Handle;
use tokio_request as http;
use transports::{Config as TransportConfig, prepare_request};

const HANDSHAKE_BINARY_RECEIVED: &'static str = "Received binary packet when string packet was expected in session initialization.";
const HANDSHAKE_PACKET_MISSING: &'static str = "Expected at least one packet as part of the handshake.";
const ONESHOT_COMPLETE_DROPPED: &'static str = "Complete was dropped before it was completed. This is a bug. Please contact the library authors of engineio-rs.";

/// Represents the receiving half of an HTTP long polling connection.
pub struct Receiver(Arc<Inner>, StreamReceiver<Packet, EngineError>);

/// Represents the sending half of an HTTP long polling connection.
#[derive(Debug)]
pub struct Sender(Arc<Inner>, bool);

/// Common inner state of both `Sender` and `Receiver`.
#[derive(Clone)]
struct Inner {
    conn_cfg: ConnectionConfig,
    handle: Handle,
    tp_cfg: TransportConfig
}

/// Asynchronously creates a new long polling connection to the given endpoint.
pub fn connect(config: ConnectionConfig, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=EngineError>> {
    let f = handshake(&config, &handle)
        .map(move |tc| connect_with_config(config, tc, handle));
    Box::new(f) // .boxed() requires Send, which we don't have
}

/// Asynchronously creates a polling connection to the given endpoint using
/// the given transport configuration.
pub fn connect_with_config(conn_cfg: ConnectionConfig, tp_cfg: TransportConfig, handle: Handle) -> (Sender, Receiver) {
    let (tx, rx) = stream::channel();
    start_polling(conn_cfg.clone(), tp_cfg.clone(), handle.clone(), tx);
    let data = Arc::new(Inner {
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
    type Error = EngineError;

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
    pub fn send(&self, packets: Vec<Packet>) -> BoxFuture<(), EngineError> {
        if !self.1 {
            send(&self.0.conn_cfg, &self.0.tp_cfg, &self.0.handle, packets)
        } else {
            futures::failed(EngineError::invalid_state("Transport is paused. Unpause it before sending packets again.")).boxed()
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

fn handshake(config: &ConnectionConfig, handle: &Handle) -> BoxFuture<TransportConfig, EngineError> {
    poll(config, None, handle).and_then(|packets| {
        if packets.len() == 0 {
            return Err(EngineError::Io(IoError::new(ErrorKind::InvalidData, HANDSHAKE_PACKET_MISSING)));
        }

        match *packets[0].payload() {
            Payload::String(ref str) => decode(str).map_err(|err| err.into()),
            Payload::Binary(_) => Err(EngineError::Io(IoError::new(ErrorKind::InvalidData, HANDSHAKE_BINARY_RECEIVED)))
        }
    }).boxed()
}

fn poll(conn_cfg: &ConnectionConfig, tp_cfg: Option<&TransportConfig>, handle: &Handle) -> BoxFuture<Vec<Packet>, EngineError> {
    prepare_request(http::get(&conn_cfg.url), conn_cfg, tp_cfg)
        .send(handle.clone())
        .map_err(|err| err.into())
        .and_then(|resp| Packet::from_reader_all(&mut Cursor::new(Vec::<u8>::from(resp))))
        .boxed()
}

fn send(conn_cfg: &ConnectionConfig, tp_cfg: &TransportConfig, handle: &Handle, packets: Vec<Packet>) -> BoxFuture<(), EngineError> {
    let capacity = packets.iter().fold(0usize, |val, p| val + p.try_compute_length(false).unwrap_or(0usize));
    let mut buf = Cursor::new(vec![0; capacity]);
    for packet in packets {
        if let Err(err) = packet.write_payload_to(&mut buf) {
            return futures::failed(err.into()).boxed();
        }
    }

    prepare_request(http::post(&conn_cfg.url).body(buf.into_inner()), conn_cfg, Some(tp_cfg))
        .send(handle.clone())
        .map_err(|err| err.into())
        .and_then(|resp| {
            if resp.is_success() {
                Ok(())
            } else {
                let msg: &str = &format!("Received erroneous HTTP response code {}.", resp.status_code());
                Err(EngineError::invalid_state(msg))
            }
        })
        .boxed()
}

fn start_polling(conn_cfg: ConnectionConfig, tp_cfg: TransportConfig, handle: Handle, sender: stream::Sender<Packet, EngineError>) {
    unimplemented!();
}