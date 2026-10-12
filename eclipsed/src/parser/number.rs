//! Numeric literal values, including Lua 5.2+ hexadecimal floats.

use crate::lexer::Span;
use crate::parser::{ParseError, Parser};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    pub(crate) fn parse_float_value(&self, text: &str, span: Span) -> Result<f64, ParseError> {
        parse_float(text).ok_or_else(|| ParseError::Expected {
            expected: "valid float",
            got: text.to_string(),
            span,
        })
    }
}

/// An unsigned integer literal (decimal, `0x` or `0b`), ignoring `_`.
#[cfg(feature = "luajit")]
pub(crate) fn parse_uint(text: &str) -> Option<u64> {
    let s = text.replace('_', "");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u64::from_str_radix(bin, 2).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// A decimal or hexadecimal float literal.
pub(crate) fn parse_float(text: &str) -> Option<f64> {
    let s = text.replace('_', "");
    if s.starts_with("0x") || s.starts_with("0X") {
        parse_hex_float(&s)
    } else {
        s.parse::<f64>().ok()
    }
}

/// A C99 hexadecimal float: `0x` H* (`.` H*)? (`p` [+-]? D+)?, correctly
/// rounded.
fn parse_hex_float(text: &str) -> Option<f64> {
    let body = text.get(2..)?;
    let (mantissa, mut exponent) = match body.split_once(['p', 'P']) {
        Some((m, e)) => (m, parse_exponent(e)?),
        None => (body, 0),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }

    let mut significand = 0u64;
    let mut digits = 0;
    let mut sticky = false;
    let int_digits = int_part.chars().map(|c| (c, false));
    let frac_digits = frac_part.chars().map(|c| (c, true));
    for (c, is_fraction) in int_digits.chain(frac_digits) {
        let digit = u64::from(c.to_digit(16)?);
        if digits < 16 {
            significand = significand << 4 | digit;
            if significand != 0 {
                digits += 1;
            }
            if is_fraction {
                exponent = exponent.saturating_sub(4);
            }
        } else {
            sticky |= digit != 0;
            if !is_fraction {
                exponent = exponent.saturating_add(4);
            }
        }
    }
    Some(compose(significand, sticky, exponent))
}

/// A decimal exponent, saturating rather than overflowing.
fn parse_exponent(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value = 0i64;
    for c in digits.chars() {
        let digit = i64::from(c.to_digit(10)?);
        value = value.saturating_mul(10).saturating_add(digit);
    }
    Some(if negative { -value } else { value })
}

/// `significand * 2^exponent` rounded to nearest, ties to even, where
/// `sticky` stands for non-zero bits below `significand`.
fn compose(significand: u64, sticky: bool, exponent: i64) -> f64 {
    if significand == 0 {
        return 0.0;
    }
    let leading = significand.leading_zeros();
    let significand = u128::from(significand << leading);
    let top = exponent.saturating_add(63 - i64::from(leading));
    if top > 1023 {
        return f64::INFINITY;
    }
    if top < -1075 {
        return 0.0;
    }
    // 53 bits for normal numbers, fewer as subnormals approach 2^-1074.
    let kept = (top + 1075).min(53) as u32;
    let dropped = 64 - kept;
    let mut rounded = (significand >> dropped) as u64;
    let rest = significand & ((1 << dropped) - 1);
    let half = 1 << (dropped - 1);
    if rest > half || (rest == half && (sticky || rounded & 1 == 1)) {
        rounded += 1;
    }
    rounded as f64 * power_of_two(top - i64::from(kept) + 1)
}

/// `2^exponent` for `exponent` in `-1074..=1023`.
fn power_of_two(exponent: i64) -> f64 {
    if exponent >= -1022 {
        f64::from_bits(((exponent + 1023) as u64) << 52)
    } else {
        f64::from_bits(1 << (exponent + 1074))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_float;

    #[test]
    fn hex_floats_are_correctly_rounded() {
        let ones = format!("0x{}p-1200", "1".repeat(300));
        let cases: &[(&str, f64)] = &[
            ("0x1p4", 16.0),
            ("0x.8", 0.5),
            ("0xA.8p1", 21.0),
            ("0x1p-1074", 5e-324),
            ("0x1p-1075", 0.0),
            ("0x1.8p-1075", 5e-324),
            ("0x1p-1060", 8.095e-320),
            ("0x1.0000000000000801p0", 1.0000000000000002),
            ("0x1.00000000000008p0", 1.0),
            ("0x1.00000000000018p0", 1.0000000000000004),
            ("0x0.000000000000001p1030", 2f64.powi(970)),
            ("0x1.fffffffffffff8p1023", f64::INFINITY),
            ("0x1p99999999999", f64::INFINITY),
            ("0x1p-99999999999", 0.0),
            (&ones, 0.06666666666666667),
        ];
        for &(text, expected) in cases {
            assert_eq!(parse_float(text), Some(expected), "{text}");
        }
        assert_eq!(parse_float("0x1p"), None);
        assert_eq!(parse_float("0x.p1"), None);
    }
}
