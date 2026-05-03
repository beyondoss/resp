//! Redis integration tests — require Docker (container is spun up automatically).
//!
//! Run with:
//!   cargo test --test redis

use beyond_resp::{RespCodec, Value, Version};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use testcontainers::{runners::AsyncRunner, ContainerRequest, ImageExt};
use testcontainers_modules::redis::Redis;

fn redis() -> ContainerRequest<Redis> {
    Redis::default().with_tag("7-alpine")
}
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

async fn connect(port: u16) -> Framed<TcpStream, RespCodec> {
    let stream = TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap();
    Framed::new(stream, RespCodec::resp2())
}

/// Send a command as a bulk-string array and return the response.
async fn cmd(c: &mut Framed<TcpStream, RespCodec>, args: &[&str]) -> Value {
    let command = Value::Array(
        args.iter()
            .map(|s| Value::BulkString(Bytes::copy_from_slice(s.as_bytes())))
            .collect(),
    );
    c.send(command).await.unwrap();
    c.next().await.unwrap().unwrap()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ping_pong() {
    let node = redis().start().await.unwrap();
    let port = node.get_host_port_ipv4(6379).await.unwrap();
    let mut c = connect(port).await;

    let resp = cmd(&mut c, &["PING"]).await;
    assert_eq!(resp, Value::SimpleString("PONG".into()));
}

#[tokio::test]
async fn set_get_del() {
    let node = redis().start().await.unwrap();
    let port = node.get_host_port_ipv4(6379).await.unwrap();
    let mut c = connect(port).await;

    let set = cmd(&mut c, &["SET", "key", "value"]).await;
    assert_eq!(set, Value::SimpleString("OK".into()));

    let get = cmd(&mut c, &["GET", "key"]).await;
    assert_eq!(get, Value::BulkString("value".into()));

    let del = cmd(&mut c, &["DEL", "key"]).await;
    assert_eq!(del, Value::Integer(1));

    let miss = cmd(&mut c, &["GET", "key"]).await;
    assert_eq!(miss, Value::Null);
}

#[tokio::test]
async fn pipeline_ten_pings() {
    let node = redis().start().await.unwrap();
    let port = node.get_host_port_ipv4(6379).await.unwrap();
    let mut c = connect(port).await;

    // Send 10 PINGs without reading any responses (true pipelining).
    let ping = Value::Array(vec![Value::BulkString("PING".into())]);
    for _ in 0..10 {
        c.feed(ping.clone()).await.unwrap();
    }
    SinkExt::<Value>::flush(&mut c).await.unwrap();

    for i in 0..10 {
        let resp = c.next().await.unwrap().unwrap();
        assert_eq!(resp, Value::SimpleString("PONG".into()), "pipeline response {i}");
    }
}

#[tokio::test]
async fn large_value_roundtrip() {
    let node = redis().start().await.unwrap();
    let port = node.get_host_port_ipv4(6379).await.unwrap();
    let mut c = connect(port).await;

    let payload: String = "x".repeat(1 << 20); // 1 MiB
    cmd(&mut c, &["SET", "big", &payload]).await;

    let resp = cmd(&mut c, &["GET", "big"]).await;
    assert_eq!(resp, Value::BulkString(Bytes::copy_from_slice(payload.as_bytes())));
}

#[tokio::test]
async fn resp3_hello_upgrade() {
    let node = redis().start().await.unwrap();
    let port = node.get_host_port_ipv4(6379).await.unwrap();
    let mut c = connect(port).await;

    // Send HELLO 3, then switch the decoder before reading the response.
    // Real Redis sends the HELLO reply in the newly negotiated format, so the
    // switch must happen before the response is read.
    c.send(Value::Array(vec![
        Value::BulkString("HELLO".into()),
        Value::BulkString("3".into()),
    ]))
    .await
    .unwrap();
    c.codec_mut().set_version(Version::Resp3);

    let hello_resp = c.next().await.unwrap().unwrap();
    assert!(
        matches!(hello_resp, Value::Map(_)),
        "expected RESP3 Map from HELLO 3, got {hello_resp:?}"
    );

    // In RESP3, HGETALL returns a Map instead of a flat Array.
    cmd(&mut c, &["HSET", "h", "f1", "v1", "f2", "v2"]).await;
    let hgetall = cmd(&mut c, &["HGETALL", "h"]).await;
    assert!(
        matches!(hgetall, Value::Map(ref pairs) if pairs.len() == 2),
        "expected RESP3 Map with 2 entries, got {hgetall:?}"
    );
}

#[tokio::test]
async fn error_response() {
    let node = redis().start().await.unwrap();
    let port = node.get_host_port_ipv4(6379).await.unwrap();
    let mut c = connect(port).await;

    cmd(&mut c, &["SET", "notanint", "hello"]).await;
    let resp = cmd(&mut c, &["INCR", "notanint"]).await;
    assert!(resp.is_error(), "expected error response, got {resp:?}");
}
