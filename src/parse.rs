use bytes::Bytes;
use memchr::memchr;

use crate::error::RespError;
use crate::value::Value;

const MAX_DEPTH: u8 = 128;

// ── Phase 1: structural validation + frame boundary ──────────────────────────

/// Validate one complete RESP frame in `src` and return its byte length.
///
/// Returns `Err(Incomplete)` when more data is needed. All other errors are
/// protocol violations. Does not allocate.
pub(crate) fn frame_len(src: &[u8]) -> Result<usize, RespError> {
    let mut pos = 0;
    count_value(src, &mut pos, 0)?;
    Ok(pos)
}

fn count_value(src: &[u8], pos: &mut usize, depth: u8) -> Result<(), RespError> {
    if depth > MAX_DEPTH {
        return Err(RespError::DepthLimitExceeded);
    }
    require(src, *pos, 1)?;
    let prefix = src[*pos];
    *pos += 1;

    match prefix {
        b'+' | b'-' | b':' | b',' | b'(' => {
            read_line(src, pos)?;
        }
        b'_' => {
            expect_crlf(src, pos)?;
        }
        b'#' => {
            require(src, *pos, 3)?;
            *pos += 3;
        }
        b'$' => {
            let len = read_length(src, pos)?;
            match len {
                -1 => {}
                n if n < 0 => return Err(RespError::InvalidLength),
                n => skip_bulk(src, pos, n as usize)?,
            }
        }
        b'*' => {
            let len = read_length(src, pos)?;
            match len {
                -1 => {}
                n if n < 0 => return Err(RespError::InvalidLength),
                n => {
                    for _ in 0..n as usize {
                        count_value(src, pos, depth + 1)?;
                    }
                }
            }
        }
        b'!' | b'=' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            skip_bulk(src, pos, len as usize)?;
        }
        b'%' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            for _ in 0..len as usize * 2 {
                count_value(src, pos, depth + 1)?;
            }
        }
        b'|' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            for _ in 0..len as usize * 2 {
                count_value(src, pos, depth + 1)?;
            }
            count_value(src, pos, depth + 1)?;
        }
        b'~' | b'>' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            for _ in 0..len as usize {
                count_value(src, pos, depth + 1)?;
            }
        }
        byte => return Err(RespError::invalid_type(byte)),
    }
    Ok(())
}

// ── Phase 2: zero-copy value construction from frozen Bytes ──────────────────

/// Build a `Value` from a validated, frozen RESP frame.
///
/// `src` must be a complete frame as determined by `frame_len`. All string
/// data is returned as zero-copy `Bytes::slice()` views into `src`.
///
/// # Safety invariant
/// Every index into `src` is safe without bounds-checking because `frame_len`
/// (which runs `count_value`) has already confirmed that `src` contains a
/// structurally complete frame. Do not call this function on a partial buffer.
pub(crate) fn build_value(src: &Bytes, pos: &mut usize, depth: u8) -> Result<Value, RespError> {
    if depth > MAX_DEPTH {
        return Err(RespError::DepthLimitExceeded);
    }
    let prefix = src[*pos];
    *pos += 1;

    match prefix {
        b'+' => {
            let (s, e) = line_range(src, pos)?;
            Ok(Value::SimpleString(src.slice(s..e)))
        }
        b'-' => {
            let (s, e) = line_range(src, pos)?;
            Ok(Value::SimpleError(src.slice(s..e)))
        }
        b':' => {
            let line = read_line(src, pos)?;
            Ok(Value::Integer(parse_i64(line)?))
        }
        b'$' => {
            let len = read_length(src, pos)?;
            match len {
                -1 => Ok(Value::Null),
                n if n < 0 => Err(RespError::InvalidLength),
                n => {
                    let (s, e) = bulk_range(src, pos, n as usize)?;
                    Ok(Value::BulkString(src.slice(s..e)))
                }
            }
        }
        b'*' => {
            let len = read_length(src, pos)?;
            match len {
                -1 => Ok(Value::Null),
                n if n < 0 => Err(RespError::InvalidLength),
                n => Ok(Value::Array(build_sequence(src, pos, n as usize, depth)?)),
            }
        }
        b'_' => {
            expect_crlf(src, pos)?;
            Ok(Value::Null)
        }
        b'#' => {
            let b = match src[*pos] {
                b't' => true,
                b'f' => false,
                byte => return Err(RespError::invalid_type(byte)),
            };
            *pos += 1;
            expect_crlf(src, pos)?;
            Ok(Value::Boolean(b))
        }
        b',' => {
            let line = read_line(src, pos)?;
            Ok(Value::Double(parse_double(line)?))
        }
        b'(' => {
            let (s, e) = line_range(src, pos)?;
            validate_bignumber(&src[s..e])?;
            Ok(Value::BigNumber(src.slice(s..e)))
        }
        b'!' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            let (s, e) = bulk_range(src, pos, len as usize)?;
            Ok(Value::BulkError(src.slice(s..e)))
        }
        b'=' => {
            let len = read_length(src, pos)?;
            if len < 4 {
                return Err(RespError::InvalidVerbatim);
            }
            let (s, e) = bulk_range(src, pos, len as usize)?;
            if src[s + 3] != b':' {
                return Err(RespError::InvalidVerbatim);
            }
            let encoding = [src[s], src[s + 1], src[s + 2]];
            Ok(Value::VerbatimString { encoding, data: src.slice(s + 4..e) })
        }
        b'%' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            Ok(Value::Map(build_pairs(src, pos, len as usize, depth)?))
        }
        b'|' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            let attrs = build_pairs(src, pos, len as usize, depth)?;
            let value = Box::new(build_value(src, pos, depth + 1)?);
            Ok(Value::Attribute { attrs, value })
        }
        b'~' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            Ok(Value::Set(build_sequence(src, pos, len as usize, depth)?))
        }
        b'>' => {
            let len = read_length(src, pos)?;
            if len < 0 {
                return Err(RespError::InvalidLength);
            }
            Ok(Value::Push(build_sequence(src, pos, len as usize, depth)?))
        }
        byte => Err(RespError::invalid_type(byte)),
    }
}

