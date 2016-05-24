use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::Error as IoError;
use std::str::Utf8Error;
use hyper::Error as HttpError;
use rustc_serialize::base64::FromBase64Error;
use rustc_serialize::json::DecoderError;

/// The error type for engine.io associated operations.
#[derive(Debug)]
pub enum EngineError {
    /// An error occured while parsing the base-64 encoded binary data.
    Base64(FromBase64Error),

    /// An HTTP error occured.
    ///
    /// For example, the server sent an invalid status code.
    ///
    /// **Attention**: To keep the error hierarchy flat, this variant does
    /// _not_ contain I/O errors that were related to HTTP operations. Those
    /// are destructured and moved into the `EngineError::Io(IoError)` variant.
    Http(HttpError),

    /// The action could not be performed because of invalid data.
    ///
    /// For example, the data length of a payload-packet could not be parsed.
    InvalidData(Box<Error + Send + Sync>),

    /// The action could not be performed because the component was in
    /// an invalid state.
    ///
    /// For example, messages could not be sent through a `Connection`
    /// because it isn't connected.
    InvalidState(Box<Error + Send + Sync>),

    /// An I/O error occured.
    ///
    /// For example, the server unexpectedly closed the connection.
    Io(IoError),

    /// An error occured while parsing string data from UTF-8.
    Utf8
}

impl EngineError {
    /// Creates an `EngineError::InvalidData` variant. Mainly used in combination
    /// with string literals.
    ///
    /// ## Example
    /// ```
    /// # use engineio::EngineError;
    /// let e = EngineError::invalid_data("Data was invalid.");
    /// ```
    pub fn invalid_data<E: Into<Box<Error + Send + Sync>>>(err: E) -> EngineError {
        EngineError::InvalidData(err.into())
    }

    /// Creates an `EngineError::InvalidState` variant. Mainly used in combination
    /// with string literals.
    ///
    /// ## Example
    /// ```
    /// # use engineio::EngineError;
    /// let e = EngineError::invalid_state("Data was invalid.");
    /// ```
    pub fn invalid_state<E: Into<Box<Error + Send + Sync>>>(err: E) -> EngineError {
        EngineError::InvalidState(err.into())
    }
}

impl Display for EngineError {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        formatter.write_str(self.description())
    }
}

impl Error for EngineError {
    fn description(&self) -> &str {
        match *self {
            EngineError::Base64(ref err) => err.description(),
            EngineError::Http(ref err) => err.description(),
            EngineError::InvalidData(ref err) => err.description(),
            EngineError::InvalidState(ref err) => err.description(),
            EngineError::Io(ref err) => err.description(),
            EngineError::Utf8 => "UTF-8 data was invalid."
        }
    }

    fn cause(&self) -> Option<&Error> {
        match *self {
            EngineError::Base64(ref err) => Some(err),
            EngineError::Http(ref err) => Some(err),
            EngineError::InvalidData(ref err) => err.cause(),
            EngineError::InvalidState(ref err) => err.cause(),
            EngineError::Io(ref err) => Some(err),
            EngineError::Utf8 => None
        }
    }
}

impl From<DecoderError> for EngineError {
    fn from(err: DecoderError) -> EngineError {
        EngineError::InvalidData(Box::new(err))
    }
}

impl From<FromBase64Error> for EngineError {
    fn from(err: FromBase64Error) -> EngineError {
        EngineError::Base64(err)
    }
}

impl From<HttpError> for EngineError {
    fn from(err: HttpError) -> EngineError {
        match err {
            HttpError::Io(io_err) => EngineError::Io(io_err),
            _ => EngineError::Http(err)
        }
    }
}

impl From<IoError> for EngineError {
    fn from(err: IoError) -> EngineError {
        EngineError::Io(err)
    }
}

impl From<Utf8Error> for EngineError {
    fn from(_: Utf8Error) -> EngineError {
        EngineError::Utf8
    }
}