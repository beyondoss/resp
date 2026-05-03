use bytes::{BufMut, BytesMut};

use crate::value::{Value, Version};

/// Encode `value` into `dst` using the given protocol version.
///
/// `Version::Resp2` encodes `Null` as `$-1\r\n`.
/// `Version::Resp3` encodes `Null` as `_\r\n`.
/// All RESP3-only types use their native wire form regardless of version —
/// the caller controls which variants it constructs.
pub fn encode(value: &Value, dst: &mut BytesMut, version: Version) {
    match value {
        Value::SimpleString(s) => {
            dst.put_u8(b'+');
            dst.put_slice(s);
            dst.put_slice(b"\r\n");
        }
        Value::SimpleError(e) => {
            dst.put_u8(b'-');
            dst.put_slice(e);
            dst.put_slice(b"\r\n");
        }
        Value::Integer(n) => {
            let mut buf = itoa::Buffer::new();
            dst.put_u8(b':');
            dst.put_slice(buf.format(*n).as_bytes());
            dst.put_slice(b"\r\n");
        }
        Value::BulkString(data) => {
            write_bulk(b'$', data, dst);
        }
        Value::Array(elements) => {
            write_aggregate(b'*', elements, dst, version);
        }
        Value::Null => match version {
            Version::Resp2 => dst.put_slice(b"$-1\r\n"),
            Version::Resp3 => dst.put_slice(b"_\r\n"),
        },
        Value::Boolean(b) => {
            dst.put_slice(if *b { b"#t\r\n" } else { b"#f\r\n" });
        }
        Value::Double(f) => {
            dst.put_u8(b',');
            if f.is_nan() {
                dst.put_slice(b"nan");
            } else if f.is_infinite() {
                dst.put_slice(if f.is_sign_positive() { b"inf" } else { b"-inf" });
            } else {
                let mut buf = ryu::Buffer::new();
                dst.put_slice(buf.format(*f).as_bytes());
            }
            dst.put_slice(b"\r\n");
        }
        Value::BigNumber(n) => {
            dst.put_u8(b'(');
            dst.put_slice(n);
            dst.put_slice(b"\r\n");
        }
        Value::BulkError(data) => {
            write_bulk(b'!', data, dst);
        }
        Value::VerbatimString { encoding, data } => {
            let mut buf = itoa::Buffer::new();
            let total = 4 + data.len(); // enc(3) + ':' + data
            dst.put_u8(b'=');
            dst.put_slice(buf.format(total).as_bytes());
            dst.put_slice(b"\r\n");
            dst.put_slice(encoding);
            dst.put_u8(b':');
            dst.put_slice(data);
            dst.put_slice(b"\r\n");
        }
        Value::Map(entries) => {
            let mut buf = itoa::Buffer::new();
            dst.put_u8(b'%');
            dst.put_slice(buf.format(entries.len()).as_bytes());
            dst.put_slice(b"\r\n");
            for (k, v) in entries {
                encode(k, dst, version);
                encode(v, dst, version);
            }
        }
        Value::Attribute { attrs, value } => {
            let mut buf = itoa::Buffer::new();
            dst.put_u8(b'|');
            dst.put_slice(buf.format(attrs.len()).as_bytes());
            dst.put_slice(b"\r\n");
            for (k, v) in attrs {
                encode(k, dst, version);
                encode(v, dst, version);
            }
            encode(value, dst, version);
        }
        Value::Set(elements) => {
            write_aggregate(b'~', elements, dst, version);
        }
        Value::Push(elements) => {
            write_aggregate(b'>', elements, dst, version);
        }
    }
}

#[inline]
fn write_bulk(prefix: u8, data: &[u8], dst: &mut BytesMut) {
    let mut buf = itoa::Buffer::new();
    dst.put_u8(prefix);
    dst.put_slice(buf.format(data.len()).as_bytes());
    dst.put_slice(b"\r\n");
    dst.put_slice(data);
    dst.put_slice(b"\r\n");
}

