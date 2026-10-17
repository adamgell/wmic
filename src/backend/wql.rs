//! WQL assembly preserves user UTF-16 and keeps value expressions opaque.
use super::{AssociationFilters, ResolvedRequest};
use crate::model::{Verb, WideText};
fn push(out: &mut WideText, s: &str) {
    out.0.extend(s.encode_utf16())
}
pub(crate) fn keyword(text: &[u16], word: &str) -> Option<usize> {
    let word: Vec<_> = word.encode_utf16().collect();
    let mut quote = None;
    let mut i = 0;
    while i < text.len() {
        let u = text[i];
        if let Some(q) = quote {
            if u == 92 {
                i += 2;
                continue;
            }
            if u == q {
                quote = None
            }
            i += 1;
            continue;
        }
        if u == 34 || u == 39 {
            quote = Some(u);
            i += 1;
            continue;
        }
        if i + word.len() <= text.len()
            && WideText(text[i..i + word.len()].to_vec())
                .eq_ignore_case(&String::from_utf16_lossy(&word))
            && (i == 0 || !identifier(text[i - 1]))
            && (i + word.len() == text.len() || !identifier(text[i + word.len()]))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}
fn identifier(u: u16) -> bool {
    u >= 128
        || (65..=90).contains(&u)
        || (97..=122).contains(&u)
        || (48..=57).contains(&u)
        || u == 95
}
pub fn build_query(r: &ResolvedRequest) -> WideText {
    let properties: Option<Vec<WideText>> = match r.verb {
        Verb::Get if !r.properties.is_empty() => Some(r.properties.clone()),
        Verb::List => Some(r.display.iter().map(|(_, source)| source.clone()).collect()),
        Verb::Assoc => Some(vec!["__PATH".into()]),
        _ => None,
    };
    let mut out: WideText = "SELECT ".into();
    if let Some(props) = &properties {
        for (i, p) in props.iter().enumerate() {
            if i > 0 {
                out.0.push(44)
            }
            out.0.extend(&p.0)
        }
    } else {
        out.0.push(42)
    }
    push(&mut out, " FROM ");
    let mut body = match &r.query {
        Some(q) => keyword(&q.0, "from")
            .map(|i| WideText(q.0[i + 4..].to_vec()).trim_ascii())
            .unwrap_or_else(|| r.class_name.clone()),
        None => r
            .instance_path
            .clone()
            .unwrap_or_else(|| r.class_name.clone()),
    };
    // A target's original projection is authoritative unless the command supplies one.
    if properties.is_none()
        && let Some(q) = &r.query
        && keyword(&q.0, "select").is_some()
    {
        out = q.clone();
    } else {
        out.0.append(&mut body.0)
    }
    if let Some(selector) = &r.selector {
        let has_where = keyword(&out.0, "where").is_some();
        push(&mut out, if has_where { " AND " } else { " WHERE " });
        out.0.extend(&selector.0)
    }
    out
}
pub fn build_association_query(path: &WideText, filters: &AssociationFilters) -> WideText {
    let mut out: WideText = "ASSOCIATORS OF {".into();
    out.0.extend(&path.0);
    push(&mut out, "}");
    let mut first = true;
    for (key, value) in [
        ("AssocClass", &filters.assoc_class),
        ("ResultClass", &filters.result_class),
        ("ResultRole", &filters.result_role),
    ] {
        if let Some(value) = value {
            push(&mut out, if first { " WHERE " } else { " " });
            first = false;
            push(&mut out, key);
            push(&mut out, "=");
            out.0.extend(&value.0)
        }
    }
    out
}
