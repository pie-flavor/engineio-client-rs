mod polling;
mod websocket;

use ::EngineError;
use packet::Packet;

pub use self::polling::*;
pub use self::websocket::*;

pub trait Transport {
    fn close(&mut self) -> Result<(), EngineError>;

    fn pause(&mut self) -> Result<(), EngineError>;

    fn send(&mut self, msgs: &[Packet]) -> Result<(), EngineError>;

    fn start(&mut self) -> Result<(), EngineError>;
}