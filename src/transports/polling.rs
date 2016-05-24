//! The polling transport.
//!
//! Since long polling is more stable in hostile environments,
//! engine.io relies on it for the handshake and the first few
//! transmitted packages.
//! It turned out that e.g. lots of company proxies or firewalls
//! block websocket traffic, which is why the upgrade to websocket
//! is done only after it has been verified that websockets can
//! indeed be used.

use super::{Transport, TransportConfig};
use std::cell::RefCell;
use std::io::{BufReader, ErrorKind, Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender, SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};
use ::{EngineEvent, EngineError, HANDLER_LOCK_POISONED};
use client::Callbacks;
use hyper::{Client, Error as HttpError};
use hyper::header::Connection;
use packet::{OpCode, Packet, Payload};
use rand::{Rng, weak_rng, XorShiftRng};
use rustc_serialize::json::decode;
use url::Url;

const EVENT_CHANNEL_DISCONNECTED: &'static str = "Event channel was disconnected. This means the connection has been shut down or an error occured.";

lazy_static! {
    static ref HTTP_CLIENT: Client = {
        let mut c = Client::new();
        c.set_read_timeout(Some(Duration::from_secs(5)));
        c.set_write_timeout(Some(Duration::from_secs(5)));
        c
    };
}

thread_local!(static RNG: RefCell<XorShiftRng> = RefCell::new(weak_rng()));

/// The long polling transport.
pub struct Polling {
    ct_tx: SyncSender<()>,
    ev_tx: Sender<PollEvent>
}

impl Polling {
    /// Creates a new instance of a long polling transport and automatically
    /// connects to the given endpoint.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    pub fn new(url: Url, callbacks: Callbacks) -> Polling {
        Polling::create(url, callbacks, None)
    }

    /// Creates a new instance of a long polling transport from a given
    /// configuration and automatically connects to the endpoint.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    /// - `cfg: TransportConfig`: A transport configuration used to recreate the
    ///   transport after it has been interrupted by network issues.
    pub fn with_cfg(url: Url, callbacks: Callbacks, cfg: TransportConfig) -> Polling {
        Polling::create(url, callbacks, Some(cfg))
    }

    fn create(url: Url, callbacks: Callbacks, cfg: Option<TransportConfig>) -> Polling {
        let (ct_tx, ct_rx) = sync_channel(0);
        let (ev_tx, ev_rx) = channel();

        thread::spawn(move || handle_polling(url, callbacks, cfg, ct_rx, ev_rx));
        Polling {
            ct_tx: ct_tx,
            ev_tx: ev_tx
        }
    }
}

impl Drop for Polling {
    fn drop(&mut self) {
        let _ = self.ct_tx.send(()); // If we fail to send, we're already down.
    }
}

impl Transport for Polling {
    fn close(&mut self, tx: Sender<Result<(), EngineError>>) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Close(tx)).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Pause).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn send(&mut self, msgs: Vec<Packet>) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Send(msgs.to_vec())).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn start(&mut self) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Start).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }
}

#[derive(Clone, Debug)]
enum PollEvent {
    Close(Sender<Result<(), EngineError>>),
    Start,
    Pause,
    Send(Vec<Packet>)
}

