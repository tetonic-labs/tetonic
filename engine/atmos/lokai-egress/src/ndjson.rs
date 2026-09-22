//! Incremental framing only. Model semantics and completion validation belong
//! to the inference adapter; transport authorization remains in EgressGuard.
use crate::EgressError;
use serde_json::Value;

#[derive(Default)]
pub(crate) struct Decoder {
    buffer: Vec<u8>,
    consumed: usize,
    scanned: usize,
}

impl Decoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        // Compact once per network chunk, rather than shifting the entire suffix
        // for every record. The remaining bytes contain only an unfinished line.
        if self.consumed > 0 {
            self.buffer.drain(..self.consumed);
            self.scanned -= self.consumed;
            self.consumed = 0;
        }
        self.buffer.extend_from_slice(bytes);
    }

    pub(crate) fn next(&mut self) -> Option<Result<Value, EgressError>> {
        loop {
            let Some(offset) = self.buffer[self.scanned..].iter().position(|&b| b == b'\n') else {
                self.scanned = self.buffer.len();
                return None;
            };
            let end = self.scanned + offset;
            let start = self.consumed;
            self.consumed = end + 1;
            self.scanned = self.consumed;
            if start == end {
                continue;
            }
            return Some(
                serde_json::from_slice(&self.buffer[start..end])
                    .map_err(|e| EgressError::StreamDecode(e.to_string())),
            );
        }
    }

    pub(crate) fn finish(self) -> Option<Result<Value, EgressError>> {
        let trailing = &self.buffer[self.consumed..];
        if trailing.is_empty() {
            return None;
        }
        Some(serde_json::from_slice(trailing).map_err(|e| {
            EgressError::StreamDecode(format!(
                "truncated trailing NDJSON record ({} bytes): {}",
                trailing.len(),
                e
            ))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(chunks: &[&[u8]]) -> Result<Vec<Value>, EgressError> {
        let mut decoder = Decoder::default();
        let mut values = Vec::new();
        for chunk in chunks {
            decoder.push(chunk);
            while let Some(value) = decoder.next() {
                values.push(value?);
            }
        }
        if let Some(value) = decoder.finish() {
            values.push(value?);
        }
        Ok(values)
    }

    #[test]
    fn every_split_preserves_records_utf8_crlf_and_final_unterminated_record() {
        let input = "\n{\"text\":\"hello 世界\\nline\"}\r\n\n[1,2]\n{\"done\":true}".as_bytes();
        let expected = decode(&[input]).unwrap();
        assert_eq!(expected.len(), 3);
        for split in 0..=input.len() {
            assert_eq!(
                decode(&[&input[..split], &input[split..]]).unwrap(),
                expected
            );
        }
        assert_eq!(
            decode(&input.chunks(1).collect::<Vec<_>>()).unwrap(),
            expected
        );
    }

    #[test]
    fn malformed_records_fail_without_accepting_later_records() {
        let mut decoder = Decoder::default();
        decoder.push(b"1\ninvalid\n2\n");
        assert_eq!(decoder.next().unwrap().unwrap(), 1);
        assert!(decoder.next().unwrap().is_err());
        assert!(decode(&[b"{\"unfinished\":"]).is_err());
        assert!(decode(&[b" \n"]).is_err());
        assert_eq!(decode(&[b"\n\n"]).unwrap(), Vec::<Value>::new());
    }

    #[test]
    #[ignore = "isolated transport framing benchmark, not task speed"]
    fn benchmark_coalesced_records() {
        let record = format!("{{\"text\":\"{}\"}}\n", "a".repeat(300));
        let input = record.repeat(8000).into_bytes();
        let started = std::time::Instant::now();
        let mut buffer = input.clone();
        let mut old = Vec::new();
        while let Some(end) = buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            old.push(serde_json::from_slice::<Value>(&line[..line.len() - 1]).unwrap());
        }
        let old_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        let new = decode(&[&input]).unwrap();
        let new_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(old, new);
        println!("NDJSON 8000 coalesced records: old={old_ms:.2}ms new={new_ms:.2}ms ratio={:.2}x; framing only", old_ms/new_ms);
    }
}
