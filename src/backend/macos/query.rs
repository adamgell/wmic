//! Instance selection: object path, WHERE conditions and projection are applied to provider records.
use super::{error, provider::Entry, record::key_number, schema::ClassInfo};
use crate::{
    alias::predicate::Predicate,
    backend::{ResolvedRequest, keyword},
    model::{CimValue, ManagementError, Property, Record, Verb, WideText},
};

/// Property names the command asks for; empty means all of them.
fn projection(request: &ResolvedRequest) -> Vec<String> {
    match request.verb {
        Verb::Get => request
            .properties
            .iter()
            .map(WideText::to_string_lossy)
            .collect(),
        Verb::List => request
            .display
            .iter()
            .map(|(_, source)| source.to_string_lossy())
            .collect(),
        _ => vec![],
    }
}

fn is_system_name(name: &str) -> bool {
    crate::render::system_property_order()
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
}

/// WHERE text of the alias target joined with the command selector, as WQL assembly does.
fn condition(request: &ResolvedRequest) -> Option<WideText> {
    let target = request.query.as_ref().and_then(|q| {
        keyword(&q.0, "where").map(|at| WideText(q.0[at + 5..].to_vec()).trim_ascii())
    });
    match (target, &request.selector) {
        (Some(mut target), Some(selector)) => {
            target.0.extend(" AND ".encode_utf16());
            target.0.extend(&selector.0);
            Some(target)
        }
        (Some(text), None) => Some(text),
        (None, Some(text)) => Some(text.clone()),
        (None, None) => None,
    }
}

/// A record with every schema property null: evaluating a condition on it finds unknown names.
fn probe(info: &ClassInfo) -> Record {
    Record {
        properties: info
            .properties
            .iter()
            .map(|p| Property {
                name: p.name.as_str().into(),
                value: CimValue::Null,
                cim_type: p.cim_type,
                system: false,
                qualifiers: vec![],
            })
            .collect(),
        ..Default::default()
    }
}

/// Key assignments of an object path; `Some(None)` is the singleton form `Class=@`.
type Keys = Option<Vec<(String, String)>>;

fn parse_path(path: &str) -> Option<Keys> {
    let start = path.find(['.', '='])?;
    let rest = &path[start..];
    if let Some(singleton) = rest.strip_prefix('=') {
        return (singleton == "@").then_some(None);
    }
    let mut chars = rest[1..].chars().peekable();
    let mut keys = vec![];
    loop {
        let mut name = String::new();
        loop {
            match chars.next()? {
                '=' => break,
                c => name.push(c),
            }
        }
        let mut value = String::new();
        match chars.peek().copied() {
            Some(quote @ ('"' | '\'')) => {
                chars.next();
                loop {
                    match chars.next()? {
                        '\\' => value.push(chars.next()?),
                        c if c == quote => break,
                        c => value.push(c),
                    }
                }
            }
            _ => {
                while let Some(c) = chars.peek().copied() {
                    if c == ',' {
                        break;
                    }
                    value.push(c);
                    chars.next();
                }
                value = value.trim().to_owned();
            }
        }
        keys.push((name.trim().to_owned(), value));
        match chars.next() {
            None => break,
            Some(',') => {}
            Some(_) => return None,
        }
    }
    Some(Some(keys))
}

fn equals(value: &CimValue, literal: &str) -> bool {
    match value {
        CimValue::String(s) | CimValue::Reference(s) | CimValue::DateTime(s) => {
            s.eq_ignore_case(literal)
        }
        CimValue::Boolean(b) => literal.eq_ignore_ascii_case(if *b { "true" } else { "false" }),
        CimValue::Null | CimValue::Empty => literal.is_empty(),
        other => key_number(other).is_some_and(|n| literal.parse::<i128>() == Ok(n)),
    }
}

/// Keeps the records an object path names; `None` when the path is malformed or names a non-key.
fn by_path(info: &ClassInfo, path: &WideText, records: Vec<Record>) -> Option<Vec<Record>> {
    let Some(keys) = parse_path(&path.to_string_lossy())? else {
        return Some(records);
    };
    let mut wanted = vec![];
    for (name, literal) in &keys {
        let (_, property) = info.property(name)?;
        if !property.key {
            return None;
        }
        wanted.push((property.name.as_str(), literal.as_str()));
    }
    Some(
        records
            .into_iter()
            .filter(|record| {
                wanted.iter().all(|(name, literal)| {
                    record
                        .properties
                        .iter()
                        .find(|p| !p.system && p.name.eq_ignore_case(name))
                        .is_some_and(|p| equals(&p.value, literal))
                })
            })
            .collect(),
    )
}

/// Records matching the request, trimmed to the requested properties.
pub(super) fn select(
    request: &ResolvedRequest,
    entry: &Entry,
    ctx: &super::context::Context,
) -> Result<Vec<Record>, ManagementError> {
    let node = &request.connection.node;
    let invalid = || error::invalid_query(node);
    let info = &entry.info;
    let columns = projection(request);
    let everything = columns.is_empty() || columns.iter().any(|c| c == "*");
    for column in &columns {
        if column != "*" && info.property(column).is_none() && !is_system_name(column) {
            return Err(invalid());
        }
    }
    let predicate = condition(request)
        .map(|text| Predicate::parse(&text).map_err(|_| invalid()))
        .transpose()?;
    if let Some(predicate) = &predicate {
        predicate.matches(&probe(info)).map_err(|_| invalid())?;
    }
    let mut records = entry.provider.instances(ctx)?;
    if let Some(path) = &request.instance_path {
        records = by_path(info, path, records).ok_or_else(invalid)?;
    }
    if let Some(predicate) = &predicate {
        let mut kept = vec![];
        for record in records {
            if predicate.matches(&record).map_err(|_| invalid())? {
                kept.push(record);
            }
        }
        records = kept;
    }
    if !everything {
        for record in &mut records {
            record
                .properties
                .retain(|p| p.system || columns.iter().any(|c| p.name.eq_ignore_case(c)));
        }
    }
    Ok(records)
}
