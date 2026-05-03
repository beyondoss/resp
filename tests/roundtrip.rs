use beyond_resp::{RespCodec, Value, Version};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

fn roundtrip(value: Value, version: Version) -> Value {
    let mut codec = RespCodec::new(version);
    let mut buf = BytesMut::new();
    codec.encode(value, &mut buf).unwrap();
    codec.decode(&mut buf).unwrap().expect("expected a complete frame")
}

fn encode_bytes(value: &Value, version: Version) -> BytesMut {
    let mut codec = RespCodec::new(version);
    let mut buf = BytesMut::new();
    codec.encode(value, &mut buf).unwrap();
    buf
}

// ── RESP2 roundtrips ─────────────────────────────────────────────────────────

#[test]
fn simple_string() {
    let v = Value::SimpleString("OK".into());
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn simple_error() {
    let v = Value::SimpleError("ERR something went wrong".into());
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn integer() {
    for n in [0i64, 1, -1, i64::MAX, i64::MIN + 1] {
        let v = Value::Integer(n);
        assert_eq!(roundtrip(v.clone(), Version::Resp2), v, "integer {n}");
    }
}

#[test]
fn bulk_string() {
    let v = Value::BulkString("hello world".into());
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn empty_bulk_string() {
    let v = Value::BulkString(bytes::Bytes::new());
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn bulk_string_binary() {
    let data: Vec<u8> = (0u8..=255).collect();
    let v = Value::BulkString(data.into());
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn null_resp2() {
    assert_eq!(roundtrip(Value::Null, Version::Resp2), Value::Null);
    // both RESP2 null forms decode to Null
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(&b"$-1\r\n"[..]);
    assert_eq!(codec.decode(&mut buf).unwrap(), Some(Value::Null));
    let mut buf = BytesMut::from(&b"*-1\r\n"[..]);
    assert_eq!(codec.decode(&mut buf).unwrap(), Some(Value::Null));
}

#[test]
fn null_resp3() {
    assert_eq!(roundtrip(Value::Null, Version::Resp3), Value::Null);
    let mut codec = RespCodec::resp3();
    let mut buf = BytesMut::from(&b"_\r\n"[..]);
    assert_eq!(codec.decode(&mut buf).unwrap(), Some(Value::Null));
}

#[test]
fn array_empty() {
    let v = Value::Array(vec![]);
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn array_integers() {
    let v = Value::Array((1..=5).map(Value::Integer).collect());
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

#[test]
fn array_nested() {
    let v = Value::Array(vec![
        Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
        Value::BulkString("hello".into()),
        Value::Null,
    ]);
    assert_eq!(roundtrip(v.clone(), Version::Resp2), v);
}

// ── RESP3 roundtrips ─────────────────────────────────────────────────────────

#[test]
fn boolean() {
    assert_eq!(roundtrip(Value::Boolean(true), Version::Resp3), Value::Boolean(true));
    assert_eq!(roundtrip(Value::Boolean(false), Version::Resp3), Value::Boolean(false));
}

#[test]
fn double_finite() {
    for f in [0.0f64, 1.5, -2.5, 1e100, f64::MIN_POSITIVE] {
        let v = Value::Double(f);
        assert_eq!(roundtrip(v.clone(), Version::Resp3), v, "double {f}");
    }
}

#[test]
fn double_special() {
    let v = roundtrip(Value::Double(f64::INFINITY), Version::Resp3);
    assert_eq!(v, Value::Double(f64::INFINITY));

    let v = roundtrip(Value::Double(f64::NEG_INFINITY), Version::Resp3);
    assert_eq!(v, Value::Double(f64::NEG_INFINITY));

    // NaN encodes to the literal "nan" token and decodes back to NaN
    let encoded = encode_bytes(&Value::Double(f64::NAN), Version::Resp3);
    assert_eq!(&encoded[..], b",nan\r\n");
    let nan_decoded = roundtrip(Value::Double(f64::NAN), Version::Resp3);
    assert!(matches!(nan_decoded, Value::Double(f) if f.is_nan()), "NaN should decode to NaN");
}

#[test]
fn big_number() {
    let n = "3492890328409238509324850943850943825024385";
    let v = Value::BigNumber(n.into());
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn big_number_edge_cases() {
    // single-digit zero
    let v = Value::BigNumber("0".into());
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);

    // negative single digit
    let v = Value::BigNumber("-1".into());
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);

    // very long number (300 digits)
    let long: String = "9".repeat(300);
    let v = Value::BigNumber(long.into());
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn bulk_error() {
    let v = Value::BulkError("SYNTAX invalid syntax".into());
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn verbatim_string() {
    let v = Value::VerbatimString {
        encoding: *b"txt",
        data: "Some string".into(),
    };
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn verbatim_string_encoding_variants() {
    for encoding in [*b"mkd", *b"url", *b"log"] {
        let v = Value::VerbatimString { encoding, data: "hello".into() };
        assert_eq!(roundtrip(v.clone(), Version::Resp3), v, "encoding {:?}", encoding);
    }

    // empty data with encoding
    let v = Value::VerbatimString { encoding: *b"txt", data: bytes::Bytes::new() };
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn map() {
    let v = Value::Map(vec![
        (Value::SimpleString("name".into()), Value::BulkString("alice".into())),
        (Value::SimpleString("age".into()), Value::Integer(30)),
    ]);
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn set() {
    let v = Value::Set(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]);
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn push() {
    let v = Value::Push(vec![
        Value::SimpleString("message".into()),
        Value::SimpleString("channel".into()),
        Value::BulkString("hello".into()),
    ]);
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

#[test]
fn attribute() {
    let v = Value::Attribute {
        attrs: vec![(Value::SimpleString("ttl".into()), Value::Integer(100))],
        value: Box::new(Value::Array(vec![Value::Integer(1), Value::Integer(2)])),
    };
    assert_eq!(roundtrip(v.clone(), Version::Resp3), v);
}

// ── Codec behaviour ──────────────────────────────────────────────────────────

#[test]
fn split_frame_delivery() {
    let mut codec = RespCodec::resp2();
    let frame = b"$11\r\nhello world\r\n";

    // Feed all but the last byte — should be incomplete
    let mut buf = BytesMut::from(&frame[..frame.len() - 1]);
    assert_eq!(codec.decode(&mut buf).unwrap(), None);

    // Feed the final byte
    buf.extend_from_slice(&frame[frame.len() - 1..]);
    assert_eq!(
        codec.decode(&mut buf).unwrap(),
        Some(Value::BulkString("hello world".into()))
    );
    assert!(buf.is_empty());
}

#[test]
fn multiple_frames_in_buffer() {
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::new();
    buf.extend_from_slice(b"+OK\r\n:42\r\n$5\r\nhello\r\n");

    assert_eq!(codec.decode(&mut buf).unwrap(), Some(Value::SimpleString("OK".into())));
    assert_eq!(codec.decode(&mut buf).unwrap(), Some(Value::Integer(42)));
    assert_eq!(codec.decode(&mut buf).unwrap(), Some(Value::BulkString("hello".into())));
    assert_eq!(codec.decode(&mut buf).unwrap(), None);
}

#[test]
fn set_version_mid_stream() {
    let mut codec = RespCodec::resp2();

    // Encode Null in RESP2 mode → $-1\r\n
    let mut buf = BytesMut::new();
    codec.encode(&Value::Null, &mut buf).unwrap();
    assert_eq!(&buf[..], b"$-1\r\n");
    let _ = codec.decode(&mut buf).unwrap();

    // Switch to RESP3; Null now encodes as _\r\n
    codec.set_version(Version::Resp3);
    let mut buf = BytesMut::new();
    codec.encode(&Value::Null, &mut buf).unwrap();
    assert_eq!(&buf[..], b"_\r\n");
}

#[test]
fn max_frame_bytes_enforced() {
    let mut codec = RespCodec::resp2().with_max_frame_bytes(4);
    let mut buf = BytesMut::from(&b"$10\r\nhelloworld\r\n"[..]);
    assert!(codec.decode(&mut buf).is_err());
}

#[test]
fn max_frame_bytes_exact_boundary_succeeds() {
    // "+OK\r\n" is exactly 5 bytes; limit == len should succeed (> not >=)
    let mut codec = RespCodec::resp2().with_max_frame_bytes(5);
    let mut buf = BytesMut::from(&b"+OK\r\n"[..]);
    assert_eq!(
        codec.decode(&mut buf).unwrap(),
        Some(Value::SimpleString("OK".into()))
    );

    // one byte under the frame size should fail
    let mut codec = RespCodec::resp2().with_max_frame_bytes(4);
    let mut buf = BytesMut::from(&b"+OK\r\n"[..]);
    assert!(codec.decode(&mut buf).is_err());
}

#[test]
fn encoder_by_ref_avoids_clone() {
    // Encoder<&Value> impl — pass by reference, no forced clone
    let v = Value::BulkString("hello".into());
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::new();
    codec.encode(&v, &mut buf).unwrap();
    assert_eq!(&buf[..], b"$5\r\nhello\r\n");
    // v is still usable
    assert_eq!(v, Value::BulkString("hello".into()));
}
