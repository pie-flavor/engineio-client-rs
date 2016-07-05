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
use std::io::{BufReader, Cursor, Error as IoError, ErrorKind, Write};
use std::sync::mpsc::{channel, Receiver, Sender, SendError};
use std::thread;
use std::time::{Duration, Instant};
use ::{EngineEvent, EngineError};
use eventual::{Async, AsyncError, Complete, Future};
use hyper::{Client, Error as HttpError};
use packet::{OpCode, Packet, Payload};
use rustc_serialize::json::decode;
use threadpool::ThreadPool;
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

pub fn connect_async(url: Url) -> Future<Config, EngineError> {
    let tp = ThreadPool::new(1);
    poll_async(&tp, url, Duration::from_secs(5), None).and_then(|packets| {
        match *packets[0].payload() {
            Payload::String(ref str) => decode(str).map_err(|err| err.into()),
            Payload::Binary(_) => Err(EngineError::Io(IoError::new(ErrorKind::InvalidData, "Received binary packet when string packet was expected in session initialization.")))
        }
    })
}

/// The long polling transport.
#[derive(Debug)]
pub struct Polling(Sender<PollEvent>, Config);

impl Polling {
    /// Creates a new instance of a long polling transport and automatically
    /// connects to the given endpoint.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    pub fn new<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C) -> Future<Polling, EngineError> {
        connect_async(url.clone()).map(move |cfg| Polling::create(url, callback, cfg, false))
    }

    /// Creates a new instance of a long polling transport from a given
    /// configuration (i.e. a reconnection). This does not fire the
    /// `connect`-callbacks.
    ///
    /// ## Parameters
    /// - `url: Url`: The _full_ URL (i.e. including the `/engine.io/`-path)
    ///   of the server to connect to.
    /// - `callbacks: Callbacks`: Callbacks to call when asynchronous events are ready.
    /// - `cfg: Config`: A transport configuration used to recreate the
    ///   transport after it has been interrupted by network issues.
    pub fn with_cfg<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C, cfg: Config) -> Polling {
        Polling::create(url, callback, cfg, true)
    }

    fn create<C: FnMut(EngineEvent) + Send + 'static>(url: Url, callback: C, cfg: Config, previously_connected: bool) -> Polling {
        let (ev_tx, ev_rx) = channel();
        let cfg2 = cfg.clone();
        thread::spawn(move || handle_polling(url, callback, cfg2, ev_rx, previously_connected));
        Polling(ev_tx, cfg)
    }

    /// Gets the configuration associated with the transport.
    pub fn cfg(&self) -> &Config {
        &self.1
    }
}

impl Drop for Polling {
    fn drop(&mut self) {
        let _ = self.close().await();
    }
}

impl Transport for Polling {
    fn close(&mut self) -> Future<(), EngineError> {
        let (tx, f) = Future::pair();
        if let Err(SendError(PollEvent::Close(tx))) = self.0.send(PollEvent::Close(tx)){
            // Never mind if we fail to transmit the poll event here.
            // In case the channel is disconnected, the background thread
            // has hung up anyway and we're not connected anymore.
            tx.complete(())
        }
        f
    }

