mod common;

use beyond_resp::{RespCodec, Value, Version};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;

// ── Mock server ───────────────────────────────────────────────────────────────

/// Binds a loopback listener that serves one connection, replying to each
/// incoming frame with the next value from `script`.
async fn scripted_server(script: Vec<Value>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = Framed::new(socket, RespCodec::resp2());
        for response in script {
            let _ = server.next().await;
            server.send(response).await.unwrap();
        }
    });
    addr
}

async fn client(addr: SocketAddr) -> Framed<TcpStream, RespCodec> {
    let stream = TcpStream::connect(addr).await.unwrap();
    Framed::new(stream, RespCodec::resp2())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn single_request_response() {
    let addr = scripted_server(vec![Value::SimpleString("PONG".into())]).await;
    let mut c = client(addr).await;

    c.send(Value::Array(vec![Value::BulkString("PING".into())])).await.unwrap();

    let resp = c.next().await.unwrap().unwrap();
    assert_eq!(resp, Value::SimpleString("PONG".into()));
}

#[tokio::test]
async fn pipeline_three_commands() {
    // Send all 3 commands before reading any responses (pipeline).
    let addr = scripted_server(vec![
        Value::SimpleString("PONG".into()),
        Value::SimpleString("PONG".into()),
        Value::SimpleString("PONG".into()),
    ])
    .await;
    let mut c = client(addr).await;

    let ping = Value::Array(vec![Value::BulkString("PING".into())]);
    for _ in 0..3 {
        c.feed(ping.clone()).await.unwrap();
    }
    // flush() is ambiguous because RespCodec implements both Encoder<Value> and
    // Encoder<&Value>; pin the item type to resolve it.
    SinkExt::<Value>::flush(&mut c).await.unwrap();

    for i in 0..3 {
        let resp = c.next().await.unwrap().unwrap();
        assert_eq!(resp, Value::SimpleString("PONG".into()), "response {i}");
    }
}

#[tokio::test]
async fn resp3_upgrade_mid_stream() {
    // Simulates the HELLO 3 handshake:
    //   1. client sends HELLO 3 (RESP2 array)
    //   2. server switches to RESP3 and replies with a Map
    //   3. client switches decoder to RESP3 before reading the reply
    //   4. subsequent frames use RESP3 types (Boolean, Map, …)
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = Framed::new(socket, RespCodec::resp2());

        // Receive HELLO 3
        let _ = server.next().await;
        // Switch to RESP3 and respond with a Map (as Redis does)
        server.codec_mut().set_version(Version::Resp3);
        server
            .send(Value::Map(vec![(
                Value::BulkString("proto".into()),
                Value::Integer(3),
            )]))
            .await
            .unwrap();

        // Receive a follow-up command, respond with a RESP3-only Boolean
        let _ = server.next().await;
        server.send(Value::Boolean(true)).await.unwrap();
    });

    let stream = TcpStream::connect(addr).await.unwrap();
    let mut c = Framed::new(stream, RespCodec::resp2());

    // Send HELLO 3, then immediately switch the decoder. Real Redis sends the
    // HELLO response in the newly negotiated format, so the switch must happen
    // before the response is read.
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
        "expected RESP3 Map from HELLO response, got {hello_resp:?}"
    );

    // A RESP3-only type comes back correctly
    c.send(Value::Array(vec![Value::BulkString("PING".into())])).await.unwrap();
    let resp = c.next().await.unwrap().unwrap();
    assert_eq!(resp, Value::Boolean(true));
}

#[tokio::test]
async fn large_bulk_string() {
    let payload: Vec<u8> = (0u8..=255).cycle().take(1 << 16).collect(); // 64 KiB
    let expected = Value::BulkString(payload.clone().into());

    let addr = scripted_server(vec![expected.clone()]).await;
    let mut c = client(addr).await;
    c.send(Value::Array(vec![Value::BulkString("GET".into())])).await.unwrap();

    let resp = c.next().await.unwrap().unwrap();
    assert_eq!(resp, expected);
}

#[tokio::test]
async fn interleaved_push_messages() {
    // Push messages (>) can appear out-of-band between ordinary responses.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = Framed::new(socket, RespCodec::resp3());
        let _ = server.next().await; // consume client command
        // Send a Push followed by a regular response
        server
            .send(Value::Push(vec![
                Value::SimpleString("message".into()),
                Value::SimpleString("chan".into()),
                Value::BulkString("hello".into()),
            ]))
            .await
            .unwrap();
        server.send(Value::SimpleString("OK".into())).await.unwrap();
    });

    let stream = TcpStream::connect(addr).await.unwrap();
    let mut c = Framed::new(stream, RespCodec::resp3());
    c.send(Value::Array(vec![Value::BulkString("SUBSCRIBE".into())])).await.unwrap();

    let push = c.next().await.unwrap().unwrap();
    assert!(matches!(push, Value::Push(_)));

    let ok = c.next().await.unwrap().unwrap();
    assert_eq!(ok, Value::SimpleString("OK".into()));
}
