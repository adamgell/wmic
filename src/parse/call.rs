//! CALL arguments retain lexical quotes until metadata determines their type.
use super::lex;
use crate::model::{ParseError, WideText};

/// Locate an assignment outside quotes, keeping quoted positional equals signs intact.
pub(crate) fn named_argument(raw: &WideText) -> Option<(WideText, WideText)> {
    let units = &raw.0;
    let mut quote = None;
    for (at, &unit) in units.iter().enumerate() {
        // The native CALL atom scanner stops at commas even inside a quoted
        // name, and a backslash immediately before a quote keeps it open.
        if unit == 44 {
            return None;
        }
        if let Some(q) = quote {
            if unit == q && (at == 0 || units[at - 1] != 92) {
                quote = None;
            }
        } else if matches!(unit, 34 | 39) {
            quote = Some(unit);
        } else if unit == 61 {
            let name = WideText(units[..at].to_vec()).trim_ascii();
            return Some((
                lex::unquote(&name.0),
                WideText(units[at + 1..].to_vec()).trim_ascii(),
            ));
        }
    }
    None
}

/// Report malformed positional syntax with the native parameter-list hint.
fn invalid_format() -> ParseError {
    ParseError::new(
        0xB7CE,
        "Invalid format.\r\r\nHint: <paramlist> = <param> [, <paramlist>].",
        None,
    )
}

/// Report an incomplete named assignment without echoing its value.
fn invalid_named() -> ParseError {
    ParseError::new(
        0xADDC,
        "Invalid named parameter list.\r\r\nHint: <named param list> ::= <named param> | <named param> <named param list> where <named param> ::= <param name>=<param value>",
        None,
    )
}

/// Normalize separator whitespace while preserving the quotes of names and values.
pub(super) fn arguments(units: &[u16]) -> Result<Vec<WideText>, ParseError> {
    let spans = lex::tokens(units)?;
    let Some(first) = spans.first() else {
        return Ok(vec![]);
    };
    let first_text = WideText(units[first.clone()].to_vec());
    let named =
        named_argument(&first_text).is_some() || spans.get(1).is_some_and(|s| units[s.start] == 61);
    if !named {
        let values = lex::split_commas(units, false);
        if values.len() > 1 && values.iter().any(|v| named_argument(v).is_some()) {
            return Err(invalid_format());
        }
        return Ok(values);
    }
    let mut at = 0;
    let mut token = 0;
    let mut result = vec![];
    while at < units.len() {
        while units.get(at).is_some_and(|u| lex::space(*u)) {
            at += 1;
        }
        let start = at;
        let mut quote = None;
        while at < units.len() {
            let unit = units[at];
            if let Some(q) = quote {
                if unit == q && (at == 0 || units[at - 1] != 92) {
                    quote = None;
                }
            } else if matches!(unit, 34 | 39) {
                quote = Some(unit);
            } else if matches!(unit, 40 | 41) {
                return Err(invalid_format());
            } else if unit == 44 {
                return Err(invalid_named());
            } else if lex::space(unit) || unit == 61 {
                break;
            }
            at += 1;
        }
        let name = WideText(units[start..at].to_vec()).trim_ascii();
        if name.0.is_empty() {
            return Err(invalid_format());
        }
        while units.get(at).is_some_and(|u| lex::space(*u)) {
            at += 1;
        }
        if units.get(at) != Some(&61) {
            return Err(invalid_named());
        }
        at += 1;
        while units.get(at).is_some_and(|u| lex::space(*u)) {
            at += 1;
        }
        while spans.get(token).is_some_and(|span| span.end <= at) {
            token += 1;
        }
        let Some(span) = spans.get(token) else {
            return Err(invalid_named());
        };
        let value = WideText(units[at..span.end].to_vec());
        if lex::split_commas(&value.0, false).len() != 1
            || named_argument(&value).is_some()
            || spans.get(token + 1).is_some_and(|s| units[s.start] == 61)
        {
            return Err(invalid_named());
        }
        let mut arg = name;
        arg.0.push(61);
        arg.0.extend(&value.0);
        result.push(arg);
        at = span.end;
    }
    Ok(result)
}
