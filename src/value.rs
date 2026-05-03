use bytes::Bytes;

/// Protocol version — controls null wire encoding and enables RESP3 types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Version {
    #[default]
    Resp2,
    Resp3,
}

/// A parsed RESP value covering all RESP2 and RESP3 wire types.
///
/// `PartialEq` is derived; `f64::NAN != f64::NAN` per IEEE 754 is correct behaviour.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    // ── RESP2 ──────────────────────────────────────────────────────────────

    /// `+<str>\r\n` — short non-binary status string, e.g. `"OK"`
    SimpleString(Bytes),
    /// `-<msg>\r\n` — error (raw bytes, includes the kind prefix e.g. `"ERR bad"`)
    SimpleError(Bytes),
    /// `:<n>\r\n` — 64-bit signed integer
    Integer(i64),
    /// `$<len>\r\n<data>\r\n` — binary-safe bulk string
    BulkString(Bytes),
    /// `*<count>\r\n<elements>` — ordered array
    Array(Vec<Value>),

    // ── Unified null (RESP2: $-1\r\n / *-1\r\n ; RESP3: _\r\n) ────────────
    Null,

    // ── RESP3 ──────────────────────────────────────────────────────────────

    /// `#t\r\n` / `#f\r\n`
    Boolean(bool),
    /// `,<value>\r\n` — IEEE 754 double; encodes `inf`, `-inf`, `nan`
    Double(f64),
    /// `(<decimal>\r\n` — arbitrary-precision integer as raw decimal bytes (no bignum dep)
    BigNumber(Bytes),
    /// `!<len>\r\n<data>\r\n` — binary-safe error payload
    BulkError(Bytes),
    /// `=<len>\r\n<enc>:<data>\r\n` — string with 3-byte encoding hint
    VerbatimString { encoding: [u8; 3], data: Bytes },
    /// `%<count>\r\n<key><value>...` — key-value map
    Map(Vec<(Value, Value)>),
    /// `|<count>\r\n<key><value>...<reply>` — attribute metadata + actual reply
    Attribute { attrs: Vec<(Value, Value)>, value: Box<Value> },
    /// `~<count>\r\n<elements>` — unordered unique set
    Set(Vec<Value>),
    /// `><count>\r\n<elements>` — out-of-band push message
    Push(Vec<Value>),
}

impl Value {
    /// Returns `true` if this value is an error type.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::SimpleError(_) | Self::BulkError(_))
    }

    /// Returns `true` if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_error_covers_all_variants() {
        assert!(Value::SimpleError(Bytes::from("ERR msg")).is_error());
        assert!(Value::BulkError(Bytes::from("SYNTAX detail")).is_error());
        assert!(!Value::SimpleString(Bytes::from("OK")).is_error());
        assert!(!Value::Integer(0).is_error());
        assert!(!Value::Null.is_error());
        assert!(!Value::Boolean(false).is_error());
    }

    #[test]
    fn is_null_only_for_null_variant() {
        assert!(Value::Null.is_null());
        assert!(!Value::Integer(0).is_null());
        assert!(!Value::SimpleString(Bytes::from("")).is_null());
        assert!(!Value::Boolean(false).is_null());
        assert!(!Value::BulkString(Bytes::new()).is_null());
    }
}
