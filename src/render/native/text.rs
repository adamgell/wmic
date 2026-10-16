//! Plain-text renderers: table, csv and list.
use super::cim::*;
use super::dom::{Dom, ROOT};

const MAX_ROWS: usize = 2049;
const MAX_COLUMNS: usize = 129;

/// Row and column limits of the original formatter fail the stylesheet.
pub(super) fn table(dom: &Dom) -> Option<String> {
    let rows = dom.deep(&[ROOT], &["INSTANCE"]);
    if rows.len() > MAX_ROWS {
        return None;
    }
    let mut data: Vec<Vec<String>> = vec![];
    let mut heads: Vec<String> = vec![];
    let mut columns = 0;
    for (r, row) in rows.iter().enumerate() {
        let props = properties(dom, *row);
        columns = props.len();
        if columns > MAX_COLUMNS {
            return None;
        }
        let mut cells = vec![];
        for p in &props {
            if r == 0 {
                heads.push(dom.attr(*p, "NAME"))
            }
            cells.push(value(dom, *p, Mode::Table, ""))
        }
        data.push(cells)
    }
    let cell = |row: &[String], c: usize| row.get(c).cloned().unwrap_or_default();
    let sizes: Vec<usize> = (0..columns)
        .map(|c| {
            data.iter()
                .map(|row| width(&cell(row, c)))
                .chain([width(&cell(&heads, c))])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for line in std::iter::once(&heads).chain(data.iter()) {
        for (c, size) in sizes.iter().enumerate() {
            let s = cell(line, c);
            out.push_str(&s);
            out.extend(std::iter::repeat_n(' ', size - width(&s) + 2));
        }
        out.push_str("\r\n")
    }
    Some(out)
}

pub(super) fn csv(dom: &Dom, norefcomma: &str) -> String {
    let groups = groups(dom);
    let first = first_instances(dom, &groups);
    let headers = dom.deep(
        &first,
        &["PROPERTY", "PROPERTY.ARRAY", "PROPERTY.REFERENCE"],
    );
    let mut out = String::from("\r\nNode,");
    for (c, h) in headers.iter().enumerate() {
        if c > 0 {
            out.push(',')
        }
        out.push_str(&html(&dom.attr(*h, "NAME")))
    }
    for group in &groups {
        for row in instances(dom, *group) {
            out.push_str("\r\n");
            out.push_str(&html(&dom.attr(*group, "NODE")));
            out.push(',');
            for (c, p) in properties(dom, row).iter().enumerate() {
                if c > 0 {
                    out.push(',')
                }
                out.push_str(&html(&value(dom, *p, Mode::Csv, norefcomma)))
            }
        }
    }
    out
}

/// `/COMMAND/RESULTS[1]/CIM/INSTANCE[1]`: the first INSTANCE of each CIM below the first RESULTS.
fn first_instances(dom: &Dom, groups: &[usize]) -> Vec<usize> {
    let Some(group) = groups.first() else {
        return vec![];
    };
    dom.children_named(*group, "CIM")
        .iter()
        .filter_map(|cim| dom.first_child(*cim, "INSTANCE"))
        .collect()
}

pub(super) fn list(dom: &Dom) -> String {
    let mut out = String::from("\r\n");
    for row in dom.deep(&[ROOT], &["INSTANCE"]) {
        out.push_str("\r\n");
        for p in properties(dom, row) {
            out.push_str(&html(&dom.attr(p, "NAME")));
            out.push('=');
            out.push_str(&html(&value(dom, p, Mode::List, "")));
            out.push_str("\r\n")
        }
        out.push_str("\r\n")
    }
    out.push_str("\r\n");
    out
}
