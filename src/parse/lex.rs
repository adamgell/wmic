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
    tokens_inner(units, false)
}

// The command parser validates every non-GET span with strict `tokens` after
// identifying the grammar. Lexing alone cannot tell a verb from a password value.
pub(super) fn relaxed_quote_tokens(units: &[u16]) -> Result<Vec<Range<usize>>, ParseError> {
    tokens_inner(units, true)
}

pub(super) fn switch_prefix(unit: u16) -> bool {
    matches!(unit, 47 | 45)
}

/// Preserve source ranges while allowing switch separators to touch values,
/// and whitespace on either side of a switch's colon. Quoted data is retained.
pub(super) fn command_tokens(units: &[u16]) -> Result<Vec<Range<usize>>, ParseError> {
    let mut split = raw_command_spans(units, 0)?;
    let mut result = vec![];
    let mut i = 0;
    while i < split.len() {
        let start = split[i].start;
        let is_switch = switch_prefix(units[start]);
        let mut end = split[i].end;
        if is_switch {
            loop {
                let next_colon = i + 1 < split.len() && units[split[i + 1].start] == 58;
                if next_colon {
                    i += 1;
                    end = split[i].end;
                }
                if units[end - 1] == 58
                    && i + 1 < split.len()
                    && !switch_prefix(units[split[i + 1].start])
                {
                    i += 1;
                    end = split[i].end;
                } else if !next_colon {
                    break;
                }
            }
        }
        if is_switch
            && let Some(colon) = units[start..end].iter().position(|&u| u == 58)
            && WideText(units[start + 1..start + colon].to_vec())
                .trim_ascii()
                .eq_cli("node")
        {
            let mut at = start + colon + 1;
            while units.get(at).is_some_and(|&u| space(u)) {
                at += 1;
            }
            if at < units.len() && !switch_prefix(units[at]) {
                let native_end = node_list_end(units, at);
                if native_end != end {
                    result.push(start..native_end);
                    // Re-scan only the affected suffix. Keep this iterative so
                    // repeated NODE switches cannot consume the process stack.
                    split = raw_command_spans(units, native_end)?;
                    i = 0;
                    continue;
                }
            }
        }
        result.push(start..end);
        i += 1;
    }
    Ok(result)
}

fn raw_command_spans(units: &[u16], offset: usize) -> Result<Vec<Range<usize>>, ParseError> {
    let mut split = vec![];
    for span in relaxed_quote_tokens(&units[offset..])? {
        let span = span.start + offset..span.end + offset;
        let mut start = span.start;
        let mut quote = None;
        for i in span.clone() {
            if let Some(q) = quote {
                if units[i] == q && !escaped(units, i) {
                    quote = None;
                }
            } else if matches!(units[i], 34 | 39) {
                quote = Some(units[i]);
            } else if units[i] == 47 && i > start {
                split.push(start..i);
                start = i;
            }
        }
        split.push(start..span.end);
    }
    Ok(split)
}

fn node_list_end(units: &[u16], mut at: usize) -> usize {
    loop {
        let end = native_node_atom_end(units, at);
        at = end;
        while units.get(at).is_some_and(|&u| space(u)) {
            at += 1;
        }
        if units.get(at) != Some(&44) {
            return end;
        }
        at += 1;
        while units.get(at).is_some_and(|&u| space(u)) {
            at += 1;
        }
        if at == units.len() || switch_prefix(units[at]) {
            return at;
        }
    }
}

// Native scanner 0x14000e298: a comma terminates its first quoted scan, even
// before the closing quote. An unmatched quote runs to the end of the input.
fn native_node_atom_end(units: &[u16], mut at: usize) -> usize {
    if units
        .get(at)
        .is_some_and(|u| matches!(u, 40 | 41 | 44 | 45 | 47 | 61))
    {
        return at + 1;
    }
    while at < units.len() {
        match units[at] {
            9 | 32 | 40 | 41 | 44 | 45 | 47 | 61 => break,
            q @ (34 | 39) => {
                at += 1;
                while at < units.len() {
                    if units[at] == 44 {
                        return at;
                    }
                    if units[at] == q && units[at - 1] != 92 {
                        break;
                    }
                    at += 1;
                }
                if at < units.len() {
                    at += 1;
                }
            }
            _ => at += 1,
        }
    }
    at
}

pub(super) fn contains_password(units: &[u16]) -> bool {
    units.iter().enumerate().any(|(at, &u)| {
        if !switch_prefix(u) {
            return false;
        }
        let end = units[at + 1..]
            .iter()
            .position(|&u| space(u) || u == 58)
            .map_or(units.len(), |n| at + 1 + n);
        // Preserve conservative /password* protection even for a malformed
        // suffix/colon. A native-equal prefix can contain ignored characters,
        // so its UTF-16 length is not necessarily eight code units.
        (at + 1..=end).any(|end| WideText(units[at + 1..end].to_vec()).eq_cli("password"))
    })
}

fn tokens_inner(units: &[u16], property_expression: bool) -> Result<Vec<Range<usize>>, ParseError> {
    let mut result: Vec<Range<usize>> = vec![];
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
            } else if matches!(unit, 40 | 91 | 123) {
                depth += 1;
            } else if matches!(unit, 41 | 93 | 125) && depth > 0 {
                depth -= 1;
            } else if space(unit) && depth == 0 {
                break;
            }
            i += 1;
        }
        if (quote.is_some() && !property_expression) || depth != 0 {
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
