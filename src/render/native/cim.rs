//! Property and value helpers shared by the built-in renderers.
use super::dom::{Dom, Id, ROOT};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Table,
    Csv,
    List,
}

pub(super) fn html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

pub(super) fn properties(dom: &Dom, id: Id) -> Vec<Id> {
    dom.children(id)
        .iter()
        .copied()
        .filter(|c| {
            ["PROPERTY", "PROPERTY.ARRAY", "PROPERTY.REFERENCE"]
                .iter()
                .any(|n| dom.is_element(*c, n))
        })
        .collect()
}

pub(super) fn command(dom: &Dom) -> Vec<Id> {
    dom.step(&[ROOT], "COMMAND")
}

/// `/COMMAND/RESULTS`
pub(super) fn groups(dom: &Dom) -> Vec<Id> {
    dom.step(&command(dom), "RESULTS")
}

/// `CIM/INSTANCE` below one RESULTS element.
pub(super) fn instances(dom: &Dom, group: Id) -> Vec<Id> {
    dom.step(&dom.step(&[group], "CIM"), "INSTANCE")
}

pub(super) fn first_text(dom: &Dom, id: Option<Id>) -> String {
    id.map(|id| dom.text(id)).unwrap_or_default()
}

fn namespace_path(dom: &Dom, id: Id) -> String {
    let mut out = String::new();
    if let Some(host) = dom.first_child(id, "HOST") {
        out.push_str("\\\\");
        out.push_str(&dom.text(host));
    }
    for local in dom.children_named(id, "LOCALNAMESPACEPATH") {
        for part in dom.children_named(local, "NAMESPACE") {
            out.push('\\');
            out.push_str(&dom.attr(part, "NAME"));
        }
    }
    out.push(':');
    out
}

/// A reference without structured path elements keeps its stored text.
pub(super) fn reference(dom: &Dom, p: Id, quoted: bool, trailing: Option<&str>) -> String {
    let values = dom.children_named(p, "VALUE.REFERENCE");
    let paths = dom.deep(&values, &["NAMESPACEPATH"]);
    let names = dom.deep(&values, &["INSTANCENAME"]);
    let mut out = String::new();
    for path in &paths {
        out.push_str(&namespace_path(dom, *path))
    }
    for name in &names {
        let keys = dom.children_named(*name, "KEYBINDING");
        out.push_str(&dom.attr(*name, "CLASSNAME"));
        for (k, key) in keys.iter().enumerate() {
            if k == 0 {
                out.push('.')
            }
            out.push_str(&dom.attr(*key, "NAME"));
            out.push_str("=\"");
            out.push_str(&first_text(dom, dom.first_child(*key, "KEYVALUE")));
            out.push('"');
            if let Some(trailing) = trailing {
                out.push_str(trailing)
            } else if k + 1 < keys.len() {
                out.push(',')
            }
        }
    }
    if paths.is_empty() && names.is_empty() {
        for value in &values {
            out.push_str(&dom.text(*value))
        }
    }
    if quoted { format!("\"{out}\"") } else { out }
}

pub(super) fn value(dom: &Dom, p: Id, mode: Mode, norefcomma: &str) -> String {
    match dom.name(p) {
        "PROPERTY.REFERENCE" => {
            let trailing =
                (mode == Mode::Csv).then(|| if norefcomma == "true" { " " } else { "," });
            reference(dom, p, mode != Mode::Table, trailing)
        }
        "PROPERTY.ARRAY" => {
            let Some(array) = dom.first_child(p, "VALUE.ARRAY") else {
                return String::new();
            };
            let kind = dom.attr(p, "TYPE");
            let quote = match (mode, kind.as_str()) {
                (Mode::Csv, _) => "",
                (_, "string") => "\"",
                (_, "char16") => "'",
                _ => "",
            };
            let mut out = String::from("{");
            for (i, item) in dom.children_named(array, "VALUE").iter().enumerate() {
                if i > 0 {
                    out.push_str(match mode {
                        Mode::Csv => ";",
                        Mode::List => ",",
                        Mode::Table => ", ",
                    })
                }
                out.push_str(quote);
                out.push_str(&dom.text(*item));
                out.push_str(quote)
            }
            out.push('}');
            out
        }
        _ => {
            let out = dom.text(p);
            if mode == Mode::Table {
                out.replace(['\r', '\n'], "")
            } else {
                out
            }
        }
    }
}

/// Display width used for column padding: code units above 255 count twice.
pub(super) fn width(text: &str) -> usize {
    text.encode_utf16()
        .map(|u| if u > 255 { 2 } else { 1 })
        .sum()
}