#[inline]
fn write_aggregate(prefix: u8, elements: &[Value], dst: &mut BytesMut, version: Version) {
    let mut buf = itoa::Buffer::new();
    dst.put_u8(prefix);
    dst.put_slice(buf.format(elements.len()).as_bytes());
    dst.put_slice(b"\r\n");
    for elem in elements {
        encode(elem, dst, version);
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    fn enc(value: &Value) -> Vec<u8> {
        let mut dst = BytesMut::new();
        encode(value, &mut dst, Version::Resp2);
        dst.to_vec()
    }

    fn enc3(value: &Value) -> Vec<u8> {
        let mut dst = BytesMut::new();
        encode(value, &mut dst, Version::Resp3);
        dst.to_vec()
    }

    #[test]
    fn simple_string() {
        assert_eq!(enc(&Value::SimpleString("OK".into())), b"+OK\r\n");
        assert_eq!(enc(&Value::SimpleString(Bytes::new())), b"+\r\n");
    }

    #[test]
    fn simple_error() {
        assert_eq!(
            enc(&Value::SimpleError("ERR bad".into())),
            b"-ERR bad\r\n"
        );
    }

    #[test]
    fn integer() {
        assert_eq!(enc(&Value::Integer(42)), b":42\r\n");
        assert_eq!(enc(&Value::Integer(-1)), b":-1\r\n");
        assert_eq!(enc(&Value::Integer(0)), b":0\r\n");
    }

    #[test]
    fn bulk_string() {
        assert_eq!(enc(&Value::BulkString("hello".into())), b"$5\r\nhello\r\n");
        assert_eq!(enc(&Value::BulkString(Bytes::new())), b"$0\r\n\r\n");
    }

    #[test]
    fn null_resp2() {
        assert_eq!(enc(&Value::Null), b"$-1\r\n");
    }

    #[test]
    fn null_resp3() {
        assert_eq!(enc3(&Value::Null), b"_\r\n");
    }

    #[test]
    fn array() {
        let v = Value::Array(vec![Value::Integer(1), Value::Integer(2)]);
        assert_eq!(enc(&v), b"*2\r\n:1\r\n:2\r\n");
        assert_eq!(enc(&Value::Array(vec![])), b"*0\r\n");
    }

    #[test]
    fn boolean() {
        assert_eq!(enc3(&Value::Boolean(true)), b"#t\r\n");
        assert_eq!(enc3(&Value::Boolean(false)), b"#f\r\n");
    }

    #[test]
    fn double() {
        assert_eq!(enc3(&Value::Double(f64::INFINITY)), b",inf\r\n");
        assert_eq!(enc3(&Value::Double(f64::NEG_INFINITY)), b",-inf\r\n");
        assert_eq!(enc3(&Value::Double(f64::NAN)), b",nan\r\n");
        assert_eq!(enc3(&Value::Double(1.5)), b",1.5\r\n");
    }

    #[test]
    fn verbatim_string() {
        let v = Value::VerbatimString {
            encoding: *b"txt",
            data: "hello".into(),
        };
        assert_eq!(enc3(&v), b"=9\r\ntxt:hello\r\n");
    }

    #[test]
    fn map() {
        let v = Value::Map(vec![(Value::SimpleString("k".into()), Value::Integer(1))]);
        assert_eq!(enc3(&v), b"%1\r\n+k\r\n:1\r\n");
    }

    #[test]
    fn attribute() {
        let v = Value::Attribute {
            attrs: vec![(Value::SimpleString("ttl".into()), Value::Integer(100))],
            value: Box::new(Value::Integer(42)),
        };
        assert_eq!(enc3(&v), b"|1\r\n+ttl\r\n:100\r\n:42\r\n");
    }

    #[test]
    fn set() {
        let v = Value::Set(vec![Value::Integer(1), Value::Integer(2)]);
        assert_eq!(enc3(&v), b"~2\r\n:1\r\n:2\r\n");
    }

    #[test]
    fn push() {
        let v = Value::Push(vec![Value::SimpleString("message".into()), Value::Integer(3)]);
        assert_eq!(enc3(&v), b">2\r\n+message\r\n:3\r\n");
    }
}
