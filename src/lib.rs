//! Redis Serialization Protocol (RESP2/RESP3) codec.
//!
//! Provides a [`RespCodec`] implementing tokio-util's [`Encoder`] and [`Decoder`]
//! traits for framing RESP messages over any async byte stream.

pub use codec::RespCodec;
pub use error::RespError;
pub use value::{Value, Version};

mod codec;
mod encode;
mod error;
mod parse;
mod value;