fn build_sequence(
    src: &Bytes,
    pos: &mut usize,
    count: usize,
    depth: u8,
) -> Result<Vec<Value>, RespError> {
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(build_value(src, pos, depth + 1)?);
    }
    Ok(out)
}

fn build_pairs(
    src: &Bytes,
    pos: &mut usize,
    count: usize,
    depth: u8,
) -> Result<Vec<(Value, Value)>, RespError> {
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let k = build_value(src, pos, depth + 1)?;
        let v = build_value(src, pos, depth + 1)?;
        out.push((k, v));
    }
    Ok(out)
}

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Return the (start, end) absolute byte positions of the current line's
/// content (excluding CRLF), and advance `pos` past the CRLF.
#[inline]
fn line_range(src: &[u8], pos: &mut usize) -> Result<(usize, usize), RespError> {
    let buf = &src[*pos..];
    let cr = memchr(b'\r', buf).ok_or(RespError::Incomplete)?;
    if cr + 1 >= buf.len() {
        return Err(RespError::Incomplete);
    }
    if buf[cr + 1] != b'\n' {
        return Err(RespError::MissingCrlf);
    }
    let start = *pos;
    let end = *pos + cr;
    *pos += cr + 2;
    Ok((start, end))
}

/// Read a line and return its content as a slice (excludes CRLF).
#[inline]
fn read_line<'a>(src: &'a [u8], pos: &mut usize) -> Result<&'a [u8], RespError> {
    let (s, e) = line_range(src, pos)?;
    Ok(&src[s..e])
}

/// Read a length/count line and parse it as i64 (may be -1 for null).
#[inline]
fn read_length(src: &[u8], pos: &mut usize) -> Result<i64, RespError> {
    let line = read_line(src, pos)?;
    parse_i64(line)
}

/// Return the (start, end) absolute byte positions of `len` bytes of bulk
/// data, verify the trailing CRLF, and advance `pos` past it.
#[inline]
fn bulk_range(src: &[u8], pos: &mut usize, len: usize) -> Result<(usize, usize), RespError> {
    let start = *pos;
    let end = pos.checked_add(len).ok_or(RespError::InvalidLength)?;
    let term = end.checked_add(2).ok_or(RespError::InvalidLength)?;
    if src.len() < term {
        return Err(RespError::Incomplete);
    }
    if src[end] != b'\r' || src[end + 1] != b'\n' {
        return Err(RespError::MissingCrlf);
    }
    *pos = term;
    Ok((start, end))
}

/// Skip `len` bytes of bulk data and verify the trailing CRLF.
#[inline]
fn skip_bulk(src: &[u8], pos: &mut usize, len: usize) -> Result<(), RespError> {
    bulk_range(src, pos, len)?;
    Ok(())
}

