use serde::Serialize;
use serde_json::ser::{Formatter, Serializer};
use std::io;

/// Canonical JSON serialization according to Protocol Design Principles.
/// This ensures:
/// - Object keys are sorted lexicographically.
/// - Whitespace is stripped.
/// - Floating point representations are strict (though preferably avoided in digests).
pub fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    // A robust canonical JSON implementation would iterate through a `serde_json::Value`
    // to sort keys of objects, then serialize to bytes without spaces.
    let value = serde_json::to_value(value)?;
    let canonical_value = sort_value_keys(value);

    let mut buf = Vec::new();
    let mut serializer = Serializer::with_formatter(&mut buf, CanonicalFormatter);
    canonical_value.serialize(&mut serializer)?;
    Ok(buf)
}

fn sort_value_keys(mut v: serde_json::Value) -> serde_json::Value {
    match &mut v {
        serde_json::Value::Array(arr) => {
            for item in arr {
                *item = sort_value_keys(item.clone());
            }
        }
        serde_json::Value::Object(obj) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<_> = obj.keys().cloned().collect();
            keys.sort_unstable();
            for k in keys {
                let val = obj.remove(&k).unwrap();
                sorted.insert(k, sort_value_keys(val));
            }
            return serde_json::Value::Object(sorted);
        }
        _ => {}
    }
    v
}

#[derive(Clone, Debug)]
struct CanonicalFormatter;

impl Formatter for CanonicalFormatter {
    #[inline]
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b"[")
    }
    #[inline]
    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b"]")
    }
    #[inline]
    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if !first {
            writer.write_all(b",")
        } else {
            Ok(())
        }
    }
    #[inline]
    fn end_array_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        Ok(())
    }
    #[inline]
    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b"{")
    }
    #[inline]
    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b"}")
    }
    #[inline]
    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if !first {
            writer.write_all(b",")
        } else {
            Ok(())
        }
    }
    #[inline]
    fn end_object_key<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        Ok(())
    }
    #[inline]
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b":")
    }
    #[inline]
    fn end_object_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        Ok(())
    }
}
