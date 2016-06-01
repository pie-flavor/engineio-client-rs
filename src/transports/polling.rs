//! The polling transport.
//!
//! Since long polling is more stable in hostile environments,
//! engine.io relies on it for the handshake and the first few
//! transmitted packages.
//! It turned out that e.g. lots of company proxies or firewalls
//! block websocket traffic, which is why the upgrade to websocket
//! is done only after it has been verified that websockets can
//! indeed be used.

use super::{append_eio_parameters, Config, Transport};
use std::io::{BufReader, Cursor, ErrorKind, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use ::{EngineEvent, EngineError};
use hyper::{Client, Error as HttpError};
use packet::{OpCode, Packet, Payload};
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

/// The long polling transport.
#[derive(Debug)]
pub struct Polling(Sender<PollEvent>);

impl Polling {
    /// Creates a new instance of a long polling transport and automatically
    /// connects to the given endpoint.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    pub fn new<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C) -> Polling {
        Polling::create(url, callback, None)
    }

    /// Creates a new instance of a long polling transport from a given
    /// configuration and automatically connects to the endpoint.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    /// - `cfg: Config`: A transport configuration used to recreate the
    ///   transport after it has been interrupted by network issues.
    pub fn with_cfg<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C, cfg: Config) -> Polling {
        Polling::create(url, callback, Some(cfg))
    }

    fn create<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C, cfg: Option<Config>) -> Polling {
        let (ev_tx, ev_rx) = channel();
        thread::spawn(move || handle_polling(url, callback, cfg, ev_rx));
        Polling(ev_tx)
    }
}

impl Drop for Polling {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl Transport for Polling {
    fn close(&mut self) -> Result<(), EngineError> {
        let (tx, rx) = channel();
        // Never mind if we fail to transmit the poll event here.
        // In case the channel is disconnected, the background thread has hung up anyway.
        if self.0.send(PollEvent::Close(tx)).is_ok() {
            // Only wait for a response if the request made it through
            if let Ok(res) = rx.recv() {
                return res;
            }
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.0.send(PollEvent::Pause).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn send(&mut self, msgs: Vec<Packet>) -> Result<(), EngineError> {
        self.0.send(PollEvent::Send(msgs)).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }

    fn start(&mut self) -> Result<(), EngineError> {
        self.0.send(PollEvent::Start).map_err(|_| EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
    }
}

#[derive(Clone, Debug)]
enum PollEvent {
    Close(Sender<Result<(), EngineError>>),
    Start,
    Pause,
    Send(Vec<Packet>)
}

fn handle_polling<C: FnMut(EngineEvent) + Send + 'static>(url: Url, mut callback: C, cfg: Option<Config>, ev_rx: Receiver<PollEvent>) {
    let cfg = if let Some(c) = cfg {
        c
    } else {
        match init_session(url.clone()) {
            Ok(config) => config,
            Err(err) => {
                callback(EngineEvent::ConnectError(&err));
                return;
            }
        }
    };
    callback(EngineEvent::Connect);

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
            recv_res = ev_rx.recv() => {
                match recv_res {
                    Ok(PollEvent::Close(tx)) => {
                        // No async here since we're shutting down anyway
                        let _ = tx.send(send(url.clone(), cfg.sid(), vec![Packet::with_str(OpCode::Close, "")]));
                        return;
                    },
                    Ok(PollEvent::Pause) => is_paused = true,
                    Ok(PollEvent::Send(packets)) => {
                        if !is_paused {
                            send_async(url.clone(), cfg.sid().to_owned(), packets, channel().0);
                        }
                    },
                    Ok(PollEvent::Start) => is_paused = false,
                    Err(_) => return
                }
            },
            recv_res = pack_rx.recv() => {
                match recv_res {
                    Ok(Ok(packets)) => {
                        for packet in packets {
                            callback(EngineEvent::Message(&packet));
                        }
                        break;
                    },
                    Ok(Err(err)) => {
                        let _ = writeln!(&mut ::std::io::stderr(), "Failed to receive packet: {:?}", &err);
                        callback(EngineEvent::Error(&err));
                        return;
                    },
                    _ => {}
                }
            }
        } }
    }
}

