// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

pub const MAX_FRACTION_DIGITS: u32 = 9;

#[derive(Debug, PartialEq, Eq)]
pub enum ParseAmountError {
    Empty,
    NotANumber,
    TooPrecise { allowed: u32 },
    TooLarge,
}

impl fmt::Display for ParseAmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "no amount given"),
            Self::NotANumber => write!(f, "not a valid amount"),
            Self::TooPrecise { allowed } => {
                write!(f, "at most {allowed} decimal places are allowed")
            }
            Self::TooLarge => write!(f, "amount is too large"),
        }
    }
}

impl std::error::Error for ParseAmountError {}

pub fn parse_amount(input: &str, digits: u32) -> Result<i64, ParseAmountError> {
    let input = input.trim().strip_prefix('+').unwrap_or(input.trim());
    if input.is_empty() {
        return Err(ParseAmountError::Empty);
    }

    let (whole, fraction) = match input.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (input, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return Err(ParseAmountError::NotANumber);
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseAmountError::NotANumber);
    }
    if fraction.len() > digits as usize {
        return Err(ParseAmountError::TooPrecise { allowed: digits });
    }

    let scale = 10i64
        .checked_pow(digits)
        .ok_or(ParseAmountError::TooLarge)?;
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| ParseAmountError::TooLarge)?
    };

    let padded = format!("{fraction:0<width$}", width = digits as usize);
    let minor: i64 = if padded.is_empty() {
        0
    } else {
        padded.parse().map_err(|_| ParseAmountError::TooLarge)?
    };

    whole
        .checked_mul(scale)
        .and_then(|major| major.checked_add(minor))
        .ok_or(ParseAmountError::TooLarge)
}

#[must_use]
pub fn format_amount(amount: i64, digits: u32) -> String {
    let scale = 10i128.pow(digits.min(MAX_FRACTION_DIGITS));
    let amount = i128::from(amount);
    let sign = if amount < 0 { "-" } else { "" };
    let magnitude = amount.abs();
    if digits == 0 {
        return format!("{sign}{magnitude}");
    }
    format!(
        "{sign}{}.{:0>width$}",
        magnitude / scale,
        magnitude % scale,
        width = digits as usize
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_shapes() {
        assert_eq!(parse_amount("12", 2), Ok(1200));
        assert_eq!(parse_amount("12.5", 2), Ok(1250));
        assert_eq!(parse_amount("12.05", 2), Ok(1205));
        assert_eq!(parse_amount(" 7 ", 2), Ok(700));
        assert_eq!(parse_amount(".5", 2), Ok(50));
        assert_eq!(parse_amount("12", 0), Ok(12));
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(parse_amount("", 2), Err(ParseAmountError::Empty));
        assert_eq!(parse_amount("-5", 2), Err(ParseAmountError::NotANumber));
        assert_eq!(parse_amount("1e5", 2), Err(ParseAmountError::NotANumber));
        assert_eq!(
            parse_amount("1.234", 2),
            Err(ParseAmountError::TooPrecise { allowed: 2 })
        );
        assert_eq!(
            parse_amount("99999999999999999999", 2),
            Err(ParseAmountError::TooLarge)
        );
    }

    #[test]
    fn formats_with_padding() {
        assert_eq!(format_amount(1205, 2), "12.05");
        assert_eq!(format_amount(5, 2), "0.05");
        assert_eq!(format_amount(-1250, 2), "-12.50");
        assert_eq!(format_amount(12, 0), "12");
        assert_eq!(format_amount(i64::MIN, 2), "-92233720368547758.08");
    }

    #[test]
    fn round_trips() {
        for amount in [0i64, 1, 99, 100, 123_456_789] {
            let text = format_amount(amount, 2);
            assert_eq!(parse_amount(&text, 2), Ok(amount));
        }
    }
}
