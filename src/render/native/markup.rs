//! HTML renderers: mof, htable and hform.
use super::cim::*;
use super::dom::{Dom, Id};

fn span(kind: &str, content: &str) -> String {
    format!("<SPAN CLASS=\"{kind}\">{content}</SPAN>")
}

fn normalized(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for c in text.chars() {
        if matches!(c, ' ' | '\t' | '\r' | '\n') {
            blank = true
        } else {
            if blank {
                out.push(' ');
                blank = false
            }
            out.push(c)
        }
    }
    if blank {
        out.push(' ')
    }
    let out = out.strip_prefix(' ').unwrap_or(&out);
    out.strip_suffix(' ').unwrap_or(out).to_string()
}

fn mof_value(kind: &str, text: &str) -> String {
    match kind {
        "string" | "datetime" => span("mofstring", &format!("\"{}\"", html(&normalized(text)))),
        "char16" => span("mofchar", &format!("'{}'", html(&normalized(text)))),
        _ => span("mofvalue", &html(text)),
    }
}

fn mof_property(dom: &Dom, p: Id) -> String {
    let kind = dom.attr(p, "TYPE");
    let mut out = String::from(
        "<DD>\r\n<DIV CLASS=\"mofproperty\">\r\n<SPAN CLASS=\"mofqualifierset\"> </SPAN>\r\n",
    );
    out.push_str(&span("mofkeyword", &html(&kind)));
    out.push_str("\r\n");
    out.push_str(&span("mofproperty", &html(&dom.attr(p, "NAME"))));
    out.push_str("\r\n");
    let array = dom.name(p) == "PROPERTY.ARRAY";
    if array {
        out.push_str(&span("mofsymbol", "["));
        out.push_str(&span("mofsymbol", "]"));
        out.push_str("\r\n")
    }
    let scalar = dom.first_child(p, "VALUE");
    if let (true, Some(values)) = (array, dom.first_child(p, "VALUE.ARRAY")) {
        out.push_str(&span("mofsymbol", "="));
        out.push_str(&span("mofsymbol", "{"));
        for (i, item) in dom.children_named(values, "VALUE").iter().enumerate() {
            if i > 0 {
                out.push(',')
            }
            out.push_str(&mof_value(&kind, &dom.text(*item)))
        }
        out.push_str(&span("mofsymbol", "}"));
        out.push_str("\r\n")
    } else if let Some(scalar) = scalar {
        out.push_str(&span("mofsymbol", "="));
        out.push_str(&mof_value(&kind, &dom.text(scalar)))
    }
    out.push_str(&span("mofsymbol", ";"));
    out.push_str("<BR />\r\n</DIV>\r\n</DD>\r\n");
    out
}

pub(super) fn mof(dom: &Dom) -> String {
    let results = dom.deep(&command(dom), &["RESULTS"]);
    let cim = dom.deep(&results, &["CIM"]);
    let mut out = String::new();
    for row in dom.deep(&cim, &["INSTANCE"]) {
        out.push_str(
            "<DIV CLASS=\"mofinstance\">\r\n<SPAN CLASS=\"mofqualifierset\"> <br /></SPAN>\r\n",
        );
        out.push_str(&span("mofkeyword", "instance of"));
        out.push(' ');
        out.push_str(&html(&dom.attr(row, "CLASSNAME")));
        out.push(' ');
        out.push_str(&span("mofsymbol", "{"));
        out.push_str("<BR /> <DL> ");
        for p in dom
            .children_named(row, "PROPERTY")
            .into_iter()
            .chain(dom.children_named(row, "PROPERTY.ARRAY"))
        {
            out.push_str(&mof_property(dom, p))
        }
        out.push_str("</DL>\r\n");
        out.push_str(&span("mofsymbol", "};"));
        out.push_str("\r\n</DIV>\r\n")
    }
    out
}

const DOT: &str = "<span style=\"height:1px;overflow-y:hidden\">.</span>";
const HEADING: &str =
    "<tr style=\"background-color:#a0a0ff;font:10pt Tahoma;font-weight:bold;\" align=\"left\">";

fn row_style(position: usize) -> String {
    format!(
        "<tr style=\"background-color:#{};font:10pt Tahoma;\">\r\n",
        if position % 2 == 1 {
            "f0f0f0"
        } else {
            "e0f0f0"
        }
    )
}

fn html_value(dom: &Dom, p: Id) -> String {
    if dom.name(p) == "PROPERTY.REFERENCE" {
        html(&reference(dom, p, false, None))
    } else {
        html(&value(dom, p, Mode::List, ""))
    }
}

