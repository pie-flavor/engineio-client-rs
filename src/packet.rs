use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::{BufRead, Error as IoError, ErrorKind, Read, Result as IoResult, Write};
use std::str::from_utf8;
use ::EngineError;
use rustc_serialize::base64::{FromBase64, STANDARD, ToBase64};

/// An engine.io message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Packet {
    pub opcode: OpCode,
    pub payload: Payload
}

impl Packet {
    /// Constructs a new `Packet`.
    pub fn new(opcode: OpCode, payload: Payload) -> Packet {
        Packet {
            opcode: opcode,
            payload: payload
        }
    }

    /// Creates an empty packet.
    pub fn empty(opcode: OpCode) -> Packet {
        Packet::with_string(opcode, String::new())
    }

    /// Constructs a new `Packet` with binary data.
    ///
    /// This method is a shorthand for `Packet::new(opcode, Payload::Binary(payload))`.
    pub fn with_binary(opcode: OpCode, payload: Vec<u8>) -> Packet {
        Packet::new(opcode, Payload::Binary(payload))
    }

    /// Constructs a new `Packet` with string data.
    ///
    /// This method is a shorthand for `Packet::new(opcode, Payload::String(payload.to_owned()))`.
    pub fn with_str(opcode: OpCode, payload: &str) -> Packet {
        Packet::with_string(opcode, payload.to_owned())
    }

    /// Constructs a new `Packet` with string data.
    ///
    /// This method is a shorthand for `Packet::new(opcode, Payload::String(payload))`.
    pub fn with_string(opcode: OpCode, payload: String) -> Packet {
        Packet::new(opcode, Payload::String(payload))
    }

    /// Parses a `Packet` from a string slice.
    pub fn from_str(buf: &str) -> Result<Packet, EngineError> {
        let mut chars = buf.chars();
        let opcode_char = chars.nth(0);
        match opcode_char {
            Some('b') => {
                let opcode_char = try!(chars.nth(0).ok_or(EngineError::Io(IoError::new(ErrorKind::UnexpectedEof, "The opcode could not be parsed because the string was too short."))));
                let opcode = try!(OpCode::parse_char(opcode_char));
                let b64 = try!(buf[2..].from_base64());
                Ok(Packet::with_binary(opcode, b64))
            },
            Some(ch @ '0'...'6') => {
                let opcode = try!(OpCode::parse_char(ch));
                Ok(Packet::with_str(opcode, &buf[1..]))
            },
            _ => Err(EngineError::invalid_data("Invalid opcode character or binary indicator."))
        }
    }

    /// Tries to parse a `Packet` from a `reader`. The reader will be read to its end.
    pub fn parse<R: Read>(reader: &mut R) -> Result<Packet, EngineError> {
        let mut buf = String::new();
        try!(reader.read_to_string(&mut buf));

        Packet::from_str(&buf)
    }

    /// Parses a list of packets in payload encoding from a `reader`. If an error occurs
    /// the packets that have been read until that point are returned.
    pub fn parse_all<R: BufRead>(reader: &mut R) -> Vec<Packet> {
        let mut results = Vec::new();
        loop {
            match Packet::parse_payload(reader) {
                Ok(packet) => results.push(packet),
                Err(_) => return results
            }
        }
    }

    /// Tries to parse a `Packet` in payload encoding from a `reader`. Only the data needed
    /// is read from the data source.
    pub fn parse_payload<R: BufRead>(reader: &mut R) -> Result<Packet, EngineError> {
        let data_length = {
            let mut buf = Vec::new();
            if let Ok(0) = reader.read_until(b':', &mut buf) {
                return Err(IoError::new(ErrorKind::UnexpectedEof, "Reader reached its end before the packet length could be read.").into());
            }
            let data_length_str = try!(from_utf8(&buf[..buf.len() - 1]));
            try!(data_length_str.parse::<usize>().map_err(|_| EngineError::invalid_data("The data length could not be parsed.")))
        };
        let mut buf = vec![0; data_length];
        try!(reader.read_exact(&mut buf));
        let data_str = try!(from_utf8(&buf));

        Packet::from_str(data_str)
    }

    /// Writes the `Packet` into the given `writer`.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> IoResult<()> {
        write!(writer, "{}", self.to_string())
    }

    /// Writes the packet as payload into the given `writer`.
    pub fn write_payload_to<W: Write>(&self, writer: &mut W) -> IoResult<()> {
        let data_to_write = self.to_string();
        let data_length = data_to_write.chars().count();
        write!(writer, "{}:{}", data_length, data_to_write)
    }
}

impl Default for Packet {
    fn default() -> Packet {
        Packet::empty(OpCode::Open)
    }
}

impl Display for Packet {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        let opcode_str = self.opcode.string_repr();
        match self.payload {
            Payload::Binary(ref data) => write!(formatter, "b{}{}", opcode_str, data.to_base64(STANDARD)),
            Payload::String(ref str) => write!(formatter, "{}{}", opcode_str, &str)
        }
    }
}

