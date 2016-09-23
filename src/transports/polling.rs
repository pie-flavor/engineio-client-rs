use std::io::{Cursor, Error as IoError, ErrorKind};

use {Config, EngineError, Packet, Payload};
use futures::Future;
use rustc_serialize::json::decode;
use tokio_core::reactor::Handle;
use tokio_request::{get, Request, Response};
use transports::{Config as TransportConfig, prepare_request};

pub fn handshake(config: &Config, handle: &Handle) -> impl Future<Item=TransportConfig, Error=EngineError> {
    poll(config, handle, None).and_then(|packets| {
        if packets.len() == 0 {
            return Err(EngineError::Io(IoError::new(ErrorKind::InvalidData, "Expected at least one packet as part of the handshake.")));
        }

        match *packets[0].payload() {
            Payload::String(ref str) => decode(str).map_err(|err| err.into()),
            Payload::Binary(_) => Err(EngineError::Io(IoError::new(ErrorKind::InvalidData, "Received binary packet when string packet was expected in session initialization.")))
        }
    })
}

fn poll(config: &Config, handle: &Handle, sid: Option<&str>) -> impl Future<Item=Vec<Packet>, Error=EngineError> {
    prepare_request(get(&config.url), config, sid)
        .send(handle.clone())
        .map_err(|err| err.into())
        .and_then(|resp| Packet::from_reader_all(&mut Cursor::new(Vec::<u8>::from(resp))))
}