    fn pause(&mut self) -> Future<(), EngineError> {
        let (tx, f) = Future::pair();
        if let Err(SendError(PollEvent::Pause(tx))) = self.0.send(PollEvent::Pause(tx)) {
            tx.fail(EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
        }
        f
    }

    fn send(&mut self, msgs: Vec<Packet>) -> Future<(), EngineError> {
        let (tx, f) = Future::pair();
        if let Err(SendError(PollEvent::Send(_, tx))) = self.0.send(PollEvent::Send(msgs, tx)) {
            tx.fail(EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
        }
        f
    }

    fn start(&mut self) -> Future<(), EngineError> {
        let (tx, f) = Future::pair();
        if let Err(SendError(PollEvent::Start(tx))) = self.0.send(PollEvent::Start(tx)) {
            tx.fail(EngineError::invalid_state(EVENT_CHANNEL_DISCONNECTED))
        }
        f
    }
}

#[derive(Debug)]
enum PollEvent {
    Close(Complete<(), EngineError>),
    Start(Complete<(), EngineError>),
    Pause(Complete<(), EngineError>),
    Send(Vec<Packet>, Complete<(), EngineError>)
}

fn handle_polling<C>(url: Url, mut callback: C, cfg: Config, ev_rx: Receiver<PollEvent>, previously_connected: bool)
    where C: FnMut(EngineEvent) + Send + 'static {
    if !previously_connected {
        callback(EngineEvent::Connect(&cfg));
    }

    let mut is_paused = false;
    let mut packet_buffer = Vec::new();
    let thread_pool = ThreadPool::new_with_name("Engine.io worker thread".to_owned(), 4);
    loop {
        let (pack_tx, pack_rx) = channel();
        if !is_paused {
            poll_async(
                &thread_pool,
                url.clone(),
                cfg.ping_timeout(),
                Some(cfg.sid().to_owned())
            ).receive(move |res| {
                let _ = pack_tx.send(res);
            });
        }

        loop { select! {
            recv_res = ev_rx.recv() => {
                match recv_res {
                    Ok(PollEvent::Close(tx)) => {
                        // No async here since we're shutting down anyway
                        let _ = send(url.clone(), cfg.sid(), vec![Packet::with_str(OpCode::Close, "")]);
                        callback(EngineEvent::Disconnect);
                        tx.complete(());
                        return;
                    },
                    Ok(PollEvent::Pause(tx)) => {
                        is_paused = true;
                        tx.complete(());
                    },
                    Ok(PollEvent::Send(packets, tx)) => {
                        packet_buffer.push((tx, packets));

                        if !is_paused {
                            for (tx, packets) in packet_buffer.drain(..) {
                                send_async(&thread_pool, url.clone(), cfg.sid().to_owned(), packets).receive(|res| {
                                    match res {
                                        Ok(_) => tx.complete(()),
                                        Err(AsyncError::Failed(err)) => tx.fail(err),
                                        Err(AsyncError::Aborted) => tx.abort()
                                    }
                                });
                            }
                        }
                    },
                    Ok(PollEvent::Start(tx)) => {
                        is_paused = false;
                        for (tx, packets) in packet_buffer.drain(..) {
                            send_async(&thread_pool, url.clone(), cfg.sid().to_owned(), packets).receive(|res| {
                                match res {
                                    Ok(_) => tx.complete(()),
                                    Err(AsyncError::Failed(err)) => tx.fail(err),
                                    Err(AsyncError::Aborted) => tx.abort()
                                }
                            });
                        }
                        tx.complete(());
                    },
                    _ => return
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
                    Ok(Err(AsyncError::Failed(err))) => {
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

fn poll_async(tp: &ThreadPool, url: Url, timeout: Duration, sid: Option<String>) -> Future<Vec<Packet>, EngineError> {
    let (tx, f) = Future::pair();
    tp.execute(move || {
        let poll_res = poll(url, timeout, match sid {
            Some(ref string) => Some(string),
            None => None
        });
        match poll_res {
            Ok(packets) => tx.complete(packets),
            Err(err) => tx.fail(err)
        }
    });
    f
}

fn send(mut url: Url, sid: &str, packets: Vec<Packet>) -> Result<(), EngineError> {
    append_eio_parameters(&mut url, Some(sid));

    let capacity = packets.iter().fold(0usize, |val, p| val + p.try_compute_length(false).unwrap_or(0usize));
    let mut buf = Cursor::new(vec![0; capacity]);
    for packet in packets {
        try!(packet.write_payload_to(&mut buf));
    }
    let buf: &[_] = &buf.into_inner();

    match HTTP_CLIENT.post(url).body(buf).send() {
        Ok(_) => Ok(()),
        Err(err) => Err(err.into())
    }
}

fn send_async(tp: &ThreadPool, url: Url, sid: String, packets: Vec<Packet>) -> Future<(), EngineError> {
    let (tx, f) = Future::pair();
    tp.execute(move || {
        match send(url, &sid, packets){
            Ok(_) => tx.complete(()),
            Err(err) => tx.fail(err)
        }
    });
    f
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn connection() {
        use ::{EngineEvent, OpCode, Packet};
        use std::sync::mpsc::channel;
        use std::time::Duration;
        use eventual::*;
        use transports::Transport;
        use url::Url;

        let (tx, rx) = channel();
        let mut p = Polling::new(Url::parse("http://festify.us:5002/engine.io/").unwrap(), move |ev| {
            match ev {
                EngineEvent::Connect(_) => tx.send("connect".to_owned()).unwrap(),
                EngineEvent::ConnectError(_) => tx.send("connect_error".to_owned()).unwrap(),
                EngineEvent::Disconnect => tx.send("disconnect".to_owned()).unwrap(),
                EngineEvent::Error(_) => tx.send("error".to_owned()).unwrap(),
                EngineEvent::Message(msg) => tx.send("message ".to_owned() + &msg.to_string()).unwrap(),
                _ => {}
            }
        }).await().unwrap();

        assert_eq!("connect", &rx.recv().unwrap());
        assert!(rx.recv().unwrap().starts_with("message"), "Next engine event wasn't a message.");

        p.send(vec![Packet::with_str(OpCode::Message, "Hello Server!")]).await().unwrap();
        ::std::thread::sleep(Duration::from_millis(5000));
        p.close().await().unwrap();
    }
}