/// A message opcode.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[repr(u8)]
pub enum OpCode {
    /// Sent from the server when a new connection is opened.
    Open = 0,

    /// Send by the client to request the shutdown of the connection.
    Close = 1,

    /// A ping message sent by the client. The server will respond with
    /// a `Pong` message containing the same data.
    Ping = 2,

    /// The answer to a ping message.
    Pong = 3,

    /// An actual data message.
    Message = 4,

    /// Before engine.io switches a transport, it tests, if server and
    /// client can communicate over this transport. If the test succeeds,
    /// the client sends an upgrade packet which requests the server to
    /// flush its cache on the old transport and switch to the new transport.
    Upgrade = 5,

    /// A noop packet. Used for forcing a polling cycle.
    Noop = 6
}

impl OpCode {
    /// Creates a new OpCode from the given scalar value.
    pub fn from_scalar(value: u8) -> Result<OpCode, EngineError> {
        match value {
            0 => Ok(OpCode::Open),
            1 => Ok(OpCode::Close),
            2 => Ok(OpCode::Ping),
            3 => Ok(OpCode::Pong),
            4 => Ok(OpCode::Message),
            5 => Ok(OpCode::Upgrade),
            6 => Ok(OpCode::Noop),
            _ => Err(EngineError::invalid_data("Invalid opcode value."))
        }
    }

    /// Tries to parse an OpCode from a scalar value encoded as char.
    pub fn parse_char(value: char) -> Result<OpCode, EngineError> {
        match value {
            '0' => Ok(OpCode::Open),
            '1' => Ok(OpCode::Close),
            '2' => Ok(OpCode::Ping),
            '3' => Ok(OpCode::Pong),
            '4' => Ok(OpCode::Message),
            '5' => Ok(OpCode::Upgrade),
            '6' => Ok(OpCode::Noop),
            _ => Err(EngineError::invalid_data("Invalid opcode value."))
        }
    }

    /// Tries to parse an OpCode from a scalar value encoded as string.
    ///
    /// ## Panics
    /// Panics in case of an empty input string.
    pub fn parse_str(value: &str) -> Result<OpCode, EngineError> {
        assert!(!value.is_empty());

        OpCode::from_scalar(try!(value.parse::<u8>().map_err(|_| EngineError::invalid_data("Could not parse opcode value to integer."))))
    }

    /// Gets the string representation of the OpCode.
    pub fn string_repr(&self) -> String {
        (*self as u8).to_string()
    }
}

