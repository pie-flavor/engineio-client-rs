//! The module that contains the code merging the HTTP long
//! polling and websocket connection under a single Sender/Receiver
//! pair and API.

use std::cell::RefCell;
use std::io::{Error, ErrorKind};
use std::rc::Rc;
use std::sync::mpsc;

use packet::{OpCode, Packet};
use transports::{CloseInitiator, Data};
use transports::polling as poll;
use transports::websocket as ws;

use futures::{Async, BoxFuture, Future, IntoFuture, Poll};
use futures::stream::Stream;
use tokio_core::reactor::Handle;
use url::Url;

/// Creates a new engine.io connection using the given configuration.
///
/// This function performs the engine.io handshake to create a new
/// session and the connects to it.
pub fn connect(config: Config, handle: Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
    Box::new(
        poll::get_data(&config, &handle)
            .and_then(move |data| Ok(connect_with_data(config, data, handle)))
    )
}

/// Creates a new engine.io connection using the given configuration.
///
/// Since this function also accepts transport configuration, it also allows
/// reopening a broken connection after downtime.
///
/// All information needed for the long polling transport is encoded in
/// the parameters, which is why this function does not return a future.
/// The websocket connection, however, is built up asynchronously in the
/// background and joined into the connection transparently.
pub fn connect_with_data(conn_cfg: Config, tp_cfg: Data, handle: Handle) -> (Sender, Receiver) {
    let (close_tx, close_rx) = mpsc::channel();
    let (poll_tx, poll_rx) = poll::connect_with_data(
        conn_cfg.clone(),
        tp_cfg.clone(),
        handle.clone()
    );
    let poll_tx_2 = poll_tx.clone();

    // We can use RefCells here (and not Mutexes) because the Sender + Receiver
    // pair can never ever leave the event loop thread. A sender that can leave
    // the thread may be desirable in the future, however, it won't be able to use
    // futures the way an event loop thread sender would be able to do since nothing
    // is there to drive them.
    let (ws_tx, ws_rx) = (Rc::new(RefCell::new(None)), Rc::new(RefCell::new(None)));
    let (ws_tx_w, ws_rx_w) = (Rc::downgrade(&ws_tx), Rc::downgrade(&ws_rx));

    let fut = ws::connect(conn_cfg.clone(), tp_cfg.clone(), handle.clone())
        .map_err(|_| ())
        .and_then(move |txrx| {
            // Before we make the websocket connection available to the end
            // user, we notify the server that we've now got a stable websocket
            // connection running and that we do not wish to receive further
            // packets through HTTP long polling.
            //
            // For the sake of implementation simplicity we continue polling for
            // now even though the packet has been sent. This should be changed
            // in the future for better performance and scalability.
            poll_tx_2.send(vec![Packet::empty(OpCode::Upgrade)])
                     .map_err(|_| ())
                     .and_then(move |_| Ok(txrx))
        })
        .and_then(move |(tx, rx)| {
            // Now as we've notified the server that we're ready for websockets,
            // transparently add the websocket sender and receiver to the instances.
            if let Some(cell) = ws_tx_w.upgrade() {
                *cell.borrow_mut() = Some(tx);
            }
            if let Some(cell) = ws_rx_w.upgrade() {
                *cell.borrow_mut() = Some(rx);
            }
            Ok(())
        });
    handle.spawn(fut);

    let tx = Sender {
        close_tx: close_tx,
        poll_tx: poll_tx,
        ws_tx: ws_tx
    };
    let rx = Receiver {
        close_rx: close_rx,
        poll_rx: poll_rx,
        ws_rx: ws_rx
    };

    (tx, rx)
}

/// Contains the configuration for creating a new connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Extra headers to pass during each request.
    pub extra_headers: Vec<(String, String)>,
    /// The engine.io endpoint.
    pub url: Url
}

/// The sending half of an engine.io connection.
#[derive(Debug)]
pub struct Sender {
    close_tx: mpsc::Sender<()>,
    poll_tx: poll::Sender,
    ws_tx: Rc<RefCell<Option<ws::Sender>>>
}

/// The receiving half of an engine.io connection.
#[derive(Debug)]
pub struct Receiver {
    close_rx: mpsc::Receiver<()>,
    poll_rx: poll::Receiver,
    ws_rx: Rc<RefCell<Option<ws::Receiver>>>
}

impl Sender {
    /// Closes the engine.io connection.
    pub fn close(self) -> BoxFuture<(), Error> {
        // Ignore dropped receivers, they don't receive anything anymore anyway
        let _ = self.close_tx.send(());

        if let Ok(Some(ws)) = Rc::try_unwrap(self.ws_tx).map(|cell| cell.into_inner()) {
            ws.close(CloseInitiator::Client)
              .map_err(|ws_err| Error::new(ErrorKind::Other, ws_err))
              .into_future()
              .boxed()
        } else {
            self.poll_tx.close(CloseInitiator::Client)
        }
    }

    /// Sends the given packet(s) to the other endpoint.
    ///
    /// This can be used to send either a single packet or multiple
    /// packets since both implement Into<Vec<Packet>>.
    pub fn send<P: Into<Vec<Packet>>>(&self, packet: P) -> BoxFuture<(), Error> {
        // Attempts to send the given messages through the websocket
        // connection, if available. Otherwise falls back to HTTP long polling.
        let packets = packet.into();
        if let Some(ref ws) = *self.ws_tx.borrow() {
            ws.send(packets)
              .map_err(|ws_err| Error::new(ErrorKind::Other, ws_err))
              .into_future()
              .boxed()
        } else {
            self.poll_tx.send(packets)
        }
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.poll_rx.poll() {
            Ok(Async::Ready(Some(item))) => Ok(Async::Ready(Some(item))),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::NotReady) => {
                if let Some(ref mut ws_rx) = *self.ws_rx.borrow_mut() {
                    match ws_rx.poll() {
                        Ok(res) => Ok(res),
                        Err(ws_err) => Err(Error::new(ErrorKind::Other, ws_err))
                    }
                } else {
                    Ok(Async::NotReady)
                }
            },
            Err(err) => Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use packet::{OpCode, Packet};

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
            .and_then(|(tx, rx)| {
                tx.send(Packet::with_str(OpCode::Message, "Hello from Connection top level Sender!"))
                  .join(rx.take(1).collect())
            })
            .and_then(|(_, msgs)| {
                assert!(msgs.len() >= 1);
                println!("{:?}", msgs);
                Ok(())
            });
        c.run(fut).unwrap();
    }
}