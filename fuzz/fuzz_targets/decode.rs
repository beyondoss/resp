#![no_main]

use beyond_resp::{RespCodec, Value, Version};
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use tokio_util::codec::{Decoder, Encoder};

fuzz_target!(|data: &[u8]| {
    for version in [Version::Resp2, Version::Resp3] {
        let mut codec = RespCodec::new(version);
        let mut buf = BytesMut::from(data);

        match codec.decode(&mut buf) {
            Ok(Some(value)) => {
                // Roundtrip invariant: encode(decode(x)) must decode to the same value.
                // Any panic here is a bug.
                let mut codec2 = RespCodec::new(version);
                let mut out = BytesMut::new();
                codec2.encode(value.clone(), &mut out).unwrap();
                let roundtripped = codec2.decode(&mut out).unwrap().unwrap();
                assert_roundtrip_eq(&value, &roundtripped);
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }
});

/// Equality check that handles NaN (NaN == NaN for our purposes here).
fn assert_roundtrip_eq(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Double(x), Value::Double(y)) => {
            assert!(
                x == y || (x.is_nan() && y.is_nan()),
                "Double roundtrip mismatch: {x} != {y}"
            );
        }
        (Value::Array(xs), Value::Array(ys))
        | (Value::Set(xs), Value::Set(ys))
        | (Value::Push(xs), Value::Push(ys)) => {
            assert_eq!(xs.len(), ys.len());
            for (x, y) in xs.iter().zip(ys.iter()) {
                assert_roundtrip_eq(x, y);
            }
        }
        (Value::Map(xs), Value::Map(ys)) => {
            assert_eq!(xs.len(), ys.len());
            for ((kx, vx), (ky, vy)) in xs.iter().zip(ys.iter()) {
                assert_roundtrip_eq(kx, ky);
                assert_roundtrip_eq(vx, vy);
            }
        }
        (Value::Attribute { attrs: ax, value: vx }, Value::Attribute { attrs: ay, value: vy }) => {
            assert_eq!(ax.len(), ay.len());
            for ((kx, vvx), (ky, vvy)) in ax.iter().zip(ay.iter()) {
                assert_roundtrip_eq(kx, ky);
                assert_roundtrip_eq(vvx, vvy);
            }
            assert_roundtrip_eq(vx, vy);
        }
        _ => assert_eq!(a, b, "roundtrip mismatch"),
    }
}
