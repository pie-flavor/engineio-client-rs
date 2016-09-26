//! An engine.io client library written in and for Rust.

#![allow(dead_code)]
#![warn(missing_docs)]
#![feature(io, never_type)]

#![cfg_attr(release, deny(warnings))]

extern crate futures;
extern crate rand;
extern crate rustc_serialize;
extern crate tokio_core;
extern crate tokio_request;
extern crate url;
extern crate ws;

mod connection;
mod error;
mod packet;
pub mod transports;

use futures::BoxFuture;
use tokio_core::reactor::Handle;
use url::Url;

pub use connection::{Receiver, Sender};
pub use error::EngineError;
pub use packet::{OpCode, Packet, Payload};

/// A builder for an engine.io connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Builder {
    extra_headers: Vec<(String, String)>,
    path: Path,
    url: Option<Url>
}

/// Contains the configuration for creating a new connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Extra headers to pass during each request.
    pub extra_headers: Vec<(String, String)>,
    /// The engine.io endpoint.
    pub url: Url
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Path {
    Append(String),

    DoNotAppend,

    AppendIfEmpty
}

/// Creates an engine.io connection to the given endpoint.
pub fn connect(url: &Url, h: &Handle) -> BoxFuture<(Sender, Receiver), EngineError> {
    Builder::new()
        .url(url)
        .build(h)
}

/// Creates an engine.io connection to the given endpoint.
pub fn connect_str(url: &str, h: &Handle) -> BoxFuture<(Sender, Receiver), EngineError> {
    connect(&Url::parse(url).unwrap(), h)
}

impl Builder {
    /// Creates a new [`Builder`](struct.Builder.html).
    pub fn new() -> Self {
        Builder {
            extra_headers: Vec::new(),
            path: Path::AppendIfEmpty,
            url: None
        }
    }

    /// Asynchronously builds a new engine.io connection to the given endpoint.
    ///
    /// ## Panics
    /// Panics if the URL hasn't been set.
    pub fn build(self, h: &Handle) -> BoxFuture<(Sender, Receiver), EngineError> {
        if let Some(mut url) = self.url {
            let c = Config {
                extra_headers: self.extra_headers,
                url: match self.path {
                    Path::DoNotAppend => url,
                    Path::Append(path) => {
                        url.path_segments_mut().unwrap().push(&path);
                        url
                    },
                    Path::AppendIfEmpty => {
                        if url.path_segments().unwrap().filter(|seg| !seg.is_empty()).count() == 0 {
                            url.path_segments_mut().unwrap().push("engine.io");
                        }
                        url
                    }
                }
            };
            connection::connect(c, h.clone())
        } else {
            panic!("Missing url.");
        }
    }

    /// Instructs the builder to take the given url as is and to not append an
    /// additional path at the end.
    pub fn do_not_append(mut self) -> Self {
        self.path = Path::DoNotAppend;
        self
    }

    /// Sets a single extra header to be sent during each request to the server.
    pub fn extra_header(mut self, name: &str, value: &str) -> Self {
        self.extra_headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Sets the given headers to be sent during each request to the server.
    ///
    /// This overwrites all previously set headers.
    pub fn extra_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Sets the path of the engine.io endpoint.
    ///
    /// If this or [`do_not_append`](struct.Builder.html#method.do_not_append) is not set,
    /// the [`Builder`](struct.Builder.html) will check for an existing path on the url.
    /// If one exists, it is not modified. Otherwise `/engine.io/` will be appended to
    /// the URL since that is where engine.io usually lives / spawns it's server.
    ///
    /// In case of socket.io, the path is `/socket.io/`.
    pub fn path(mut self, path: &str) -> Self {
        self.path = Path::Append(path.to_owned());
        self
    }

    /// Sets the URL.
    pub fn url(mut self, url: &Url) -> Self {
        if url.cannot_be_a_base() {
            panic!("Cannot use given URL since it cannot be a base. See https://docs.rs/url/1.2.0/url/struct.Url.html#method.cannot_be_a_base for more information.");
        }

        self.url = Some(url.clone());
        self
    }

    /// Sets the URL from a string slice.
    pub fn url_str(self, url: &str) -> Self {
        self.url(&Url::parse(url).unwrap())
    }

    /// Sets the user agent.
    pub fn user_agent(self, ua: &str) -> Self {
        self.extra_header("User-Agent", ua)
    }
}