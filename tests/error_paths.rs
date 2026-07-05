mod common;

use common::parse_err;

use beyond_resp::{RespCodec, RespError, Value};
use bytes::BytesMut;
use tokio_util::codec::Decoder;

// ── Invalid type byte ────────────────────────────────────────────────────────

#[test]
fn invalid_type_byte() {
    for byte in [b'?', b'A', b'z', b'0', b'@', b'/'] {
        let wire = [byte, b'\r', b'\n'];
        assert!(
            matches!(parse_err(&wire), RespError::InvalidTypeByte { byte: b } if b == byte),
            "byte 0x{byte:02x}"
        );
    }
}

// ── Missing CRLF ─────────────────────────────────────────────────────────────

#[test]
fn missing_crlf_after_simple_string() {
    // \r not followed by \n
    assert!(matches!(
        parse_err(b"+OK\r\x00"),
        RespError::MissingCrlf
    ));
}

#[test]
fn missing_crlf_after_bulk_body() {
    // bulk string body terminated with \r then wrong byte
    assert!(matches!(
        parse_err(b"$3\r\nfoo\r\x00"),
        RespError::MissingCrlf
    ));
}

#[test]
fn missing_crlf_after_boolean() {
    assert!(matches!(
        parse_err(b"#t\r\x00"),
        RespError::MissingCrlf
    ));
}

// ── Invalid integer ──────────────────────────────────────────────────────────

#[test]
fn invalid_integer_empty() {
    assert!(matches!(parse_err(b":\r\n"), RespError::InvalidInteger));
}

#[test]
fn invalid_integer_sign_only() {
    assert!(matches!(parse_err(b":-\r\n"), RespError::InvalidInteger));
    assert!(matches!(parse_err(b":+\r\n"), RespError::InvalidInteger));
}

#[test]
fn invalid_integer_non_digit() {
    assert!(matches!(parse_err(b":12x\r\n"), RespError::InvalidInteger));
}

#[test]
fn invalid_integer_overflow() {
    // i64::MAX + 1
    assert!(matches!(
        parse_err(b":9223372036854775808\r\n"),
        RespError::InvalidInteger
    ));
    // i64::MIN - 1
    assert!(matches!(
        parse_err(b":-9223372036854775809\r\n"),
        RespError::InvalidInteger
    ));
}

// ── Invalid length ───────────────────────────────────────────────────────────

#[test]
fn invalid_length_negative_not_null() {
    // Only -1 is a valid null sentinel; anything else is an error
    assert!(matches!(parse_err(b"$-2\r\n"), RespError::InvalidLength));
    assert!(matches!(parse_err(b"*-2\r\n"), RespError::InvalidLength));
    assert!(matches!(parse_err(b"!-1\r\n"), RespError::InvalidLength));
    assert!(matches!(parse_err(b"%-1\r\n"), RespError::InvalidLength));
    assert!(matches!(parse_err(b"~-1\r\n"), RespError::InvalidLength));
    assert!(matches!(parse_err(b">-1\r\n"), RespError::InvalidLength));
    assert!(matches!(parse_err(b"|-1\r\n"), RespError::InvalidLength));
}

// ── Invalid double ───────────────────────────────────────────────────────────

#[test]
fn invalid_double_garbage() {
    assert!(matches!(parse_err(b",not-a-double\r\n"), RespError::InvalidDouble));
}

#[test]
fn invalid_double_empty() {
    assert!(matches!(parse_err(b",\r\n"), RespError::InvalidDouble));
}

// ── Invalid verbatim string ──────────────────────────────────────────────────

#[test]
fn invalid_verbatim_length_too_short() {
    // len must be >= 4 (3-byte encoding + ':' + at least 0 data bytes → minimum is 4)
    assert!(matches!(parse_err(b"=3\r\nabc\r\n"), RespError::InvalidVerbatim));
}

#[test]
fn invalid_verbatim_missing_colon_separator() {
    // 4 bytes, but byte at index 3 is not ':'
    assert!(matches!(parse_err(b"=4\r\nabcd\r\n"), RespError::InvalidVerbatim));
}

// ── Invalid big number ───────────────────────────────────────────────────────

#[test]
fn invalid_big_number_not_digits() {
    assert!(matches!(parse_err(b"(not-a-number\r\n"), RespError::InvalidBigNumber));
    assert!(matches!(parse_err(b"(12.34\r\n"), RespError::InvalidBigNumber));
}

