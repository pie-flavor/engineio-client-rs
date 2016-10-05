//! The module that contains the code for the connection builder.

use std::io::Error;

use connection::{self, Config, Sender, Receiver};
use transports::Data;

use futures::Future;
use tokio_core::reactor::Handle;
use url::Url;

const URL_CANNOT_BE_A_BASE: &'static str = "Cannot use given URL since it cannot be a base. See https://docs.rs/url/1.2.0/url/struct.Url.html#method.cannot_be_a_base for more information.";

/// A builder for an engine.io connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Builder {
    extra_headers: Vec<(String, String)>,
    path: Path,
    transport_config: Option<Data>,
    url: Url
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Path {
    Append(String),

    DoNotAppend,

    AppendIfEmpty
}

impl Builder {
    /// Creates a new [`Builder`](struct.Builder.html).
    ///
    /// ## Panics
    /// Panics if the URL is a cannot-be-a-base.
    pub fn new<U: Into<Url>>(url: U) -> Self {
        let url = url.into();
        if url.cannot_be_a_base() {
            panic!(URL_CANNOT_BE_A_BASE);
        }

        Builder {
            extra_headers: Vec::new(),
            path: Path::AppendIfEmpty,
            transport_config: None,
            url: url
        }
    }

    /// Creates a new [`Builder`](struct.Builder.html).
    ///
    /// ## Panics
    /// Panics if the given string is not a valid URL or is a cannot-be-a-base.
    pub fn new_with_str(url: &str) -> Self {
        Builder::new(Url::parse(url).unwrap())
    }

    /// Asynchronously builds a new engine.io connection to the given endpoint.
    pub fn build(mut self, h: &Handle) -> Box<Future<Item=(Sender, Receiver), Error=Error>> {
        let c = Config {
            extra_headers: self.extra_headers,
            url: match self.path {
                Path::DoNotAppend => self.url,
                Path::Append(path) => {
                    self.url.path_segments_mut().unwrap().push(&path);
                    self.url
                },
                Path::AppendIfEmpty => {
                    if self.url.path_segments().unwrap().filter(|seg| !seg.is_empty()).count() == 0 {
                        self.url.path_segments_mut().unwrap().push("engine.io");
                    }
                    self.url
                }
            }
        };
        connection::connect(c, h.clone())
    }

    /// Instructs the builder to take the given url as is and to not append an
    /// additional path at the end.
    ///
    /// See [`Builder::path`](struct.Builder.html#method.path) for more information.
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
    /// If this or [`Builder::do_not_append`](struct.Builder.html#method.do_not_append) is not set,
    /// the [`Builder`](struct.Builder.html) will check for an existing path on the url.
    /// If one exists, it is not modified. Otherwise `/engine.io/` will be appended to
    /// the URL since that is where engine.io usually lives / spawns its server.
    ///
    /// In case of socket.io, the path is `/socket.io/`.
    pub fn path(mut self, path: &str) -> Self {
        self.path = Path::Append(path.to_owned());
        self
    }

    /// Sets the transport configuration for reconnecting to a broken session.
    pub fn transport_config(mut self, data: Data) -> Self {
        self.transport_config = Some(data);
        self
    }

    /// Sets the user agent.
    pub fn user_agent(self, ua: &str) -> Self {
        self.extra_header("User-Agent", ua)
    }
}