/// Number conversion of the sort key: blank text is zero and unparsable text is NaN.
fn number(text: &str) -> f64 {
    let t = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    if let Some(digits) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return if digits.is_empty() {
            f64::NAN
        } else {
            u64::from_str_radix(digits, 16).map_or(f64::NAN, |n| n as f64)
        };
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) =>
        {
            t.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

/// The script engine sorts by binary insertion: an element that does not compare as less than
/// its predecessor stays in place, otherwise its slot is found by bisection. A comparison that
/// yields NaN counts as not less.
fn sort_rows(dom: &Dom, rows: &mut Vec<Id>, sortby: &str, datatype: &str) {
    let keys: Vec<String> = rows
        .iter()
        .map(|row| {
            let mut found = String::new();
            for p in properties(dom, *row) {
                if dom.attr(p, "NAME") == sortby {
                    found = dom.text(p)
                }
            }
            found
        })
        .collect();
    let less = |a: &str, b: &str| {
        if datatype == "number" {
            number(a) - number(b) < 0.0
        } else {
            a.encode_utf16().lt(b.encode_utf16())
        }
    };
    let mut order: Vec<usize> = vec![];
    for i in 0..rows.len() {
        let at = match order.last() {
            Some(last) if less(&keys[i], &keys[*last]) => {
                let (mut low, mut high) = (0isize, order.len() as isize - 1);
                while low <= high {
                    let middle = (low + high) / 2;
                    if less(&keys[i], &keys[order[middle as usize]]) {
                        high = middle - 1
                    } else {
                        low = middle + 1
                    }
                }
                low as usize
            }
            _ => order.len(),
        };
        order.insert(at, i)
    }
    *rows = order.into_iter().map(|i| rows[i]).collect();
}

pub(super) fn htable(dom: &Dom, sortby: &str, datatype: &str) -> String {
    let groups = groups(dom);
    let all = dom.step(&dom.step(&groups, "CIM"), "INSTANCE");
    let mut out = format!(
        "<html>\r\n<H3>{} Instances of {}</H3>\r\n<table border=\"1\">\r\n{HEADING}\r\n<td>Node</td>\r\n",
        all.len(),
        all.first()
            .map(|i| html(&dom.attr(*i, "CLASSNAME")))
            .unwrap_or_default()
    );
    if let Some(first) = all.first() {
        for head in properties(dom, *first) {
            out.push_str(&format!("<td>{}</td>\r\n", html(&dom.attr(head, "NAME"))))
        }
    }
    out.push_str("</tr>\r\n");
    for (g, group) in groups.iter().enumerate() {
        out.push_str(&row_style(g + 2));
        let mut rows = instances(dom, *group);
        if !sortby.is_empty() {
            sort_rows(dom, &mut rows, sortby, datatype)
        }
        for (r, row) in rows.iter().enumerate() {
            out.push_str(&row_style(r + 1));
            out.push_str(&format!(
                "<td align=\"left\">{}</td>\r\n",
                html(&dom.attr(*group, "NODE"))
            ));
            for p in properties(dom, *row) {
                out.push_str(&format!("<td>{}{DOT}</td>\r\n", html_value(dom, p)))
            }
            out.push_str("</tr>\r\n")
        }
        out.push_str("</tr>\r\n")
    }
    out.push_str("</table>\r\n</html>\r\n");
    out
}

pub(super) fn hform(dom: &Dom) -> String {
    let mut out = String::from("<html>\r\n");
    for group in groups(dom) {
        let rows = instances(dom, group);
        out.push_str(&format!(
            "<H3>Node: {} - {} Instances of {}</H3>\r\n<table border=\"1\">\r\n",
            html(&dom.attr(group, "NODE")),
            rows.len(),
            rows.first()
                .map(|r| html(&dom.attr(*r, "CLASSNAME")))
                .unwrap_or_default()
        ));
        for row in rows {
            out.push_str(&format!(
                "{HEADING}<td colspan=\"2\">INSTANCE{DOT}</td></tr>\r\n<tr style=\"background-color:#c0c0c0;font:8pt Tahoma;\">\r\n<td>Property Name</td><td>Value</td>\r\n</tr>\r\n"
            ));
            for (c, p) in properties(dom, row).iter().enumerate() {
                out.push_str(&format!(
                    "{}<td>{}</td>\r\n<td>{}{DOT}</td>\r\n</tr>\r\n",
                    row_style(c + 1),
                    html(&dom.attr(*p, "NAME")),
                    html_value(dom, *p)
                ))
            }
            out.push_str(&format!(
                "<tr style=\"background-color:#ffffff;font:10pt Tahoma;font-weight:bold;\"><td colspan=\"2\">{DOT}</td></tr>\r\n"
            ))
        }
        out.push_str("</table>\r\n")
    }
    out.push_str("</html>\r\n");
    out
}
