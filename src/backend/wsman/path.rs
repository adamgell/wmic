//! WMI object paths: parsing, selector extraction and key literals.
use crate::model::CimValue;

/// Key part of an object path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keys {
    /// The class name alone.
    None,
    /// `Class=@`.
    Singleton,
    /// `Class.Name="value",...` as name and unescaped value.
    Assigned(Vec<(String, String)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectPath {
    pub server: Option<String>,
    pub namespace: Option<String>,
    pub class: String,
    pub keys: Keys,
}

impl ObjectPath {
    /// Selector name/value pairs for the path's keys; empty for classes and singletons.
    pub fn selectors(&self) -> Vec<(String, String)> {
        match &self.keys {
            Keys::Assigned(keys) => keys.clone(),
            _ => vec![],
        }
    }
}

/// Splits off a leading server and namespace prefix when it precedes the class part.
fn split_prefix(text: &str) -> (Option<String>, Option<String>, &str) {
    let limit = if text.starts_with("\\\\") {
        text.find('=').unwrap_or(text.len())
    } else {
        text.find(['.', '=']).unwrap_or(text.len())
    };
    let Some(colon) = text[..limit].find(':') else {
        return (None, None, text);
    };
    let (prefix, rest) = (&text[..colon], &text[colon + 1..]);
    let trimmed = prefix.trim_start_matches('\\');
    if prefix.starts_with("\\\\") {
        let (server, namespace) = trimmed.split_once('\\').unwrap_or((trimmed, ""));
        (
            Some(server.to_owned()).filter(|s| !s.is_empty()),
            Some(namespace.to_owned()).filter(|n| !n.is_empty()),
            rest,
        )
    } else {
        (
            None,
            Some(trimmed.to_owned()).filter(|n| !n.is_empty()),
            rest,
        )
    }
}

/// Parses an object path with optional server and namespace prefix; `None` when malformed.
pub fn parse_object_path(text: &str) -> Option<ObjectPath> {
    let text = text.trim();
    let (server, namespace, rest) = split_prefix(text);
    let start = rest.find(['.', '=']).unwrap_or(rest.len());
    let class = rest[..start].trim();
    if class.is_empty() || class.contains(char::is_whitespace) {
        return None;
    }
    let tail = &rest[start..];
    let keys = if tail.is_empty() {
        Keys::None
    } else if let Some(singleton) = tail.strip_prefix('=') {
        if singleton.trim() != "@" {
            return None;
        }
        Keys::Singleton
    } else {
        Keys::Assigned(parse_assignments(&tail[1..])?)
    };
    Some(ObjectPath {
        server,
        namespace,
        class: class.to_owned(),
        keys,
    })
}

fn parse_assignments(text: &str) -> Option<Vec<(String, String)>> {
    let mut chars = text.chars().peekable();
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
        let name = name.trim().to_owned();
        if name.is_empty() {
            return None;
        }
        keys.push((name, value));
        match chars.next() {
            None => return Some(keys),
            Some(',') => {}
            Some(_) => return None,
        }
    }
}

/// Double-quoted key literal with `\` and `"` escaped.
pub fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Key literal for a converted value: strings quoted, numbers and booleans bare.
pub fn key_literal(value: &CimValue) -> Option<String> {
    Some(match value {
        CimValue::String(s) | CimValue::Reference(s) | CimValue::DateTime(s) => {
            quoted(&s.to_string_lossy())
        }
        CimValue::Boolean(b) => (if *b { "TRUE" } else { "FALSE" }).into(),
        CimValue::Null | CimValue::Empty => quoted(""),
        CimValue::Sint8(v) => v.to_string(),
        CimValue::Uint8(v) => v.to_string(),
        CimValue::Sint16(v) => v.to_string(),
        CimValue::Uint16(v) | CimValue::Char16(v) => v.to_string(),
        CimValue::Sint32(v) => v.to_string(),
        CimValue::Uint32(v) => v.to_string(),
        CimValue::Sint64(v) => v.to_string(),
        CimValue::Uint64(v) => v.to_string(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_with_quotes_and_backslashes() {
        let p = parse_object_path(r#"Win32_Directory.Name="C:\\Windows\\a \"b\"""#).unwrap();
        assert_eq!(p.class, "Win32_Directory");
        assert_eq!(
            p.keys,
            Keys::Assigned(vec![("Name".into(), r#"C:\Windows\a "b""#.into())])
        );
    }

    #[test]
    fn parses_server_namespace_prefix_and_singletons() {
        let p = parse_object_path(r"\\AYW\ROOT\CIMV2:Win32_OperatingSystem=@").unwrap();
        assert_eq!(p.server.as_deref(), Some("AYW"));
        assert_eq!(p.namespace.as_deref(), Some(r"ROOT\CIMV2"));
        assert_eq!(p.keys, Keys::Singleton);
        let dotted = parse_object_path(r#"\\host.example.com\root\cimv2:Win32_X.K="v""#).unwrap();
        assert_eq!(dotted.server.as_deref(), Some("host.example.com"));
        assert_eq!(dotted.selectors(), vec![("K".into(), "v".into())]);
        assert!(parse_object_path("Win32_Process.Handle").is_none());
    }

    #[test]
    fn colon_inside_a_key_value_is_not_a_namespace_separator() {
        let p = parse_object_path(r#"Win32_Directory.Name="C:\\x""#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(p.selectors(), vec![("Name".into(), r"C:\x".into())]);
    }

    #[test]
    fn numeric_values_may_be_bare() {
        let p = parse_object_path("Win32_X.A=5,B=\"z\"").unwrap();
        assert_eq!(
            p.selectors(),
            vec![("A".into(), "5".into()), ("B".into(), "z".into())]
        );
    }
}