/// Verify and consume a `\r\n`.
#[inline]
fn expect_crlf(src: &[u8], pos: &mut usize) -> Result<(), RespError> {
    if src.len() < *pos + 2 {
        return Err(RespError::Incomplete);
    }
    if src[*pos] != b'\r' || src[*pos + 1] != b'\n' {
        return Err(RespError::MissingCrlf);
    }
    *pos += 2;
    Ok(())
}

#[inline]
fn require(src: &[u8], pos: usize, n: usize) -> Result<(), RespError> {
    if src.len() >= pos + n { Ok(()) } else { Err(RespError::Incomplete) }
}

/// Parse an i64 from ASCII decimal bytes with optional leading sign.
/// No allocation; iterates raw bytes.
#[inline]
fn parse_i64(buf: &[u8]) -> Result<i64, RespError> {
    if buf.is_empty() {
        return Err(RespError::InvalidInteger);
    }
    let (neg, digits) = match buf[0] {
        b'-' => (true, &buf[1..]),
        b'+' => (false, &buf[1..]),
        _ => (false, buf),
    };
    if digits.is_empty() {
        return Err(RespError::InvalidInteger);
    }
    let mut n: i64 = 0;
    for &b in digits {
        if !b.is_ascii_digit() {
            return Err(RespError::InvalidInteger);
        }
        let digit = (b - b'0') as i64;
        n = if neg {
            n.checked_mul(10).and_then(|v| v.checked_sub(digit))
        } else {
            n.checked_mul(10).and_then(|v| v.checked_add(digit))
        }
        .ok_or(RespError::InvalidInteger)?;
    }
    Ok(n)
}

fn validate_bignumber(buf: &[u8]) -> Result<(), RespError> {
    let digits = match buf.first() {
        Some(b'-') => &buf[1..],
        _ => buf,
    };
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return Err(RespError::InvalidBigNumber);
    }
    Ok(())
}