fn handle_polling(url: Url, callbacks: Callbacks, cfg: Option<TransportConfig>, ct_rx: Receiver<()>, ev_rx: Receiver<PollEvent>) {
    let cfg = if let Some(c) = cfg {
        c
    } else {
        match init_session(url.clone()) {
            Ok(config) => config,
            Err(err) => {
                for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).iter_mut() {
                    func(EngineEvent::ConnectError(&err));
                }
                return;
            }
        }
    };
    for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).iter_mut() {
        func(EngineEvent::Connect);
    }

    let mut is_paused = false;
    loop {
        let (pack_tx, pack_rx) = channel();
        if !is_paused {
            poll_async(
                url.clone(),
                cfg.ping_timeout(),
                Some(cfg.sid().to_owned()),
                pack_tx
            );
        }

        loop { select! {
            _ = ct_rx.recv() => return,
            recv_res = ev_rx.recv() => {
                match recv_res {
                    Ok(ev) => {
                        match ev {
                            PollEvent::Close(tx) => {
                                let _ = tx.send(send(url.clone(), cfg.sid(), vec![Packet::with_str(OpCode::Close, "")]));
                                return;
                            },
                            PollEvent::Pause => is_paused = true,
                            PollEvent::Send(packets) => {
                                if !is_paused {
                                    send_async(url.clone(), cfg.sid().to_owned(), packets, channel().0);
                                }
                            },
                            PollEvent::Start => is_paused = false
                        }
                    },
                    Err(_) => return
                }
            },
            recv_res = pack_rx.recv() => {
                match recv_res {
                    Ok(Ok(packet)) => {
                        for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).iter_mut() {
                            func(EngineEvent::Message(&packet));
                        }
                        break;
                    },
                    Ok(Err(err)) => {
                        let _ = writeln!(&mut ::std::io::stderr(), "Failed to receive packet: {:?}", &err);
                        for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).iter_mut() {
                            func(EngineEvent::Error(&err));
                        }
                        return;
                    },
                    _ => {}
                }
            }
        } }
    }
}

// ----------------------------------------------------------------------------

fn append_eio_parameters(url: &mut Url, sid: Option<&str>) {
    RNG.with(|rc| {
        let mut query = url.query_pairs_mut();
        query.append_pair("EIO", "3")
             .append_pair("transport", "polling")
             .append_pair("t", &rc.borrow_mut().gen_ascii_chars().take(7).collect::<String>())
             .append_pair("b64", "1");
        if let Some(id) = sid {
            query.append_pair("sid", id);
        }
    });
}

fn init_session(url: Url) -> Result<TransportConfig, EngineError> {
    let p = try!(poll(url, Duration::from_secs(5), None));
    match p.payload {
        Payload::String(str) => decode(&str).map_err(|err| err.into()),
        Payload::Binary(_) => Err(EngineError::invalid_data("Received binary packet when string packet was expected in session initialization."))
    }
}

fn poll(mut url: Url, timeout: Duration, sid: Option<&str>) -> Result<Packet, EngineError> {
    append_eio_parameters(&mut url, sid);
    let pre_poll_time = Instant::now();
    loop {
        match HTTP_CLIENT.get(url.clone()).header(Connection::close()).send() {
            Ok(response) => return Packet::from_reader_payload(&mut BufReader::new(response)),
            Err(HttpError::Io(ref err)) if err.kind() == ErrorKind::TimedOut && pre_poll_time.elapsed() < timeout => {},
            Err(err) => return Err(err.into())
        }
    }
}

fn poll_async(url: Url, timeout: Duration, sid: Option<String>, channel: Sender<Result<Packet, EngineError>>) {
    thread::spawn(move || {
        let send_res = channel.send(poll(url, timeout, match sid {
            Some(ref string) => Some(string),
            None => None
        }));
        if let Err(err) = send_res {
            let _ = writeln!(&mut ::std::io::stderr(), "Failed to transmit polling result: {:?}", err);
        }
    });
}

fn send(mut url: Url, sid: &str, packets: Vec<Packet>) -> Result<(), EngineError> {
    use std::io::Cursor;

    append_eio_parameters(&mut url, Some(sid));
    let mut buf = Cursor::new(Vec::new());
    for packet in packets {
        try!(packet.write_payload_to(&mut buf));
    }
    let buf: &[u8] = &buf.into_inner();

    match HTTP_CLIENT.post(url).header(Connection::close()).body(buf).send() {
        Ok(_) => Ok(()),
        Err(err) => Err(err.into())
    }
}

fn send_async(url: Url, sid: String, packets: Vec<Packet>, channel: Sender<Result<(), EngineError>>) {
    thread::spawn(move || {
        if let Err(err) = channel.send(send(url, &sid, packets)) {
            let _ = writeln!(&mut ::std::io::stderr(), "Failed to transmit packet sending result: {:?}", err);
        }
    });
}