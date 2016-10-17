//! The HTTP long polling transport.
//!
//! HTTP long polling is comparatively slow but very reliable.
//! Engine.io always sets up a connection using long polling and
//! upgrates to web sockets, if possible.

use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::io::{Cursor, Error, ErrorKind};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::vec::IntoIter;

use packet::{Packet, OpCode};
use connection::Config;
use transports::{CloseInitiator, Data, gen_random_string};

use futures::{self, Async, BoxFuture, Future, Poll};
use futures::stream::Stream;
use tokio_core::reactor::Handle;
use tokio_request as http;
use url::Url;

const HANDSHAKE_PACKET_MISSING: &'static str = "Expected at least one valid packet as part of the handshake.";
const HTTP_INVALID_STATUS_CODE: &'static str = "Received an invalid HTTP status code.";

/// Asynchronously creates a new long polling connection to the given endpoint.
///
/// This method performs a handshake and then connects to the server.
pub fn connect(config: Config, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
    Box::new(
        get_data(&config, &handle)
            .map(move |tc| connect_with_data(config, tc, handle))
    )
}

/// Creates a polling connection to the given endpoint using the given transport configuration.
///
/// This method does not perform the handshake to obtain the [`Config`](../struct.Config.html).
pub fn connect_with_data(conn_cfg: Config, data: Data, handle: Handle) -> (Sender, Receiver) {
    let (close_tx, close_rx) = mpsc::channel();
    let inner = Rc::new(Inner {
        conn_cfg: conn_cfg,
        data: data,
        handle: handle
    });
    let tx = Sender {
        close_tx: close_tx,
        inner: inner.clone()
    };
    let rx = Receiver {
        close_rx: close_rx,
        inner: inner,
        state: Some(State::Empty)
    };

    (tx, rx)
}

/// Obtains the configuration data used to set up an engine.io connection.
pub fn get_data(config: &Config, handle: &Handle) -> BoxFuture<Data, Error> {
    poll(config, None, handle)
        .and_then(|packets| {
            // Result implements an iterator that either returns the element
            // in the Ok-case or nothing in the Err-case. We use this to select
            // only the packets where the deserialization has been successful.
            packets.into_iter()
                   .flat_map(|pck| pck.payload().from_json().into_iter())
                   .nth(0)
                   .ok_or(Error::new(ErrorKind::InvalidData, HANDSHAKE_PACKET_MISSING))
        })
        .boxed()
}

/// Represents the sending half of an HTTP long polling connection.
#[derive(Clone, Debug)]
pub struct Sender {
    close_tx: mpsc::Sender<()>,
    inner: Rc<Inner>
}

/// Represents the receiving half of an HTTP long polling connection.
#[derive(Debug)]
#[must_use = "Receiver doesn't check for packets unless polled."]
pub struct Receiver {
    close_rx: mpsc::Receiver<()>,
    inner: Rc<Inner>,
    state: Option<State>
}

/// Common inner state of senders and receivers.
#[derive(Clone)]
struct Inner {
    conn_cfg: Config,
    data: Data,
    handle: Handle
}

/// Inner state of a receiver.
enum State {
    /// Placeholder state when no future is running.
    Empty,

    /// We've got packets to yield.
    Ready(IntoIter<Packet>),

    /// We're currently waiting for a response from the server.
    Waiting(Instant, BoxFuture<Vec<Packet>, Error>)
}

