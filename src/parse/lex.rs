use crate::model::{ParseError, WideText};
use std::ops::Range;

pub(super) fn space(unit: u16) -> bool {
    matches!(unit, 9 | 10 | 13 | 32)
}

fn escaped(units: &[u16], at: usize) -> bool {
    let mut slashes = 0;
    let mut i = at;
    while i > 0 && units[i - 1] == b'\\' as u16 {
        slashes += 1;
        i -= 1;
    }
    slashes % 2 == 1
}

pub(super) fn unquote(units: &[u16]) -> WideText {
    if units.len() >= 2 && matches!(units[0], 34 | 39) {
        let quote = units[0];
        for i in 1..units.len() {
            if units[i] == quote && !escaped(units, i) {
                if i == units.len() - 1 {
                    return WideText(units[1..i].to_vec());
                }
                break;
            }
        }
    }
    WideText(units.to_vec())
}

pub(super) fn tokens(units: &[u16]) -> Result<Vec<Range<usize>>, ParseError> {
    let mut result = vec![];
    let mut i = 0;
    while i < units.len() {
        while i < units.len() && space(units[i]) {
            i += 1;
        }
        if i == units.len() {
            break;
        }
        let start = i;
        let mut quote = None;
        let mut depth = 0usize;
        while i < units.len() {
            let unit = units[i];
            if let Some(q) = quote {
                if unit == q && !escaped(units, i) {
                    quote = None;
                }
            } else if matches!(unit, 34 | 39) && !escaped(units, i) {
                quote = Some(unit);
            } else if unit == b'(' as u16 {
                depth += 1;
            } else if unit == b')' as u16 && depth > 0 {
                depth -= 1;
            } else if space(unit) && depth == 0 {
                break;
            }
            i += 1;
        }
        if quote.is_some() || depth != 0 {
            return Err(ParseError::new(
                0xABFD,
                "Invalid alias verb.",
                Some(WideText(units[start..i].to_vec())),
            ));
        }
        result.push(start..i);
    }
    Ok(result)
}

pub(super) fn split_commas(units: &[u16], strip_quotes: bool) -> Vec<WideText> {
    if units.iter().all(|u| space(*u)) {
        return vec![];
    }
    let mut result = vec![];
    let mut start = 0;
    let mut quote = None;
    let mut depth = 0usize;
    for (i, &unit) in units.iter().enumerate() {
        if let Some(q) = quote {
            if unit == q && !escaped(units, i) {
                quote = None;
            }
        } else if matches!(unit, 34 | 39) && !escaped(units, i) {
            quote = Some(unit);
        } else if matches!(unit, 40 | 91 | 123) {
            depth += 1;
        } else if matches!(unit, 41 | 93 | 125) && depth > 0 {
            depth -= 1;
        } else if unit == b',' as u16 && depth == 0 {
            result.push(segment(&units[start..i], strip_quotes));
            start = i + 1;
        }
    }
    result.push(segment(&units[start..], strip_quotes));
    result
}

fn segment(units: &[u16], strip_quotes: bool) -> WideText {
    let trimmed = WideText(units.to_vec()).trim_ascii();
    if strip_quotes {
        unquote(&trimmed.0)
    } else {
        trimmed
    }
}