#[test]
fn invalid_big_number_empty_after_sign() {
    assert!(matches!(parse_err(b"(-\r\n"), RespError::InvalidBigNumber));
}

#[test]
fn invalid_big_number_empty() {
    assert!(matches!(parse_err(b"(\r\n"), RespError::InvalidBigNumber));
}

// ── Depth limit ──────────────────────────────────────────────────────────────

/// Build a wire buffer with `n` levels of array-of-one nesting.
fn nested_array(levels: usize) -> Vec<u8> {
    let mut buf: Vec<u8> = b"*1\r\n".repeat(levels);
    buf.extend_from_slice(b":0\r\n");
    buf
}

#[test]
fn depth_limit_exceeded() {
    // 129 levels of nesting pushes parse_value to depth=129 > MAX_DEPTH=128
    let wire = nested_array(129);
    assert!(matches!(parse_err(&wire), RespError::DepthLimitExceeded));
}

#[test]
fn depth_limit_boundary_succeeds() {
    // Exactly 128 levels must succeed (depth 128 is == MAX_DEPTH, not >)
    let wire = nested_array(128);
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(wire.as_slice());
    assert!(
        codec.decode(&mut buf).unwrap().is_some(),
        "128 levels should be within the limit"
    );
}

// ── Frame size limit ─────────────────────────────────────────────────────────

#[test]
fn frame_too_large_reports_limit() {
    let mut codec = RespCodec::resp2().with_max_frame_bytes(4);
    let mut buf = BytesMut::from(&b"$10\r\nhelloworld\r\n"[..]);
    assert!(matches!(
        codec.decode(&mut buf).unwrap_err(),
        RespError::FrameTooLarge { limit: 4 }
    ));
}

// ── Post-error codec state ───────────────────────────────────────────────────

#[test]
fn error_leaves_buffer_unconsumed() {
    // On a protocol error, frame_len returns Err without split_to — bytes stay in src.
    // Callers using Framed will tear down the stream; direct users must clear manually.
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(&b"?\r\n"[..]);
    assert!(codec.decode(&mut buf).is_err());
    assert!(!buf.is_empty(), "invalid bytes should remain in the buffer after error");
}

#[test]
fn semantic_error_leaves_buffer_unconsumed() {
    for wire in [
        &b",not-a-double\r\n"[..],
        &b"(not-a-number\r\n"[..],
        &b"=4\r\nabcd\r\n"[..],
        &b"#x\r\n"[..],
    ] {
        let mut codec = RespCodec::resp2();
        let mut buf = BytesMut::from(wire);
        assert!(codec.decode(&mut buf).is_err(), "wire: {wire:?}");
        assert_eq!(buf.as_ref(), wire, "semantic error must not consume input");
    }
}

#[test]
fn codec_usable_after_buffer_cleared() {
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(&b"?\r\n"[..]);
    assert!(codec.decode(&mut buf).is_err());
    buf.clear();
    buf.extend_from_slice(b"+OK\r\n");
    assert_eq!(
        codec.decode(&mut buf).unwrap(),
        Some(Value::SimpleString("OK".into()))
    );
}

// ── Invalid big number: positive sign ────────────────────────────────────────

#[test]
fn invalid_big_number_positive_sign() {
    // Only '-' is a valid leading sign; '+' is not accepted
    assert!(matches!(parse_err(b"(+123\r\n"), RespError::InvalidBigNumber));
}

// ── Incomplete frames ────────────────────────────────────────────────────────

#[test]
fn incomplete_empty_buffer() {
    let mut codec = RespCodec::resp2();
    assert_eq!(codec.decode(&mut BytesMut::new()).unwrap(), None);
}

#[test]
fn incomplete_truncated_line() {
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(&b"+OK"[..]);
    assert_eq!(codec.decode(&mut buf).unwrap(), None);
}

#[test]
fn incomplete_truncated_bulk() {
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(&b"$11\r\nhello"[..]);
    assert_eq!(codec.decode(&mut buf).unwrap(), None);
}

#[test]
fn incomplete_partial_array() {
    // Array header says 2 elements, only 1 is present
    let mut codec = RespCodec::resp2();
    let mut buf = BytesMut::from(&b"*2\r\n+OK\r\n"[..]);
    assert_eq!(codec.decode(&mut buf).unwrap(), None);
}