// ----------------------------------------------------------------------------

fn init_session(url: Url) -> Result<Config, EngineError> {
    let p = try!(poll(url, Duration::from_secs(5), None));
    match *p[0].payload() {
        Payload::String(ref str) => decode(str).map_err(|err| err.into()),
        Payload::Binary(_) => Err(EngineError::invalid_data("Received binary packet when string packet was expected in session initialization."))
    }
}

fn poll(mut url: Url, timeout: Duration, sid: Option<&str>) -> Result<Vec<Packet>, EngineError> {
    append_eio_parameters(&mut url, sid);
    let pre_poll_time = Instant::now();
    loop {
        match HTTP_CLIENT.get(url.clone()).send() {
            Ok(response) => return Packet::from_reader_all(&mut BufReader::new(response)),
            Err(HttpError::Io(ref err)) if err.kind() == ErrorKind::TimedOut && pre_poll_time.elapsed() < timeout => {},
            Err(err) => return Err(err.into())
        }
    }
}

fn poll_async(url: Url, timeout: Duration, sid: Option<String>, channel: Sender<Result<Vec<Packet>, EngineError>>) {
    thread::spawn(move || {
        let poll_res = poll(url, timeout, match sid {
            Some(ref string) => Some(string),
            None => None
        });
        let send_res = channel.send(poll_res);
        if let Err(err) = send_res {
            if cfg!(debug) {
                let _ = writeln!(&mut ::std::io::stderr(), "Failed to transmit polling result: {:?}", err);
            }
        }
    });
}

fn send(mut url: Url, sid: &str, packets: Vec<Packet>) -> Result<(), EngineError> {
    append_eio_parameters(&mut url, Some(sid));

    let capacity = packets.iter().fold(0usize, |val, p| val + p.try_compute_length(false).unwrap_or(0usize));
    let mut buf = Cursor::new(Vec::with_capacity(capacity));
    for packet in packets {
        try!(packet.write_payload_to(&mut buf));
    }
    let buf: &[_] = &buf.into_inner();

    match HTTP_CLIENT.post(url).body(buf).send() {
        Ok(_) => Ok(()),
        Err(err) => Err(err.into())
    }
}

fn send_async(url: Url, sid: String, packets: Vec<Packet>, channel: Sender<Result<(), EngineError>>) {
    thread::spawn(move || {
        if let Err(err) = channel.send(send(url, &sid, packets)) {
            if cfg!(debug) {
                let _ = writeln!(&mut ::std::io::stderr(), "Failed to transmit packet sending result: {:?}", err);
            }
        }
    });
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn connection() {
        use ::{EngineEvent, OpCode, Packet};
        use std::sync::mpsc::channel;
        use std::time::Duration;
        use transports::Transport;
        use url::Url;

        let (tx, rx) = channel();
        let mut p = Polling::new(Url::parse("http://festify.us:5000/engine.io/").unwrap(), move |ev| {
            match ev {
                EngineEvent::Connect => tx.send("connect".to_owned()).unwrap(),
                EngineEvent::ConnectError(_) => tx.send("connect_error".to_owned()).unwrap(),
                EngineEvent::Disconnect => tx.send("disconnect".to_owned()).unwrap(),
                EngineEvent::Error(_) => tx.send("error".to_owned()).unwrap(),
                EngineEvent::Message(msg) => tx.send("message ".to_owned() + &msg.to_string()).unwrap(),
                _ => {}
            }
        });

        assert_eq!("connect", &rx.recv().unwrap());
        assert!(rx.recv().unwrap().starts_with("message"), "Next engine event wasn't a message.");

        p.send(vec![Packet::with_str(OpCode::Message, "Hello Server!")]).expect("Failed to send packet to be sent to background thread.");
        ::std::thread::sleep(Duration::from_millis(5000));
        p.close().unwrap();
    }
}