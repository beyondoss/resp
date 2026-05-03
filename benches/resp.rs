use beyond_resp::{RespCodec, Value};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

fn main() {
    divan::main();
}

// ── Decode benchmarks ────────────────────────────────────────────────────────

#[divan::bench]
fn decode_simple_string(b: divan::Bencher) {
    let frame = BytesMut::from(&b"+OK\r\n"[..]);
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_integer(b: divan::Bencher) {
    let frame = BytesMut::from(&b":12345\r\n"[..]);
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_bulk_string_small(b: divan::Bencher) {
    let frame = BytesMut::from(&b"$11\r\nhello world\r\n"[..]);
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_bulk_string_1kb(b: divan::Bencher) {
    let payload = vec![b'x'; 1024];
    let mut frame = format!("${}\r\n", payload.len()).into_bytes();
    frame.extend_from_slice(&payload);
    frame.extend_from_slice(b"\r\n");
    let frame = BytesMut::from(frame.as_slice());
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_bulk_string_64kb(b: divan::Bencher) {
    let payload = vec![b'x'; 65536];
    let mut frame = format!("${}\r\n", payload.len()).into_bytes();
    frame.extend_from_slice(&payload);
    frame.extend_from_slice(b"\r\n");
    let frame = BytesMut::from(frame.as_slice());
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_array_10_integers(b: divan::Bencher) {
    let mut raw = b"*10\r\n".to_vec();
    for i in 0..10i64 {
        raw.extend_from_slice(format!(":{i}\r\n").as_bytes());
    }
    let frame = BytesMut::from(raw.as_slice());
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_array_10_bulk_strings(b: divan::Bencher) {
    let mut raw = b"*10\r\n".to_vec();
    for _ in 0..10 {
        raw.extend_from_slice(b"$5\r\nhello\r\n");
    }
    let frame = BytesMut::from(raw.as_slice());
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_nested_array(b: divan::Bencher) {
    // 3-deep array, 3 elements each
    let frame = BytesMut::from(
        &b"*3\r\n*3\r\n:1\r\n:2\r\n:3\r\n*3\r\n:4\r\n:5\r\n:6\r\n*3\r\n:7\r\n:8\r\n:9\r\n"[..],
    );
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp2();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_map_5_entries(b: divan::Bencher) {
    let mut raw = b"%5\r\n".to_vec();
    for i in 0..5u8 {
        raw.extend_from_slice(format!("+key{i}\r\n:{i}\r\n").as_bytes());
    }
    let frame = BytesMut::from(raw.as_slice());
    b.bench(|| {
        let mut buf = frame.clone();
        let mut codec = RespCodec::resp3();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn decode_pipeline_100(b: divan::Bencher) {
    // 100 concatenated simple-string frames
    let frame: Vec<u8> = b"+OK\r\n".repeat(100);
    b.bench(|| {
        let mut buf = BytesMut::from(frame.as_slice());
        let mut codec = RespCodec::resp2();
        let mut count = 0usize;
        while codec.decode(&mut buf).unwrap().is_some() {
            count += 1;
        }
        divan::black_box(count)
    });
}

// ── Encode benchmarks ────────────────────────────────────────────────────────

#[divan::bench]
fn encode_simple_string(b: divan::Bencher) {
    let v = Value::SimpleString("OK".into());
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(8);
        let mut codec = RespCodec::resp2();
        codec.encode(divan::black_box(&v), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_integer(b: divan::Bencher) {
    let v = Value::Integer(12345);
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(16);
        let mut codec = RespCodec::resp2();
        codec.encode(divan::black_box(&v), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_bulk_string_small(b: divan::Bencher) {
    let v = Value::BulkString("hello world".into());
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(32);
        let mut codec = RespCodec::resp2();
        codec.encode(divan::black_box(&v), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_bulk_string_64kb(b: divan::Bencher) {
    let v = Value::BulkString(vec![b'x'; 65536].into());
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(65560);
        let mut codec = RespCodec::resp2();
        codec.encode(divan::black_box(&v), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_array_10_integers(b: divan::Bencher) {
    let v = Value::Array((0..10).map(Value::Integer).collect());
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(64);
        let mut codec = RespCodec::resp2();
        codec.encode(divan::black_box(&v), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_null_resp2(b: divan::Bencher) {
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(8);
        let mut codec = RespCodec::resp2();
        codec.encode(divan::black_box(&Value::Null), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_null_resp3(b: divan::Bencher) {
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(8);
        let mut codec = RespCodec::resp3();
        codec.encode(divan::black_box(&Value::Null), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

#[divan::bench]
fn encode_double(b: divan::Bencher) {
    let v = Value::Double(3.14159265358979);
    b.bench(|| {
        let mut dst = BytesMut::with_capacity(32);
        let mut codec = RespCodec::resp3();
        codec.encode(divan::black_box(&v), &mut dst).unwrap();
        divan::black_box(dst)
    });
}

// ── Roundtrip benchmarks ─────────────────────────────────────────────────────

#[divan::bench]
fn roundtrip_get_response(b: divan::Bencher) {
    let v = Value::BulkString("some-cached-value".into());
    b.bench(|| {
        let mut codec = RespCodec::resp2();
        let mut buf = BytesMut::with_capacity(32);
        codec.encode(divan::black_box(&v), &mut buf).unwrap();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}

#[divan::bench]
fn roundtrip_hgetall_response(b: divan::Bencher) {
    // 10-entry map — typical HGETALL response in RESP3
    let entries: Vec<(Value, Value)> = (0..10)
        .map(|i| {
            (
                Value::BulkString(format!("field{i}").into_bytes().into()),
                Value::BulkString(format!("value{i}").into_bytes().into()),
            )
        })
        .collect();
    let v = Value::Map(entries);
    b.bench(|| {
        let mut codec = RespCodec::resp3();
        let mut buf = BytesMut::with_capacity(256);
        codec.encode(divan::black_box(&v), &mut buf).unwrap();
        divan::black_box(codec.decode(&mut buf).unwrap())
    });
}
