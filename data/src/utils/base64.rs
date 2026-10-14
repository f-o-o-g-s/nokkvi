//! Minimal base64 decoding (RFC 4648), inlined so the workspace doesn't take
//! the `base64` crate as a direct dependency for two small reads: a JWT's
//! payload (URL alphabet, unpadded) and Navidrome's `thumbHash` (standard
//! alphabet, padded).

/// Decode base64 in either alphabet (`+` `/` or `-` `_`), with or without
/// `=` padding. `None` on any other byte.
pub(crate) fn decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for byte in input.bytes() {
        let v: u32 = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a' + 26),
            b'0'..=b'9' => u32::from(byte - b'0' + 52),
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => continue,
            _ => return None,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_standard_alphabet_with_padding() {
        // "?>" encodes to Pz4=, and 0xfb 0xff to +/8= (the two letters the
        // URL alphabet swaps).
        assert_eq!(decode("Pz4=").as_deref(), Some(b"?>".as_slice()));
        assert_eq!(decode("+/8=").as_deref(), Some([0xfb, 0xff].as_slice()));
    }

    #[test]
    fn decodes_the_url_alphabet_without_padding() {
        assert_eq!(decode("-_8").as_deref(), Some([0xfb, 0xff].as_slice()));
        assert_eq!(decode("aGk").as_deref(), Some(b"hi".as_slice()));
    }

    #[test]
    fn rejects_bytes_outside_both_alphabets() {
        assert_eq!(decode("aG k"), None);
        assert_eq!(decode("aG.k"), None);
    }
}
