use std::io::Result;
use transports::Config;
use error::EngineError;
use packet::Packet;

pub trait Handler {
    fn on_connect(&mut self, Config) -> Result<()> {
        Ok(())
    }

    fn on_connect_error(&mut self, EngineError) -> Result<()> {
        Ok(())
    }

    fn on_disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    fn on_error(&mut self) -> Result<()> {
        Ok(())
    }

    fn on_packet(&mut self, Packet) -> Result<()>;
}

impl<F: FnMut(Packet) -> Result<()>> Handler for F {
    fn on_packet(&mut self, p: Packet) -> Result<()> {
        self(p);
    }
}