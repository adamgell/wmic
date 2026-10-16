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
    .find_map(|(name, value)| word.eq_ascii_ignore_case(name).then_some(value))
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
    let units = command_units(raw, mode)?;
    let spans = lex::tokens(units)?;
    let mut command = ParsedCommand::empty(raw.clone());
    let mut i = 0;
    while i < spans.len() && units[spans[i].start] == b'/' as u16 {
        let switch = modifier(
            &units[spans[i].clone()],
            ModifierScope::Global,
            Verb::Interactive,
        )?;
        command.help |= switch.name.eq_ascii_ignore_case("?");
        command.modifiers.push(switch);
        i += 1;
    }
    if i == spans.len() {
        if command.help {
            command.verb = Verb::Help;
        }
        return Ok(command);
    }
    let first = unquote(&units[spans[i].clone()]);
    if first.eq_ascii_ignore_case("context")
        || first.eq_ascii_ignore_case("quit")
        || first.eq_ascii_ignore_case("exit")
    {
        command.verb = if first.eq_ascii_ignore_case("context") {
            Verb::Context
        } else {
            Verb::Exit
        };
        command.verb_explicit = true;
        i += 1;
    } else {
        if first.eq_ascii_ignore_case("path") || first.eq_ascii_ignore_case("class") {
            let class = first.eq_ascii_ignore_case("class");
            i += 1;
            let name = if i < spans.len() && units[spans[i].start] != b'/' as u16 {
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
            if word.eq_ascii_ignore_case("where") {
                i += 1;
                if i == spans.len() {
                    return Err(ParseError::new(0xABFD, "Invalid alias verb.", None));
                }
                command.selector = Some(unquote(&units[spans[i].clone()]));
                i += 1;
                if i < spans.len() {
                    let next = unquote(&units[spans[i].clone()]);
                    if verb(&next).is_none() && units[spans[i].start] != b'/' as u16 {
                        return Err(ParseError::new(0xABFD, "Invalid alias verb.", Some(next)));
                    }
                }
            } else if let Some(value) = verb(&word) {
                command.verb = value;
                command.verb_explicit = true;
                i += 1;
                break;
            } else if units[spans[i].start] == b'/' as u16 {
                break;
            } else {
                command.positionals.push(word);
                i += 1;
            }
        }
    }
    let mut body_end = i;
    while body_end < spans.len() && units[spans[body_end].start] != b'/' as u16 {
        body_end += 1;
    }
    let body = if i < body_end {
        &units[spans[i].start..spans[body_end - 1].end]
    } else {
        &[]
    };
    match command.verb {
        Verb::Get => {
            command.properties = lex::split_commas(body, false);
            for property in &command.properties {
                if property.0.is_empty() || lex::tokens(&property.0)?.len() != 1 {
                    return Err(ParseError::new(0xACBC, "Invalid GET Expression.", None));
                }
            }
        }
        Verb::List => {
            if !body.is_empty() {
                command.view = Some(unquote(body));
            }
        }
        Verb::Call => {
            if i < body_end {
                command.method = Some(unquote(&units[spans[i].clone()]));
                if i + 1 < body_end {
                    command.arguments = lex::split_commas(
                        &units[spans[i + 1].start..spans[body_end - 1].end],
                        true,
                    );
                }
            }
        }
        Verb::Create | Verb::Set => command.arguments = lex::split_commas(body, false),
        _ => {}
    }
    i = body_end;
    while i < spans.len() {
        let token = &units[spans[i].clone()];
        if token.first() != Some(&(b'/' as u16)) {
            return Err(ParseError::new(
                0xABFD,
                "Invalid alias verb.",
                Some(unquote(token)),
            ));
        }
        let switch = match modifier(token, ModifierScope::Verb, command.verb) {
            Ok(value) => value,
            Err(mut error) => {
                if !command.verb_explicit && !token.starts_with(&[47, 63]) {
                    error.code = 0xAC5C;
                    error.message = "Unexpected switch at this level.".into();
                }
                return Err(error);
            }
        };
        command.help |= switch.name.eq_ascii_ignore_case("?");
        command.modifiers.push(switch);
        i += 1;
    }
    Ok(command)
}