fn parse_double(buf: &[u8]) -> Result<f64, RespError> {
    match buf {
        b"inf" => Ok(f64::INFINITY),
        b"-inf" => Ok(f64::NEG_INFINITY),
        b"nan" => Ok(f64::NAN),
        _ => std::str::from_utf8(buf)
            .map_err(|_| RespError::InvalidDouble)?
            .parse()
            .map_err(|_| RespError::InvalidDouble),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(src: &[u8]) -> Result<(Value, usize), RespError> {
        let len = frame_len(src)?;
        let frozen = Bytes::copy_from_slice(&src[..len]);
        let mut pos = 0;
        let value = build_value(&frozen, &mut pos, 0)?;
        Ok((value, len))
    }

    fn parse(input: &[u8]) -> Value {
        let (v, n) = parse_one(input).unwrap();
        assert_eq!(n, input.len(), "didn't consume full input");
        v
    }

    #[test]
    fn simple_string() {
        assert_eq!(parse(b"+OK\r\n"), Value::SimpleString("OK".into()));
        assert_eq!(parse(b"+\r\n"), Value::SimpleString(Bytes::new()));
    }

    #[test]
    fn simple_error() {
        assert_eq!(parse(b"-ERR bad\r\n"), Value::SimpleError("ERR bad".into()));
    }

    #[test]
    fn integer() {
        assert_eq!(parse(b":0\r\n"), Value::Integer(0));
        assert_eq!(parse(b":42\r\n"), Value::Integer(42));
        assert_eq!(parse(b":-1\r\n"), Value::Integer(-1));
        assert_eq!(
            parse(b":9223372036854775807\r\n"),
            Value::Integer(i64::MAX)
        );
        assert_eq!(
            parse(b":-9223372036854775808\r\n"),
            Value::Integer(i64::MIN)
        );
    }

    #[test]
    fn bulk_string() {
        assert_eq!(parse(b"$5\r\nhello\r\n"), Value::BulkString("hello".into()));
        assert_eq!(parse(b"$0\r\n\r\n"), Value::BulkString(Bytes::new()));
    }

    #[test]
    fn null_resp2() {
        assert_eq!(parse(b"$-1\r\n"), Value::Null);
        assert_eq!(parse(b"*-1\r\n"), Value::Null);
    }

    #[test]
    fn null_resp3() {
        assert_eq!(parse(b"_\r\n"), Value::Null);
    }

    #[test]
    fn array() {
        let v = parse(b"*2\r\n$5\r\nhello\r\n$5\r\nworld\r\n");
        assert_eq!(
            v,
            Value::Array(vec![
                Value::BulkString("hello".into()),
                Value::BulkString("world".into()),
            ])
        );
    }

    #[test]
    fn empty_array() {
        assert_eq!(parse(b"*0\r\n"), Value::Array(vec![]));
    }

    #[test]
    fn nested_array() {
        let v = parse(b"*2\r\n*2\r\n:1\r\n:2\r\n*1\r\n:3\r\n");
        assert_eq!(
            v,
            Value::Array(vec![
                Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
                Value::Array(vec![Value::Integer(3)]),
            ])
        );
    }

    #[test]
    fn boolean() {
        assert_eq!(parse(b"#t\r\n"), Value::Boolean(true));
        assert_eq!(parse(b"#f\r\n"), Value::Boolean(false));
    }

    #[test]
    fn double() {
        assert_eq!(parse(b",1.5\r\n"), Value::Double(1.5));
        assert_eq!(parse(b",inf\r\n"), Value::Double(f64::INFINITY));
        assert_eq!(parse(b",-inf\r\n"), Value::Double(f64::NEG_INFINITY));
        assert!(matches!(parse(b",nan\r\n"), Value::Double(f) if f.is_nan()));
    }

    #[test]
    fn big_number() {
        let input = b"(3492890328409238509324850943850943825024385\r\n";
        assert_eq!(
            parse(input),
            Value::BigNumber("3492890328409238509324850943850943825024385".into())
        );
    }

    #[test]
    fn big_number_negative() {
        let input = b"(-3492890328409238509324850943850943825024385\r\n";
        assert_eq!(
            parse(input),
            Value::BigNumber("-3492890328409238509324850943850943825024385".into())
        );
    }

    #[test]
    fn big_number_invalid() {
        assert!(matches!(
            parse_one(b"(not-a-number\r\n"),
            Err(RespError::InvalidBigNumber)
        ));
        assert!(matches!(
            parse_one(b"(\r\n"),
            Err(RespError::InvalidBigNumber)
        ));
    }

    #[test]
    fn bulk_error() {
        assert_eq!(
            parse(b"!21\r\nSYNTAX invalid syntax\r\n"),
            Value::BulkError("SYNTAX invalid syntax".into()),
        );
    }

    #[test]
    fn verbatim_string() {
        let v = parse(b"=15\r\ntxt:Some string\r\n");
        assert_eq!(
            v,
            Value::VerbatimString {
                encoding: *b"txt",
                data: "Some string".into()
            }
        );
    }

    #[test]
    fn map() {
        let v = parse(b"%2\r\n+first\r\n:1\r\n+second\r\n:2\r\n");
        assert_eq!(
            v,
            Value::Map(vec![
                (Value::SimpleString("first".into()), Value::Integer(1)),
                (Value::SimpleString("second".into()), Value::Integer(2)),
            ])
        );
    }

    #[test]
    fn set() {
        let v = parse(b"~3\r\n:1\r\n:2\r\n:3\r\n");
        assert_eq!(
            v,
            Value::Set(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)])
        );
    }

    #[test]
    fn push() {
        let v = parse(b">2\r\n+subscribe\r\n:1\r\n");
        assert_eq!(
            v,
            Value::Push(vec![
                Value::SimpleString("subscribe".into()),
                Value::Integer(1),
            ])
        );
    }

    #[test]
    fn attribute() {
        let input = b"|1\r\n+ttl\r\n:100\r\n+reply\r\n";
        assert_eq!(
            parse(input),
            Value::Attribute {
                attrs: vec![(Value::SimpleString("ttl".into()), Value::Integer(100))],
                value: Box::new(Value::SimpleString("reply".into())),
            }
        );
    }

    #[test]
    fn incomplete() {
        assert!(matches!(parse_one(b""), Err(RespError::Incomplete)));
        assert!(matches!(parse_one(b"+OK"), Err(RespError::Incomplete)));
        assert!(matches!(parse_one(b"$5\r\nhel"), Err(RespError::Incomplete)));
        assert!(matches!(
            parse_one(b"*2\r\n$5\r\nhello\r\n"),
            Err(RespError::Incomplete)
        ));
    }

    #[test]
    fn invalid_type_byte() {
        assert!(matches!(
            parse_one(b"?foo\r\n"),
            Err(RespError::InvalidTypeByte { byte: b'?' })
        ));
    }

    #[test]
    fn multiple_frames_in_buffer() {
        let input = b"+OK\r\n:42\r\n";
        let (v1, n1) = parse_one(input).unwrap();
        assert_eq!(v1, Value::SimpleString("OK".into()));
        let (v2, n2) = parse_one(&input[n1..]).unwrap();
        assert_eq!(v2, Value::Integer(42));
        assert_eq!(n1 + n2, input.len());
    }
}
