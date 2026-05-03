use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::encode::encode;
use crate::error::RespError;
use crate::parse;
use crate::value::{Value, Version};

/// Maximum frame size the decoder will accept (matches Redis default: 512 MiB).
///
/// Note: the parsed `Value` tree is larger than the wire frame. Compact types
/// like integers use ~4 wire bytes but ~56 bytes as a `Value` variant, so
/// peak heap usage can be 10–15× the frame size in the worst case. Size this
/// limit with that amplification in mind.
pub const DEFAULT_MAX_FRAME_BYTES: usize = 512 * 1024 * 1024;

/// Tokio-util codec for RESP2/RESP3 framing.
///
/// A single instance manages one connection's protocol state. Call
/// [`set_version`] after a successful `HELLO 3` handshake to switch to RESP3.
///
/// [`set_version`]: RespCodec::set_version
#[derive(Debug, Clone)]
pub struct RespCodec {
    version: Version,
    max_frame_bytes: usize,
}

impl Default for RespCodec {
    fn default() -> Self {
        Self::new(Version::Resp2)
    }
}

impl RespCodec {
    pub fn new(version: Version) -> Self {
        Self {
            version,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
        }
    }

    pub fn resp2() -> Self {
        Self::new(Version::Resp2)
    }

    pub fn resp3() -> Self {
        Self::new(Version::Resp3)
    }

    pub fn with_max_frame_bytes(mut self, limit: usize) -> Self {
        self.max_frame_bytes = limit;
        self
    }

    /// Switch protocol version mid-stream (e.g. after HELLO 3 succeeds).
    pub fn set_version(&mut self, version: Version) {
        self.version = version;
    }

    pub fn version(&self) -> Version {
        self.version
    }
}

impl Decoder for RespCodec {
    type Item = Value;
    type Error = RespError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Value>, RespError> {
        if src.is_empty() {
            return Ok(None);
        }
        if src.len() > self.max_frame_bytes {
            return Err(RespError::too_large(self.max_frame_bytes));
        }
        match parse::frame_len(src) {
            Ok(len) => {
                if len > self.max_frame_bytes {
                    return Err(RespError::too_large(self.max_frame_bytes));
                }
                let frozen = src.split_to(len).freeze();
                let mut pos = 0;
                Ok(Some(parse::build_value(&frozen, &mut pos, 0)?))
            }
            Err(RespError::Incomplete) => {
                src.reserve(64);
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

impl Encoder<Value> for RespCodec {
    type Error = RespError;

    fn encode(&mut self, item: Value, dst: &mut BytesMut) -> Result<(), RespError> {
        encode(&item, dst, self.version);
        Ok(())
    }
}

impl Encoder<&Value> for RespCodec {
    type Error = RespError;

    fn encode(&mut self, item: &Value, dst: &mut BytesMut) -> Result<(), RespError> {
        encode(item, dst, self.version);
        Ok(())
    }
}
