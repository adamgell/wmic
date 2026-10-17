use super::{
    lex::{self, unquote},
    switches::modifier,
};
use crate::model::*;

fn verb(word: &WideText) -> Option<Verb> {
    [
        ("get", Verb::Get),
        ("list", Verb::List),
        ("assoc", Verb::Assoc),
        ("call", Verb::Call),
        ("create", Verb::Create),
        ("delete", Verb::Delete),
        ("set", Verb::Set),
    ]
    .into_iter()
    .find_map(|(name, value)| word.eq_cli(name).then_some(value))
}

fn command_units(raw: &WideText, mode: EntryMode) -> Result<&[u16], ParseError> {
    if raw.0.contains(&0) {
        return Err(ParseError::new(0xABFD, "Invalid command.", None));
    }
    if mode == EntryMode::Interactive {
        return Ok(&raw.0);
    }
    let mut at = 0;
    while at < raw.0.len() && lex::space(raw.0[at]) {
        at += 1;
    }
    if raw.0.get(at) == Some(&(b'"' as u16)) {
        at += 1;
        while at < raw.0.len() && raw.0[at] != b'"' as u16 {
            at += 1;
        }
        if at == raw.0.len() {
            return Err(ParseError::new(0xABFD, "Invalid command.", None));
        }
        at += 1;
    } else {
        while at < raw.0.len() && !lex::space(raw.0[at]) {
            at += 1;
        }
    }
    Ok(&raw.0[at..])
}