impl Receiver {
    /// Gets the underlying transport configuration.
    pub fn transport_data(&self) -> &Data {
        &self.inner.data
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Ok(_) = self.close_rx.try_recv() {
            self.state = None;
            return Ok(Async::Ready(None));
        }

        loop {
            match self.state.take().expect("Cannot poll Receiver twice.") {
                State::Empty => {
                    let fut = poll(
                        &self.inner.conn_cfg,
                        Some(&self.inner.data),
                        &self.inner.handle
                    );

                    // Record the time when we start polling so that we can determine
                    // whether a possible timeout has just occured because there was
                    // no data available or because the connection had issues.
                    self.state = Some(State::Waiting(Instant::now(), fut));
                },
                State::Ready(mut packets) => {
                    match packets.next() {
                        Some(e) => {
                            self.state = Some(State::Ready(packets));
                            return Ok(Async::Ready(Some(e)));
                        },
                        None => self.state = Some(State::Empty),
                    }
                },
                State::Waiting(poll_start, mut fut) => {
                    match fut.poll() {
                        Ok(Async::Ready(packets)) => {
                            self.state = Some(State::Ready(packets.into_iter()));
                        },
                        Ok(Async::NotReady) => {
                            self.state = Some(State::Waiting(poll_start, fut));
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            // If we're dealing with a timeout error and it occured because there
                            // was no data available to us (ping timeout hasn't elapsed yet), just
                            // continue polling.
                            if err.kind() == ErrorKind::TimedOut &&
                               poll_start.elapsed() <= self.inner.data.ping_timeout() {
                                self.state = Some(State::Empty);
                            } else {
                                return Err(err);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Sender {
    /// Closes the connection to the server.
    pub fn close(self, initiator: CloseInitiator) -> BoxFuture<(), Error> {
        let _ = self.close_tx.send(());
        if initiator == CloseInitiator::Client {
            let pck = Packet::empty(OpCode::Close);
            self.send(vec![pck])
        } else {
            futures::finished(()).boxed()
        }
    }

    /// Sends packets to the server.
    pub fn send(&self, packets: Vec<Packet>) -> BoxFuture<(), Error> {
        let capacity = packets.iter().fold(0usize, |val, p| {
            val + p.compute_payload_length(false)
        });
        let mut buf = Cursor::new(vec![0; capacity]);
        for packet in packets {
            if let Err(err) = packet.write_payload_to(&mut buf) {
                return futures::failed(err).boxed();
            }
        }

        prepare_request(http::post, &self.inner.conn_cfg, Some(&self.inner.data))
            .body(buf.into_inner())
            .send(self.inner.handle.clone())
            .and_then(|resp| {
                resp.ensure_success()
                    .map_err(|res| Error::new(ErrorKind::InvalidData, format!("{} {:?}", HTTP_INVALID_STATUS_CODE, res).as_ref()))
            })
            .map(|_| ())
            .boxed()
    }

    /// Gets the underlying transport configuration.
    pub fn transport_data(&self) -> &Data {
        &self.inner.data
    }
}

impl Debug for Inner {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        fmt.debug_struct("Inner")
           .field("conn_cfg", &self.conn_cfg)
           .field("data", &self.data)
           .finish()
    }
}

impl Debug for State {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        match *self {
            State::Empty => fmt.debug_tuple("Empty").finish(),
            State::Ready(ref iter) => fmt.debug_tuple("Ready")
                                         .field(&iter)
                                         .finish(),
            State::Waiting(time, _) => fmt.debug_tuple("Waiting")
                                          .field(&time)
                                          .finish()
        }
    }
}

fn poll(conn_cfg: &Config,
        data: Option<&Data>,
        handle: &Handle)
        -> BoxFuture<Vec<Packet>, Error> {
    prepare_request(http::get, conn_cfg, data)
        .send(handle.clone())
        .and_then(|resp| {
            resp.ensure_success()
                .map_err(|res| Error::new(ErrorKind::InvalidData, format!("{} {:?}", HTTP_INVALID_STATUS_CODE, res).as_ref()))
        })
        .and_then(|resp| Packet::from_reader_all(&mut Cursor::new(resp)))
        .boxed()
}

fn prepare_request<R: FnOnce(&Url) -> http::Request>(request_fn: R, conn_cfg: &Config, data: Option<&Data>) -> http::Request {
    let mut url = conn_cfg.url.clone();
    if let Some(cfg) = data {
        cfg.apply_to(&mut url);
    }
    request_fn(&url)
        .param("EIO", "3")
        .param("transport", "polling")
        .param("t", &gen_random_string())
        .param("b64", "1")
        .headers(conn_cfg.extra_headers.clone())
        .timeout(if let Some(cfg) = data {
            cfg.ping_interval()
        } else {
            Duration::from_secs(10)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use connection::Config;
    use packet::*;

    use futures::Future;
    use futures::stream::Stream;
    use tokio_core::reactor::Core;
    use url::Url;

    fn get_config() -> Config {
        const ENGINEIO_URL: &'static str = "http://festify.us:5002/engine.io/";

        Config {
            extra_headers: vec![("X-Requested-By".to_owned(), "engineio-rs".to_owned())],
            url: Url::parse(ENGINEIO_URL).unwrap()
        }
    }

    #[test]
    fn connection() {
        let mut c = Core::new().unwrap();
        let fut = connect(get_config(), c.handle())
            .then(|res| {
                assert!(res.is_ok(), "Failed to connect to server: {:?}", res);
                res
            })
            .and_then(|(tx, rx)| {
                tx.send(vec![Packet::with_str(OpCode::Message, "Hello!")])
                  .join(rx.take(1).collect())
            })
            .and_then(|(_, packets)| {
                assert!(packets.len() == 1);
                println!("Received the following packet: {:?}", packets[0]);
                Ok(())
            });
        c.run(fut).unwrap();
    }

    #[test]
    fn transport_config() {
        let mut c = Core::new().unwrap();
        let fut = get_data(&get_config(), &c.handle())
            .then(|res| {
                assert!(res.is_ok(), "Failed to get transport config: {:?}", res);
                res
            })
            .and_then(|tp_cfg| {
                assert!(!tp_cfg.sid().is_empty());
                Ok(())
            });
        c.run(fut).unwrap();
    }
}