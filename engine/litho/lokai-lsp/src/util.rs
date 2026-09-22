//! Shared LSP helpers (content sync hashing, UTF-16 positions).

/// FNV-1a 64-bit content hash (hex) — matches `lokai-index` incrementality.
pub(crate) fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// UTF-16 code-unit column at a UTF-8 byte index on one line (LSP positions).
pub fn utf16_col_at_byte(line: &str, byte_index: usize) -> u32 {
    let end = if line.is_char_boundary(byte_index) {
        byte_index
    } else {
        line.char_indices()
            .take_while(|(i, _)| *i < byte_index)
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(0)
    };
    line[..end].encode_utf16().count() as u32
}

/// When `character` looks like a byte offset (common from grep/read_file), convert to UTF-16.
pub fn normalize_character(file_text: &str, line: u32, character: u32) -> u32 {
    let line_text = file_text
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("");
    let utf16_len = line_text.encode_utf16().count() as u32;
    let byte_len = line_text.len() as u32;
    if character > utf16_len && character <= byte_len {
        return utf16_col_at_byte(line_text, character as usize);
    }
    character
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ascii_and_multibyte() {
        assert_eq!(utf16_col_at_byte("hello", 3), 3);
        assert_eq!(utf16_col_at_byte("héllo", 1), 1);
        assert_eq!(utf16_col_at_byte("héllo", 2), 2);
        // é is 2 bytes, 1 UTF-16 code unit
        assert_eq!(utf16_col_at_byte("héllo", 3), 2);
    }

    #[test]
    fn normalize_byte_offset_heuristic() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let col = normalize_character(text, 2, 8);
        assert_eq!(col, 8);
        let text2 = "café\n";
        let col2 = normalize_character(text2, 1, 5);
        assert_eq!(col2, 4);
    }
}
