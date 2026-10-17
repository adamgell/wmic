//! Colored markup view of the whole document.
use super::cim::html;
use super::dom::{Dom, Id, Kind, ROOT, is_declaration};

fn attributes(dom: &Dom, id: Id) -> String {
    let mut out = String::new();
    let ordinary = dom.attrs(id).iter().filter(|(n, _)| !is_declaration(n));
    for (i, (name, value)) in ordinary.enumerate() {
        if i > 0 {
            out.push_str("\r\n")
        }
        let base = name
            .rsplit_once(':')
            .map_or(name.as_str(), |(_, local)| local);
        out.push_str(&format!(
            "<SPAN STYLE=\"color:navy\">{}=\"<SPAN STYLE=\"color:black\">{}</SPAN>\"</SPAN>",
            html(base),
            html(value)
        ))
    }
    out
}

/// The XML declaration has no attribute collection, so the view stylesheet fails on it.
fn node(dom: &Dom, id: Id) -> Option<String> {
    Some(match dom.kind(id) {
        Kind::Text | Kind::CData => html(&dom.node(id).text),
        Kind::Pi if dom.name(id) == "xml" => return None,
        Kind::Pi => format!(
            "<DIV STYLE=\"margin-left:1em; color:maroon\">&lt;?{}?&gt;</DIV>\r\n",
            html(dom.base_name(id))
        ),
        Kind::Element => {
            let name = html(dom.base_name(id));
            let attributes = attributes(dom, id);
            if dom.children(id).is_empty() {
                return Some(format!(
                    "<DIV STYLE=\"margin-left:1em; color:gray\">\r\n        &lt;{name}&gt;{attributes}/&gt;\r\n     </DIV>\r\n"
                ));
            }
            let mut out = format!(
                "<DIV STYLE=\"margin-left:1em\">\r\n<SPAN STYLE=\"color:gray\">&lt;{name}{attributes}&gt;</SPAN>"
            );
            for child in dom.children(id) {
                out.push_str(&node(dom, *child)?)
            }
            out.push_str(&format!(
                "<SPAN STYLE=\"color:gray\">&lt;/{name}&gt;</SPAN>\r\n</DIV>\r\n"
            ));
            out
        }
        Kind::Comment | Kind::Document => String::new(),
    })
}

pub(super) fn xml(dom: &Dom) -> Option<String> {
    let mut out =
        String::from("<DIV STYLE=\"font-family:Courier; font-size:10pt; margin-bottom:2em\">\r\n");
    for child in dom.children(ROOT) {
        out.push_str(&node(dom, *child)?)
    }
    out.push_str("</DIV>\r\n");
    Some(out)
}
