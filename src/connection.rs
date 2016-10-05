//! The module that contains the code merging the HTTP long
//! polling and websocket connection under a single Sender/Receiver
//! pair and API.

use std::cell::RefCell;
use std::io::Error;
use std::rc::Rc;
use std::sync::mpsc;

use packet::Packet;
use transports::Data;
use transports::polling as poll;
use transports::websocket as ws;

use futures::{BoxFuture, Future, Poll};
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
pub fn connect_with_data(conn_cfg: Config, tp_cfg: Data, handle: Handle) -> (Sender, Receiver) {
    let (close_tx, close_rx) = mpsc::channel();
    let (poll_tx, poll_rx) = poll::connect_with_data(
        conn_cfg.clone(),
        tp_cfg.clone(),
        handle.clone()
    );

    // We can use RefCells here (and not Mutexes) because the Sender + Receiver
    // pair can never ever leave the event loop thread. A sender that can leave
    // the thread may be desirable in the future. However, it won't be able to use
    // futures the way an event loop thread sender would be able to do since nothing
    // is there to drive them.
    let (ws_tx, ws_rx) = (Rc::new(RefCell::new(None)), Rc::new(RefCell::new(None)));
    let (ws_tx_w, ws_rx_w) = (Rc::downgrade(&ws_tx), Rc::downgrade(&ws_rx));
    let fut = ws::connect(conn_cfg.clone(), tp_cfg.clone())
        .map_err(|_| ())
        .and_then(move |txrx| {
            if let Some(cell) = ws_tx_w.upgrade() {
                *cell.borrow_mut() = Some(txrx.0);
            }
            if let Some(cell) = ws_rx_w.upgrade() {
                *cell.borrow_mut() = Some(txrx.1);
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
    pub fn close(self) {
        unimplemented!();
    }

    /// Sends the given packet to the other endpoint.
    pub fn send(&self, packet: Packet) -> BoxFuture<(), ()> {
        self.send_all(vec![packet])
    }

    /// Sends all the given packets to the other endpoint.
    pub fn send_all(&self, packets: Vec<Packet>) -> BoxFuture<(), ()> {
        unimplemented!();
    }
}

impl Stream for Receiver {
    type Item = Packet;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        unimplemented!();
    }
}