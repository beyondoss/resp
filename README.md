# beyond-resp

Frame and parse RESP2/RESP3 over any async byte stream

`RespCodec` implements tokio-util's `Encoder` and `Decoder` traits. Drop it onto a `Framed` transport and you have a working Redis wire codec.

## Install

```toml
[dependencies]
beyond-resp = "0.1"
```

Enable monoio support:

```toml
[dependencies]
beyond-resp = { version = "0.1", features = ["monoio"] }
```

## Quick Start

```rust
use beyond_resp::{RespCodec, Value};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

let mut codec = RespCodec::resp2();
let mut buf = BytesMut::new();

// Encode
codec.encode(Value::BulkString(b"hello"[..].into()), &mut buf)?;

// Decode
let mut incoming = BytesMut::from(&b"+OK\r\n"[..]);
let value = codec.decode(&mut incoming)?; // Some(Value::SimpleString("OK"))
```

With a tokio transport:

```rust
use beyond_resp::{RespCodec, Value};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use futures::{SinkExt, StreamExt};

let stream = TcpStream::connect("127.0.0.1:6379").await?;
let mut framed = Framed::new(stream, RespCodec::resp2());

framed.send(Value::Array(vec![
    Value::BulkString(b"GET"[..].into()),
    Value::BulkString(b"mykey"[..].into()),
])).await?;

if let Some(response) = framed.next().await {
    println!("{:?}", response?);
}
```

With a monoio transport:

```rust
use beyond_resp::{RespCodec, Value};
use monoio::net::TcpStream;
use monoio_codec::Decoder; // brings .framed() into scope

let stream = TcpStream::connect("127.0.0.1:6379").await?;
let mut framed = RespCodec::resp2().framed(stream);
```

`monoio_codec::Decoder` returns `Decoded<Value>` — `Decoded::Some(v)` when a frame is ready, `Decoded::Insufficient` when more bytes are needed.

## Protocol Version

`RespCodec::resp2()` and `RespCodec::resp3()` select the version at construction. Switch mid-stream after a `HELLO 3` handshake:

```rust
codec.set_version(Version::Resp3);
```

RESP3 enables `Map`, `Set`, `Push`, `Boolean`, `Double`, `BigNumber`, `VerbatimString`, `BulkError`, and `Attribute` types. RESP2 encodes `Null` as `$-1\r\n`; RESP3 uses `_\r\n`.

## Frame Size Limit

Default maximum frame size matches Redis at 512 MiB. Override:

```rust
let codec = RespCodec::resp2().with_max_frame_bytes(64 * 1024 * 1024);
```

Frames exceeding the limit return `RespError::FrameTooLarge`.

## Value Types

| Variant | RESP2 | RESP3 |
|---|---|---|
| `SimpleString` | ✓ | ✓ |
| `SimpleError` | ✓ | ✓ |
| `Integer` | ✓ | ✓ |
| `BulkString` | ✓ | ✓ |
| `Array` | ✓ | ✓ |
| `Null` | ✓ | ✓ |
| `Boolean` | | ✓ |
| `Double` | | ✓ |
| `BigNumber` | | ✓ |
| `BulkError` | | ✓ |
| `VerbatimString` | | ✓ |
| `Map` | | ✓ |
| `Attribute` | | ✓ |
| `Set` | | ✓ |
| `Push` | | ✓ |

## Performance

Benchmarked with [divan](https://github.com/nvzqis/divan) on an Apple M2 (MacBook Air, 8-core, 24 GB RAM).

The decoder uses a two-phase design: phase 1 walks the wire bytes to find the frame boundary without allocating; phase 2 builds the `Value` tree using zero-copy `Bytes::slice()` views into the receive buffer. String data (`SimpleString`, `SimpleError`, `BulkString`, `BulkError`, `BigNumber`, `VerbatimString`) is never copied — it points directly into the `BytesMut`.

```
decode_simple_string          36 ns
decode_integer                57 ns
decode_bulk_string   (11 B)   54 ns
decode_bulk_string   (1 KB)   92 ns
decode_bulk_string   (64 KB)  1.1 µs
decode_array         (10 i64) 273 ns
decode_array         (10 str) 500 ns
decode_nested_array           315 ns
decode_map           (5 KV)   273 ns
decode_pipeline_100           2.9 µs   (~29 ns/frame)

encode_null                   11 ns
encode_simple_string          15 ns
encode_integer                19 ns
encode_double                 29 ns
encode_bulk_string   (11 B)   32 ns
encode_bulk_string   (64 KB)  958 ns
encode_array         (10 i64) 140 ns

roundtrip_get_response        68 ns
roundtrip_hgetall_response    1.2 µs   (10-entry map)
```

Run benchmarks locally:

```sh
cargo bench
```

## License

MIT
