//! Decoding of string literals into the bytes they denote.

/// Decodes a complete string literal, delimiters included.
///
/// Short strings have their escapes decoded; long strings are taken verbatim
/// minus a single leading line ending and with each line ending read as
/// `\n`, per Lua.
pub fn decode_literal(raw: &str) -> Vec<u8> {
    let bytes = raw.as_bytes();
    if bytes.first() == Some(&b'[') {
        return decode_long(bytes);
    }
    let inner = match bytes {
        [_, inner @ .., _] => inner,
        _ => bytes,
    };
    decode_escapes(inner)
}

fn decode_long(bytes: &[u8]) -> Vec<u8> {
    let level = bytes[1..].iter().take_while(|&&b| b == b'=').count();
    let open = level + 2;
    let start = open + line_ending_len(bytes.get(open..).unwrap_or_default());
    let end = bytes.len().saturating_sub(level + 2);
    let body = bytes.get(start..end).unwrap_or_default();
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        match line_ending_len(&body[i..]) {
            0 => {
                out.push(body[i]);
                i += 1;
            }
            len => {
                out.push(b'\n');
                i += len;
            }
        }
    }
    out
}

/// The length of the `\n`, `\r`, `\r\n` or `\n\r` that `bytes` starts with,
/// or 0.
fn line_ending_len(bytes: &[u8]) -> usize {
    match bytes {
        [b'\r', b'\n', ..] | [b'\n', b'\r', ..] => 2,
        [b'\r' | b'\n', ..] => 1,
        _ => 0,
    }
}

/// Decodes escape sequences in a literal body with its delimiters removed.
///
/// Malformed escapes are decoded leniently rather than rejected.
pub fn decode_escapes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let Some(&c) = bytes.get(i) else { break };
        i += 1;
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'v' => out.push(0x0b),
            b'\n' | b'\r' => {
                i += line_ending_len(&bytes[i - 1..]) - 1;
                out.push(b'\n');
            }
            b'x' => {
                let (value, used) = digits(&bytes[i..], 16, 2);
                i += used;
                out.push(value as u8);
            }
            b'z' => {
                while bytes
                    .get(i)
                    .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c))
                {
                    i += 1;
                }
            }
            b'u' if bytes.get(i) == Some(&b'{') => {
                let (value, used) = digits(&bytes[i + 1..], 16, usize::MAX);
                i += 1 + used;
                if bytes.get(i) == Some(&b'}') {
                    i += 1;
                }
                encode_utf8_extended(value, &mut out);
            }
            b'0'..=b'9' => {
                let (value, used) = digits(&bytes[i - 1..], 10, 3);
                i += used - 1;
                out.push(value as u8);
            }
            other => out.push(other),
        }
    }
    out
}

/// Reads up to `max` digits in `radix`, returning the value and digit count.
fn digits(bytes: &[u8], radix: u32, max: usize) -> (u32, usize) {
    let mut value = 0u32;
    let mut used = 0;
    for &byte in bytes.iter().take(max) {
        let Some(digit) = (byte as char).to_digit(radix) else {
            break;
        };
        value = value.saturating_mul(radix).saturating_add(digit);
        used += 1;
    }
    (value, used)
}

/// Encodes `value` the way Lua 5.4 does, which extends UTF-8 to 31 bits and
/// permits surrogates.
fn encode_utf8_extended(value: u32, out: &mut Vec<u8>) {
    let value = value.min(0x7FFF_FFFF);
    if value < 0x80 {
        out.push(value as u8);
        return;
    }
    let mut buffer = [0u8; 6];
    let mut len = 0;
    let mut rest = value;
    let mut limit = 0x3f;
    while rest > limit {
        buffer[5 - len] = 0x80 | (rest & 0x3f) as u8;
        rest >>= 6;
        limit >>= 1;
        len += 1;
    }
    buffer[5 - len] = ((!limit << 1) | rest) as u8;
    out.extend_from_slice(&buffer[5 - len..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_escapes() {
        assert_eq!(decode_literal(r#""a\n\t\\\"""#), b"a\n\t\\\"");
        assert_eq!(decode_literal(r"'\65\x41\u{41}'"), b"AAA");
    }

    #[test]
    fn escaped_line_endings_become_newline() {
        assert_eq!(decode_escapes(b"a\\\r\nb\\\rc"), b"a\nb\nc");
    }

    #[test]
    fn unicode_escapes() {
        assert_eq!(decode_escapes(br"\u{E9}"), "é".as_bytes());
        assert_eq!(decode_escapes(br"\u{1F600}"), "😀".as_bytes());
        assert_eq!(decode_escapes(br"\u{D800}"), [0xED, 0xA0, 0x80]);
        assert_eq!(
            decode_escapes(br"\u{7FFFFFFF}"),
            [0xFD, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF]
        );
    }

    #[test]
    fn oversized_unicode_escape_does_not_panic() {
        decode_escapes(br"\u{FFFFFFFFFFFFFFFF}");
    }

    #[test]
    fn long_strings() {
        assert_eq!(decode_literal("[[\nabc]]"), b"abc");
        assert_eq!(decode_literal("[==[\r\na]]b]==]"), b"a]]b");
        assert_eq!(
            decode_literal("[[a\r\nb\rc\n\rd\n\ne]]"),
            b"a\nb\nc\nd\n\ne"
        );
        assert_eq!(decode_literal("[[\r\n\r\nx]]"), b"\nx");
        assert_eq!(decode_literal("[[]]"), b"");
    }

    #[test]
    fn z_skips_whitespace() {
        assert_eq!(decode_escapes(b"a\\z \n\t b"), b"ab");
    }
}
