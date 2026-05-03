#![allow(dead_code)]

use beyond_resp::{RespCodec, RespError, Value, Version};
use bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Decode a complete RESP2 frame from `wire`. Panics on error or incomplete input.
pub fn parse(wire: &[u8]) -> Value {
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(wire);
    codec
        .decode(&mut buf)
        .expect("decode error")
        .expect("incomplete frame")
}

/// Decode a complete RESP3 frame from `wire`. Panics on error or incomplete input.
pub fn parse3(wire: &[u8]) -> Value {
    let mut codec = RespCodec::resp3();
    let mut buf = BytesMut::from(wire);
    codec
        .decode(&mut buf)
        .expect("decode error")
        .expect("incomplete frame")
}

/// Decode from `wire` using RESP2, expecting an error. Panics if decoding succeeds.
pub fn parse_err(wire: &[u8]) -> RespError {
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(wire);
    codec.decode(&mut buf).expect_err("expected error, got Ok")
}

/// Encode `value` using RESP2 wire format.
pub fn wire2(value: &Value) -> Bytes {
    encode(value, Version::Resp2)
}

/// Encode `value` using RESP3 wire format.
pub fn wire3(value: &Value) -> Bytes {
    encode(value, Version::Resp3)
}

fn encode(value: &Value, version: Version) -> Bytes {
    let mut codec = RespCodec::new(version);
    let mut buf = BytesMut::new();
    codec.encode(value, &mut buf).unwrap();
    buf.freeze()
}
