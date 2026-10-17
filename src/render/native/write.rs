//! XML writers: the rawxml layout and the compact form used between stages.
use super::dom::{Dom, Id, Kind, ROOT, is_declaration};

fn text(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn attribute(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#xA;"),
            c => out.push(c),
        }
    }
}

/// Namespace declarations follow the ordinary attributes in the rawxml layout.
fn open(dom: &Dom, id: Id, out: &mut String, declarations_last: bool) {
    out.push('<');
    out.push_str(dom.name(id));
    let attrs = dom.attrs(id);
    let ordinary = attrs
        .iter()
        .filter(|(n, _)| !(declarations_last && is_declaration(n)));
    let declared = attrs
        .iter()
        .filter(|(n, _)| declarations_last && is_declaration(n));
    for (name, value) in ordinary.chain(declared) {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        attribute(out, value);
        out.push('"')
    }
}

fn instruction(dom: &Dom, id: Id, out: &mut String) {
    out.push_str("<?");
    out.push_str(dom.name(id));
    if dom.name(id) == "xml" {
        for (name, value) in dom.attrs(id) {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            attribute(out, value);
            out.push('"')
        }
    } else if !dom.node(id).text.is_empty() {
        out.push(' ');
        out.push_str(&dom.node(id).text)
    }
    out.push_str("?>")
}

fn leaf(dom: &Dom, id: Id, out: &mut String) {
    match dom.kind(id) {
        Kind::Text | Kind::CData => text(out, &dom.node(id).text),
        Kind::Comment => {
            out.push_str("<!--");
            out.push_str(&dom.node(id).text);
            out.push_str("-->")
        }
        Kind::Pi => instruction(dom, id, out),
        Kind::Element | Kind::Document => {}
    }
}

fn compact_node(dom: &Dom, id: Id, out: &mut String) {
    if dom.kind(id) != Kind::Element {
        return leaf(dom, id, out);
    }
    open(dom, id, out, false);
    out.push('>');
    for child in dom.children(id) {
        compact_node(dom, *child, out)
    }
    out.push_str("</");
    out.push_str(dom.name(id));
    out.push('>')
}

/// Serialization that parses back to the same document.
pub(super) fn compact(dom: &Dom) -> String {
    let mut out = String::new();
    for child in dom.children(ROOT) {
        compact_node(dom, *child, &mut out)
    }
    out
}

fn raw_node(dom: &Dom, id: Id, out: &mut String) {
    if dom.kind(id) != Kind::Element {
        return leaf(dom, id, out);
    }
    open(dom, id, out, true);
    out.push('>');
    if dom.attrs(id).iter().any(|(n, _)| !is_declaration(n)) && dom.has_element_child(id) {
        out.push_str("\r\n")
    }
    for child in dom.children(id) {
        raw_node(dom, *child, out)
    }
    out.push_str("</");
    out.push_str(dom.name(id));
    out.push_str(">\r\n")
}

/// The identity transform that adds a line break after every element.
pub(super) fn rawxml(dom: &Dom) -> String {
    let mut out = String::new();
    for child in dom.children(ROOT) {
        if dom.kind(*child) == Kind::Element {
            raw_node(dom, *child, &mut out)
        }
    }
    out
}