/// The message's payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Payload {
    /// The message contains binary data.
    Binary(Vec<u8>),

    /// The message contains UTF-8 string data.
    String(String)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_from_int_scalar() {
        assert_eq!(OpCode::Ping, OpCode::from_scalar(2).expect("Could not parse OpCode from int scalar."));
    }

    #[test]
    #[should_panic]
    fn opcode_from_int_scalar_panic() {
        OpCode::from_scalar(7).expect("Invalid char OpCode value to parse yielded an error. Test succeeded.");
    }

    #[test]
    fn opcode_from_char_scalar() {
        assert_eq!(OpCode::Ping, OpCode::parse_char('2').expect("Could not parse OpCode from char scalar."));
    }

    #[test]
    #[should_panic]
    fn opcode_from_char_scalar_panic() {
        OpCode::parse_char('7').expect("Invalid char OpCode value to parse yielded an error. Test succeeded.");
    }

    #[test]
    fn opcode_from_string_scalar() {
        assert_eq!(OpCode::Ping, OpCode::parse_str("2").expect("Could not parse OpCode from string scalar."));
    }

    #[test]
    #[should_panic]
    fn opcode_from_string_scalar_panic() {
        OpCode::parse_str("7").expect("Invalid string OpCode value to parse yielded an error. Test succeeded.");
    }

    #[test]
    fn opcode_string_repr() {
        assert_eq!(OpCode::Open.string_repr(), "0");
        assert_eq!(OpCode::Close.string_repr(), "1");
        assert_eq!(OpCode::Ping.string_repr(), "2");
        assert_eq!(OpCode::Pong.string_repr(), "3");
        assert_eq!(OpCode::Message.string_repr(), "4");
        assert_eq!(OpCode::Upgrade.string_repr(), "5");
        assert_eq!(OpCode::Noop.string_repr(), "6");
    }

    const BINARY_PAYLOAD: [u8; 9] = [1, 2, 3, 4, 6, 7, 8, 9, 10];
    const BINARY_PAYLOAD_B64: &'static str = "AQIDBAYHCAkK";
    const STRING_PAYLOAD: &'static str = "Hello World";

    #[test]
    fn packet_string_encoding() {
        let p = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        let p_enc = p.to_string();
        println!("{}", p_enc);
        assert_eq!(p_enc, format!("4{}", STRING_PAYLOAD));
    }

    #[test]
    fn packet_binary_encoding() {
        let p = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        let p_enc = p.to_string();
        println!("{}", p_enc);
        assert_eq!(p_enc, format!("b4{}", BINARY_PAYLOAD_B64));
    }

    #[test]
    fn payload_string_encoding() {
        let p = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        let mut buf = Vec::new();
        p.write_payload_to(&mut buf).expect("Writing string payload to buffer failed.");
        assert_eq!(buf, format!("12:4{}", STRING_PAYLOAD).as_bytes());
    }

    #[test]
    fn payload_binary_encoding() {
        let p = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        let mut buf = Vec::new();
        p.write_payload_to(&mut buf).expect("Writing binary payload to buffer failed.");
        assert_eq!(buf, format!("14:b4{}", BINARY_PAYLOAD_B64).as_bytes());
    }

    #[test]
    fn packet_string_decoding() {
        let p = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        test_packet_decoding_equality(&p);
    }

    #[test]
    fn packet_binary_decoding() {
        let p = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        test_packet_decoding_equality(&p);
    }

    fn test_packet_decoding_equality(p: &Packet) {
        let mut buf = Vec::new();
        p.write_to(&mut buf).expect("Failed to write packet to buffer.");
        let p_read = Packet::parse(&mut buf.as_slice()).expect("Failed to read packet from buffer.");
        assert_eq!(*p, p_read);
    }

    #[test]
    fn packet_empty_string_decoding() {
        let p = Packet::from_str("4").expect("Failed to parse empty string packet.");
        if let Payload::String(str) = p.payload {
            assert!(p.opcode == OpCode::Message);
            assert!(str.is_empty());
        } else {
            panic!("String packet was decoded to binary.");
        }
    }

    #[test]
    fn packet_empty_binary_decoding() {
        let p = Packet::from_str("b4").expect("Failed to parse empty string packet.");
        if let Payload::Binary(vec) = p.payload {
            assert!(p.opcode == OpCode::Message);
            assert!(vec.len() == 0);
        } else {
            panic!("Binary packet was decoded to string.");
        }
    }

    #[test]
    fn payload_string_decoding() {
        let p = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        test_payload_decoding_equality(&p);
    }

    #[test]
    fn payload_binary_decoding() {
        let p = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        test_payload_decoding_equality(&p);
    }

    fn test_payload_decoding_equality(p: &Packet) {
        let mut buf = Vec::new();
        p.write_payload_to(&mut buf).expect("Failed to write payload to buffer.");
        let p_read = Packet::parse_payload(&mut buf.as_slice()).expect("Failed to read payload from buffer.");
        assert_eq!(*p, p_read);
    }

    #[test]
    fn payload_multiple_decoding_single() {
        use std::io::Cursor;

        let p1 = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        let p2 = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        let mut buf = Cursor::new(Vec::new());
        p1.write_payload_to(&mut buf).expect("Failed to write string packet into buffer.");
        p2.write_payload_to(&mut buf).expect("Failed to write binary packet into buffer.");
        buf.set_position(0);

        let p1_dec = Packet::parse_payload(&mut buf).expect("Failed to decode string packet.");
        let p2_dec = Packet::parse_payload(&mut buf).expect("Failed to decode binary packet.");

        assert_eq!(p1, p1_dec);
        assert_eq!(p2, p2_dec);
    }

    #[test]
    fn payload_multiple_decoding_multiple() {
        use std::io::Cursor;

        let p1 = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        let p2 = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        let mut buf = Cursor::new(Vec::new());
        p1.write_payload_to(&mut buf).expect("Failed to write string packet into buffer.");
        p2.write_payload_to(&mut buf).expect("Failed to write binary packet into buffer.");
        buf.set_position(0);

        let dec = Packet::parse_all(&mut buf);
        assert!(dec.len() == 2, "Could not read all packets from buffer.");
        assert_eq!(dec[0], p1);
        assert_eq!(dec[1], p2);
    }

    #[test]
    fn payload_multiple_encoding() {
        use std::io::Cursor;
        use std::str::from_utf8;

        let p1 = Packet::with_str(OpCode::Message, STRING_PAYLOAD);
        let p2 = Packet::with_binary(OpCode::Message, BINARY_PAYLOAD.to_vec());
        let mut buf = Cursor::new(Vec::new());
        p1.write_payload_to(&mut buf).expect("Failed to write string packet into buffer.");
        p2.write_payload_to(&mut buf).expect("Failed to write binary packet into buffer.");
        let buf = buf.into_inner();
        let str = from_utf8(&buf).expect("Failed to convert written data into UTF-8.");

        assert_eq!(str, format!("12:4{}14:b4{}", STRING_PAYLOAD, BINARY_PAYLOAD_B64))
    }
}