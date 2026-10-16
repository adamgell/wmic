use super::lex::unquote;
use crate::model::{Modifier, ModifierScope, ParseError, SwitchValue, Verb, WideText};

pub(super) fn modifier(
    units: &[u16],
    scope: ModifierScope,
    verb: Verb,
) -> Result<Modifier, ParseError> {
    let colon = units.iter().position(|u| *u == b':' as u16);
    let name = WideText(units[1..colon.unwrap_or(units.len())].to_vec());
    let allowed: &[&str] = match scope {
        ModifierScope::Global => &[
            "namespace",
            "role",
            "node",
            "implevel",
            "authlevel",
            "locale",
            "privileges",
            "trace",
            "record",
            "interactive",
            "failfast",
            "user",
            "password",
            "output",
            "append",
            "aggregate",
            "authority",
            "?",
        ][..],
        ModifierScope::Verb => match verb {
            Verb::Get => &[
                "value",
                "all",
                "translate",
                "every",
                "repeat",
                "format",
                "?",
            ],
            Verb::List => &["translate", "every", "repeat", "format", "?"],
            Verb::Assoc => &[
                "resultclass",
                "resultrole",
                "assocclass",
                "format",
                "translate",
                "?",
            ],
            Verb::Delete => &["interactive", "nointeractive", "?"],
            _ => &["?"],
        },
    };
    if !allowed.iter().any(|word| name.eq_ascii_ignore_case(word)) {
        let (code, message) = if scope == ModifierScope::Global {
            (0xAC5D, "Invalid Global Switch.")
        } else if verb == Verb::Get {
            (0xACB4, "Invalid GET switch.")
        } else {
            (0xAC5C, "Unexpected switch at this level.")
        };
        return Err(ParseError::new(code, message, Some(name)));
    }
    let value = match colon {
        None => SwitchValue::Missing,
        Some(i) => {
            let value = unquote(&units[i + 1..]);
            if value.0.is_empty() {
                SwitchValue::Reset
            } else {
                SwitchValue::Value(value)
            }
        }
    };
    Ok(Modifier {
        name,
        value,
        scope,
        raw_value: colon.map(|i| WideText(units[i + 1..].to_vec())),
    })
}
