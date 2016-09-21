//! An engine.io client library written in and for Rust.
//!
//! The first goal of this library is to reach feature-complete
//! status and full interoperability with the JS implementation.
//! Performance is always being worked on, though not focused on
//! primarily. Major improvements in that area can come once the
//! library is working properly and stable.

#![crate_name = "engineio"]
#![crate_type = "lib"]
#![feature(const_fn, io, never_type)]
#![cfg_attr(release, deny(warnings))]

extern crate futures;
#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate rustc_serialize;
extern crate tokio_core;
extern crate tokio_request;
extern crate url;
extern crate ws;

mod connection;
mod error;
mod packet;
mod transports;

pub use error::EngineError;
pub use packet::{OpCode, Packet, Payload};

use std::collections::HashMap;
use futures::BoxFuture;
use futures::stream::{channel, Receiver, Sender};
use tokio_core::reactor::Handle;
use url::Url;

/// Creates an engine.io connection to the given endpoint.
pub fn connect(url: &Url, h: Handle) -> BoxFuture<Receiver<Packet, EngineError>, EngineError> {
    ConnectionBuilder::new()
        .url(url)
        .build(h)
}

/// Creates an engine.io connection to the given endpoint.
pub fn connect_str(url: &str, h: Handle) -> BoxFuture<Receiver<Packet, EngineError>, EngineError> {
    connect(&Url::parse(url).unwrap(), h)
}

/// The struct that creates an engine.io connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionBuilder {
    extra_headers: Option<HashMap<String, String>>,
    path: Path,
    url: Option<Url>,
    user_agent: Option<String>
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Path {
    Append(String),

    AlreadyAppended,

    AppendIfEmpty
}

impl ConnectionBuilder {
    /// Creates a new [`ConnectionBuilder`](struct.ConnectionBuilder.html).
    pub const fn new() -> Self {
        ConnectionBuilder {
            extra_headers: None,
            path: Path::AppendIfEmpty,
            url: None,
            user_agent: None
        }
    }

    /// Asynchronously builds a new engine.io connection to the given endpoint.
    pub fn build(&self, h: Handle) -> BoxFuture<Receiver<Packet, EngineError>, EngineError> {
        if let Some(ref url) = self.url {

        } else {
            panic!("Missing url.");
        }

        unimplemented!();
    }

    /// Instructs the builder to take the given url as is and to not append an
    /// additional path at the end.
    pub fn do_not_append(mut self) -> Self {
        self.path = Path::AlreadyAppended;
        self
    }

    /// Sets a single extra header to be sent during each request to the server.
    pub fn extra_header(mut self, name: &str, value: &str) -> Self {
        if let Some(ref mut map) = self.extra_headers {
            map.insert(name.to_owned(), value.to_owned());
        } else {
            let mut map = HashMap::with_capacity(1);
            map.insert(name.to_owned(), value.to_owned());
            self.extra_headers = Some(map);
        }
        self
    }

    /// Sets the given headers to be sent during each request to the server.
    ///
    /// This overwrites all previously set headers.
    pub fn extra_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = Some(headers);
        self
    }

    /// Sets the path of the engine.io endpoint.
    ///
    /// If this or [`do_not_append`](struct.ConnectionBuilder.html#method.do_not_append) is not set,
    /// the [`ConnectionBuilder`](struct.ConnectionBuilder.html) will check for an existing path on
    /// the url. If one exists, it is not modified. Otherwise /engine.io/ will be appended to the path
    /// since that is where engine.io usually lives.
    pub fn path(mut self, path: &str) -> Self {
        self.path = Path::Append(path.to_owned());
        self
    }

    /// Sets the URL.
    pub fn url(mut self, url: &Url) -> Self {
        self.url = Some(url.clone());
        self
    }

    /// Sets the URL from a string slice.
    pub fn url_str(self, url: &str) -> Self {
        self.url(&Url::parse(url).unwrap())
    }

    /// Sets the user agent.
    pub fn user_agent(mut self, ua: &str) -> Self {
        self.user_agent = Some(ua.to_owned());
        self
    }
}