pub fn parse_command(raw: &WideText, mode: EntryMode) -> Result<ParsedCommand, ParseError> {
    parse_command_inner(raw, mode).map_err(|mut error| {
        if lex::contains_password(&raw.0) {
            // Lexing can fail before a credential switch has been recognized.
            // No error token from such an input is safe to echo or record.
            error.token = None;
        }
        error
    })
}
/// Parse raw UTF-16 grammar while retaining CALL quotes for later type conversion.
fn parse_command_inner(raw: &WideText, mode: EntryMode) -> Result<ParsedCommand, ParseError> {
    let units = command_units(raw, mode)?;
    let spans = lex::command_tokens(units)?;
    let mut command = ParsedCommand::empty(raw.clone());
    let mut i = 0;
    let mut redactions = vec![];
    let mut protected_credentials = vec![];
    while i < spans.len() && lex::switch_prefix(units[spans[i].start]) {
        let switch_start = spans[i].start;
        let mut switch = modifier(
            &units[spans[i].clone()],
            ModifierScope::Global,
            Verb::Interactive,
        )?;
        if switch.value == SwitchValue::Reset
            && switch.raw_value.as_ref().is_some_and(|v| v.0.is_empty())
            && i + 1 < spans.len()
            && !lex::switch_prefix(units[spans[i + 1].start])
        {
            i += 1;
            switch.value = SwitchValue::Value(unquote(&units[spans[i].clone()]));
        }
        if switch.name.eq_cli("password")
            && let Some(colon) = units[switch_start..spans[i].end]
                .iter()
                .position(|u| *u == 58)
        {
            redactions.push(switch_start + colon + 1..spans[i].end);
            protected_credentials.push(switch_start..spans[i].end);
        }
        command.help |= switch.name.eq_cli("?");
        command.modifiers.push(switch);
        i += 1;
    }
    let mut presented = units.to_vec();
    let global_end = i;
    let native_node = command.modifiers.iter().any(|m| m.name.eq_cli("node"));
    let mut unprotected = units.to_vec();
    for span in protected_credentials {
        unprotected[span].fill(32);
    }
    let ambiguous_credentials = lex::contains_password(&unprotected);
    if spans[..global_end]
        .iter()
        .zip(&command.modifiers)
        .any(|(span, m)| {
            m.name.eq_cli("node") && lex::contains_password(&unprotected[span.clone()])
        })
    {
        // Do not persist a credential-looking switch swallowed by NODE's
        // permissive scanner into the node list or later CONTEXT/RECORD output.
        return Err(ParseError::new(0xABFD, "Invalid command.", None));
    }
    for (span, switch) in spans[..i].iter().zip(&command.modifiers) {
        if !switch.name.eq_cli("node") {
            lex::tokens(&units[span.clone()])?;
        }
    }
    for span in redactions.into_iter().rev() {
        presented.splice(span, "<redacted>".encode_utf16());
    }
    command.command_line = if ambiguous_credentials {
        "<command; credentials redacted>".into()
    } else {
        WideText(presented)
    };
    if i == spans.len() {
        if command.help {
            command.verb = Verb::Help;
        }
        return Ok(command);
    }
    let first = unquote(&units[spans[i].clone()]);
    if first.eq_cli("context") || first.eq_cli("quit") || first.eq_cli("exit") {
        command.verb = if first.eq_cli("context") {
            Verb::Context
        } else {
            Verb::Exit
        };
        command.verb_explicit = true;
        i += 1;
    } else {
        if first.eq_cli("path") || first.eq_cli("class") {
            let class = first.eq_cli("class");
            i += 1;
            let name = if i < spans.len() && !lex::switch_prefix(units[spans[i].start]) {
                let value = unquote(&units[spans[i].clone()]);
                i += 1;
                value
            } else {
                WideText::default()
            };
            command.target = Some(if class {
                Target::Class(name)
            } else {
                Target::Path(name)
            });
        } else {
            command.target = Some(Target::Alias(first));
            i += 1;
        }
        command.verb = Verb::List;
        while i < spans.len() {
            let word = unquote(&units[spans[i].clone()]);
            if word.eq_cli("where") {
                i += 1;
                if i == spans.len() {
                    return Err(ParseError::new(0xACAA, "Invalid Query.", None));
                }
                command.selector = Some(unquote(&units[spans[i].clone()]));
                i += 1;
                if i < spans.len() {
                    let next = unquote(&units[spans[i].clone()]);
                    if verb(&next).is_none() && !lex::switch_prefix(units[spans[i].start]) {
                        return Err(ParseError::new(0xABFD, "Invalid alias verb.", Some(next)));
                    }
                }
            } else if let Some(value) = verb(&word) {
                command.verb = value;
                command.verb_explicit = true;
                i += 1;
                break;
            } else if lex::switch_prefix(units[spans[i].start]) {
                break;
            } else {
                if units[spans[i].start] == 92 {
                    return Err(ParseError::new(
                        0xAC5C,
                        "Unexpected switch at this level.",
                        None,
                    ));
                }
                command.positionals.push(word);
                i += 1;
            }
        }
    }
    let mut body_end = i;
    while body_end < spans.len() && !lex::switch_prefix(units[spans[body_end].start]) {
        body_end += 1;
    }
    let body = if i < body_end {
        &units[spans[i].start..spans[body_end - 1].end]
    } else {
        &[]
    };
    // Only GET property expressions are forwarded with unfinished quotes.
    // Validate global values, targets, selectors and verb switches strictly.
    for (index, span) in spans.iter().enumerate() {
        if index < global_end
            && command
                .modifiers
                .get(index)
                .is_some_and(|m| m.name.eq_cli("node"))
            || index == global_end
                && native_node
                && matches!(command.target, Some(Target::Alias(_)))
        {
            continue;
        }
        if command.verb != Verb::Get || index < i || index >= body_end {
            lex::tokens(&units[span.clone()])?;
        }
    }
    match command.verb {
        Verb::Context if !body.is_empty() => {
            return Err(ParseError::new(0xBBF1, "Invalid context syntax.", None));
        }
        Verb::Get => {
            if body.last() == Some(&(b',' as u16)) {
                return Err(ParseError::new(0xACAC, "Invalid Expression.", None));
            }
            command.properties = lex::split_commas(body, false)
                .into_iter()
                .map(|p| {
                    let unquoted = unquote(&p.0);
                    // A quoted comma is not a property-list separator in this
                    // representation; retaining it lets WMI reject the expression.
                    if unquoted.0.contains(&44) {
                        p
                    } else {
                        unquoted
                    }
                })
                .collect();
            if matches!(command.target, Some(Target::Class(_))) && !command.properties.is_empty() {
                return Err(ParseError::new(0xACBC, "Invalid GET Expression.", None));
            }
            for property in &command.properties {
                if property.0.is_empty() || lex::relaxed_quote_tokens(&property.0)?.len() != 1 {
                    return Err(ParseError::new(0xACBC, "Invalid GET Expression.", None));
                }
            }
        }
        Verb::List => {
            if !body.is_empty() {
                if body_end > i + 1 {
                    return Err(ParseError::new(0xACB6, "Invalid LIST expression.", None));
                }
                command.view = Some(unquote(body));
            }
        }
        Verb::Assoc if !body.is_empty() => {
            return Err(ParseError::new(0xB3C3, "Invalid ASSOC Syntax.", None));
        }
        Verb::Call => {
            if i < body_end {
                command.method = Some(unquote(&units[spans[i].clone()]));
                if i + 1 < body_end {
                    command.raw_arguments = super::call::arguments(
                        &units[spans[i + 1].start..spans[body_end - 1].end],
                    )?;
                    command.arguments = command
                        .raw_arguments
                        .iter()
                        .map(|arg| unquote(&arg.0))
                        .collect();
                }
            }
        }
        Verb::Create | Verb::Set => command.arguments = lex::split_commas(body, false),
        _ => {}
    }
    if command.verb == Verb::Call
        && command.method.is_none()
        && body_end == spans.len()
        && !command.help
    {
        return Err(ParseError::new(0xABE2, "Incomplete Command.", None));
    }
    i = body_end;
    if command.verb == Verb::Context
        && i < spans.len()
        && (spans.len() != i + 1
            || !["/?", "-?"]
                .iter()
                .any(|help| unquote(&units[spans[i].clone()]).eq_cli(help)))
    {
        return Err(ParseError::new(0xBBF1, "Invalid context syntax.", None));
    }
    let mut verb_help = false;
    while i < spans.len() {
        let token = &units[spans[i].clone()];
        if verb_help {
            return Err(ParseError::new(0xABE5, "Invalid command.", None));
        }
        if !lex::switch_prefix(token[0]) {
            return Err(ParseError::new(
                0xABFD,
                "Invalid alias verb.",
                Some(unquote(token)),
            ));
        }
        let mut switch = match modifier(token, ModifierScope::Verb, command.verb) {
            Ok(value) => value,
            Err(mut error) => {
                if !command.verb_explicit && !token.starts_with(&[47, 63]) {
                    error.code = 0xAC5C;
                    error.message = "Unexpected switch at this level.".into();
                }
                return Err(error);
            }
        };
        if command.verb == Verb::Assoc
            && ["resultclass", "resultrole", "assocclass"]
                .iter()
                .any(|n| switch.name.eq_cli(n))
            && switch.value == SwitchValue::Reset
            && switch.raw_value.as_ref().is_some_and(|v| !v.0.is_empty())
        {
            switch.value = SwitchValue::Value(WideText::default());
        }
        verb_help |= switch.name.eq_cli("?");
        command.help |= verb_help;
        command.modifiers.push(switch);
        i += 1;
    }
    if !command.help {
        for m in &command.modifiers {
            if command.verb == Verb::Assoc
                && ["resultclass", "resultrole", "assocclass"]
                    .iter()
                    .any(|n| m.name.eq_cli(n))
                && matches!(m.value, SwitchValue::Missing | SwitchValue::Reset)
            {
                return Err(ParseError::new(0xB088, "Invalid Assoc Switch.", None));
            }
            if m.name.eq_cli("format")
                && let SwitchValue::Value(v) = &m.value
            {
                super::parse_format_spec(m.raw_value.as_ref().unwrap_or(v))?;
            }
        }
    }
    Ok(command)
}

/// Failed commands still get recorded; never expose a possibly supplied password.
pub fn diagnostic_command_line(raw: &WideText, mode: EntryMode) -> WideText {
    let units = command_units(raw, mode).unwrap_or(&raw.0);
    if lex::contains_password(units) {
        "<invalid command; credentials redacted>".into()
    } else {
        WideText(units.to_vec())
    }
}
