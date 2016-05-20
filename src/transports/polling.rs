#![allow(dead_code)]

use super::*;
use std::io::{BufReader, ErrorKind, Read};
use std::ops::DerefMut;
use std::sync::mpsc::{channel, Receiver, Sender, SyncSender, sync_channel};
use std::thread;
use ::{EngineError, HANDLER_LOCK_POISONED};
use connection::{Callbacks, ConnectionEvent};
use hyper::client::Client;
use packet::{Packet, Payload};
use rustc_serialize::json::decode;
use url::Url;

const EVENT_CHANNEL_DISCONNECTED: &'static str = "Event channel was disconnected. Please contact the developers of engineio-rs.";

lazy_static! {
    static ref HTTP_CLIENT: Client = {
        use std::time::Duration;

        let mut c = Client::new();
        c.set_read_timeout(Some(Duration::from_secs(25)));
        c.set_write_timeout(Some(Duration::from_secs(25)));
        c
    };
}

pub struct Polling {
    ct_tx: SyncSender<()>,
    ev_tx: Sender<PollEvent>
}

impl Polling {
    pub fn new(url: Url, callbacks: Callbacks) -> Polling {
        let (ct_tx, ct_rx) = sync_channel(0);
        let (ev_tx, ev_rx) = channel();

        thread::spawn(move || handle_polling(url, callbacks, ct_rx, ev_rx));
        Polling {
            ct_tx: ct_tx,
            ev_tx: ev_tx
        }
    }
}

impl Drop for Polling {
    fn drop(&mut self) {
        self.ct_tx.send(()).expect("Failed to send cancellation signal.");
    }
}

impl Transport for Polling {
    fn close(&mut self) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Close).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Pause).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn send(&mut self, msgs: &[Packet]) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Send(msgs.to_vec())).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn start(&mut self) -> Result<(), EngineError> {
        self.ev_tx.send(PollEvent::Start).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Eq, PartialEq, RustcEncodable, RustcDecodable)]
struct TransportConfig {
    pub pingInterval: u32,
    pub pingTimeout: u32,
    pub sid: String,
    pub upgrades: Vec<String>
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PollEvent {
    Close,
    Start,
    Pause,
    Poll,
    Send(Vec<Packet>)
}

fn handle_polling(url: Url, callbacks: Callbacks, ct_rx: Receiver<()>, ev_rx: Receiver<PollEvent>) {
    let cfg = match init_session(url.clone()) {
        Ok(config) => config,
        Err(err) => {
            for func in callbacks.lock().expect(HANDLER_LOCK_POISONED).deref_mut() {
                func(ConnectionEvent::ConnectError(&err));
            }
            return;
        }
    };

    select! {
        _ = ct_rx.recv() => return,
        ev = ev_rx.recv() => {

        }
    }
}

fn init_session(mut url: Url) -> Result<TransportConfig, EngineError> {
    append_eio_parameters(&mut url, None);
    let p = try!(do_poll(url));
    match p.payload {
        Payload::String(str) => decode(&str).map_err(|err| err.into()),
        Payload::Binary(_) => Err(EngineError::invalid_data("Received binary packet when string packet was expected in session initialization."))
    }
}

fn append_eio_parameters(url: &mut Url, sid: Option<&str>) {
    use rand::{Rng, weak_rng};

    let mut query = url.query_pairs_mut();
    query.append_pair("EIO", "3")
         .append_pair("transport", "polling")
         .append_pair("t", &weak_rng().gen_ascii_chars().take(7).collect::<String>())
         .append_pair("b64", "1");
    if let Some(id) = sid {
        query.append_pair("sid", id);
    }
}

fn do_poll(url: Url) -> Result<Packet, EngineError> {
    use hyper::Error as HttpError;
    use hyper::header::{Connection as ConnHeader, UserAgent};

    loop {
        let res = HTTP_CLIENT.get(url.clone())
                             .header(ConnHeader::close())
                             .header(UserAgent("rust-application".to_owned()))
                             .send();
        match res {
            Ok(response) => {
                let mut buf = BufReader::new(response);
                return Packet::parse_payload(&mut buf);
            },
            Err(HttpError::Io(ref err)) if err.kind() == ErrorKind::TimedOut => {},
            Err(err) => return Err(err.into())
        }
    }
}