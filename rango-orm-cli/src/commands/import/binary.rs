//! Binary and UUID value encodings shared by the importers.

use anyhow::{Context, Result};

/// Bytes of a binary column value in a Rango JSON file: base64, or the `\x` + hex form
/// that dumps of version 1 and `to_jsonb` use (`\` is not a base64 character).
pub(crate) fn decode_binary(column: &str, s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    match s.strip_prefix("\\x") {
        Some(hex) => hex_to_bytes(hex),
        None => base64::engine::general_purpose::STANDARD.decode(s).ok(),
    }
    .with_context(|| format!("Column '{column}' holds a binary value that is not base64"))
}

pub(crate) fn encode_binary(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(super) fn uuid_text_to_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() == 36 {
        let bytes = s.as_bytes();
        if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
            return None;
        }
        let hex = s.replace('-', "");
        hex_to_bytes(&hex)
    } else if s.len() == 32 {
        hex_to_bytes(s)
    } else {
        None
    }
}

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.chars();

    while let Some(hi) = chars.next() {
        let lo = chars.next()?;
        out.push(hex_pair_to_byte(hi, lo)?);
    }

    Some(out)
}

pub(super) fn hex_pair_to_byte(hi: char, lo: char) -> Option<u8> {
    Some(hex_digit(hi)? * 16 + hex_digit(lo)?)
}

pub(super) fn hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_text_to_bytes_parses_hyphenated_uuid() {
        let bytes = uuid_text_to_bytes("123e4567-e89b-12d3-a456-426614174000").unwrap();
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[15], 0x00);
    }

    #[test]
    fn binary_values_read_base64_and_v1_hex() {
        assert_eq!(decode_binary("c", "AAEC/w==").unwrap(), [0, 1, 2, 255]);
        assert_eq!(decode_binary("c", "\\x000102ff").unwrap(), [0, 1, 2, 255]);
        assert_eq!(encode_binary(&[0, 1, 2, 255]), "AAEC/w==");
        assert!(decode_binary("c", "not base64!").is_err());
    }

    #[test]
    fn uuid_text_to_bytes_parses_32_hex_uuid() {
        let bytes = uuid_text_to_bytes("123e4567e89b12d3a456426614174000").unwrap();
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[15], 0x00);
    }

    #[test]
    fn uuid_text_to_bytes_rejects_invalid_uuids() {
        assert!(uuid_text_to_bytes("123e4567e89b12d3a45642661417400").is_none());
        assert!(uuid_text_to_bytes("123e4567-e89b-12d3-a456-42661417400g").is_none());
        assert!(uuid_text_to_bytes("123e4567-e89b-12d3-a456-4266141740000").is_none());
        assert!(uuid_text_to_bytes("123e4567e89b12d3a4564266141740000").is_none());
    }
}
