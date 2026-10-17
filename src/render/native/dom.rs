//! Minimal XML 1.0 reader with the load behavior of an MSXML6 DOM document:
//! DTDs are rejected and whitespace-only text is dropped outside `xml:space="preserve"`.
use crate::model::WideText;

pub(super) type Id = usize;
pub(super) const ROOT: Id = 0;
const MAX_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Document,
    Element,
    Text,
    CData,
    Comment,
    Pi,
}

pub(super) struct Node {
    pub kind: Kind,
    pub name: String,
    pub text: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Id>,
}

impl Node {
    fn new(kind: Kind, name: String, text: String) -> Self {
        Self {
            kind,
            name,
            text,
            attrs: vec![],
            children: vec![],
        }
    }
}

/// Node identifiers grow in document order for a freshly parsed document.
pub(super) struct Dom {
    nodes: Vec<Node>,
}

impl Dom {
    pub fn parse(text: &WideText) -> Option<Self> {
        let source = String::from_utf16(&text.0).ok()?;
        let chars: Vec<char> = source.chars().collect();
        if !chars.iter().copied().all(is_xml_char) {
            return None;
        }
        Reader {
            chars,
            at: 0,
            dom: Dom {
                nodes: vec![Node::new(Kind::Document, String::new(), String::new())],
            },
        }
        .document()
    }

    fn add(&mut self, parent: Id, node: Node) -> Id {
        let id = self.nodes.len();
        self.nodes.push(node);
        self.nodes[parent].children.push(id);
        id
    }

    pub fn node(&self, id: Id) -> &Node {
        &self.nodes[id]
    }

    pub fn kind(&self, id: Id) -> Kind {
        self.nodes[id].kind
    }

    pub fn name(&self, id: Id) -> &str {
        &self.nodes[id].name
    }

    /// Name without a namespace prefix.
    pub fn base_name(&self, id: Id) -> &str {
        let name = self.name(id);
        name.rsplit_once(':').map_or(name, |(_, local)| local)
    }

    pub fn children(&self, id: Id) -> &[Id] {
        &self.nodes[id].children
    }

    pub fn attrs(&self, id: Id) -> &[(String, String)] {
        &self.nodes[id].attrs
    }

    pub fn attr(&self, id: Id, name: &str) -> String {
        self.nodes[id]
            .attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    pub fn is_element(&self, id: Id, name: &str) -> bool {
        self.kind(id) == Kind::Element && self.name(id) == name
    }

    pub fn has_element_child(&self, id: Id) -> bool {
        self.children(id)
            .iter()
            .any(|c| self.kind(*c) == Kind::Element)
    }

    /// Concatenated text of all descendant text nodes.
    pub fn text(&self, id: Id) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: Id, out: &mut String) {
        let node = &self.nodes[id];
        match node.kind {
            Kind::Text | Kind::CData => out.push_str(&node.text),
            Kind::Document | Kind::Element => {
                for child in &node.children {
                    self.collect_text(*child, out)
                }
            }
            Kind::Comment | Kind::Pi => {}
        }
    }

    /// The MSXML `text` property: plain text loses leading and trailing whitespace of the
    /// whole string unless the edge node is CDATA.
    pub fn text_property(&self, id: Id) -> String {
        let mut parts: Vec<(&str, bool)> = vec![];
        self.collect_parts(id, &mut parts);
        let first_data = parts.first().is_some_and(|(_, data)| *data);
        let last_data = parts.last().is_some_and(|(_, data)| *data);
        let joined: String = parts.iter().map(|(t, _)| *t).collect();
        let text = if first_data {
            joined.as_str()
        } else {
            joined.trim_start_matches(is_blank)
        };
        if last_data {
            text.to_string()
        } else {
            text.trim_end_matches(is_blank).to_string()
        }
    }

    fn collect_parts<'a>(&'a self, id: Id, out: &mut Vec<(&'a str, bool)>) {
        let node = &self.nodes[id];
        match node.kind {
            Kind::Text => out.push((&node.text, false)),
            Kind::CData => out.push((&node.text, true)),
            Kind::Document | Kind::Element => {
                for child in &node.children {
                    self.collect_parts(*child, out)
                }
            }
            Kind::Comment | Kind::Pi => {}
        }
    }

    pub fn first_child(&self, id: Id, name: &str) -> Option<Id> {
        self.children(id)
            .iter()
            .copied()
            .find(|c| self.is_element(*c, name))
    }

    pub fn children_named(&self, id: Id, name: &str) -> Vec<Id> {
        self.children(id)
            .iter()
            .copied()
            .filter(|c| self.is_element(*c, name))
            .collect()
    }

    /// Child elements of every node in `set`, in document order, without duplicates.
    pub fn step(&self, set: &[Id], name: &str) -> Vec<Id> {
        let mut out: Vec<Id> = set
            .iter()
            .flat_map(|id| self.children_named(*id, name))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Descendant elements of every node in `set`, in document order, without duplicates.
    pub fn deep(&self, set: &[Id], names: &[&str]) -> Vec<Id> {
        let mut out = vec![];
        for id in set {
            self.collect_deep(*id, names, &mut out)
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn collect_deep(&self, id: Id, names: &[&str], out: &mut Vec<Id>) {
        for child in self.children(id) {
            if self.kind(*child) == Kind::Element {
                if names.contains(&self.name(*child)) {
                    out.push(*child)
                }
                self.collect_deep(*child, names, out)
            }
        }
    }

    /// Elements named VALUE or KEYVALUE below a RESULTS element.
    pub fn result_values(&self) -> Vec<Id> {
        let results = self.deep(&[ROOT], &["RESULTS"]);
        self.deep(&results, &["VALUE", "KEYVALUE"])
    }

    /// Replaces every child of `id` with one text node; an empty text leaves no child.
    pub fn set_text(&mut self, id: Id, text: String) {
        self.nodes[id].children.clear();
        let text = normalize_lines(&text);
        if !text.is_empty() {
            let node = Node::new(Kind::Text, String::new(), text);
            self.add(id, node);
        }
    }
}

/// `xmlns` and `xmlns:prefix` declare namespaces and are not attributes in the data model.
pub(super) fn is_declaration(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

fn is_xml_char(c: char) -> bool {
    matches!(c as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..)
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':' || (c as u32 > 0x7f && !c.is_whitespace())
}

fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || c.is_ascii_digit()
        || matches!(c, '.' | '-' | '\u{b7}')
        || (c as u32 > 0x7f && !c.is_whitespace())
}

/// Loader line-break rule: a carriage return, written literally or as a character reference,
/// becomes one line feed that absorbs an immediately following line feed.
fn push_normalized(out: &mut String, after_return: &mut bool, c: char) {
    match c {
        '\r' => {
            out.push('\n');
            *after_return = true
        }
        '\n' if *after_return => *after_return = false,
        c => {
            out.push(c);
            *after_return = false
        }
    }
}

fn normalize_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_return = false;
    for c in text.chars() {
        push_normalized(&mut out, &mut after_return, c)
    }
    out
}

fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

struct Reader {
    chars: Vec<char>,
    at: usize,
    dom: Dom,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn starts(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(i, c)| self.chars.get(self.at + i) == Some(&c))
    }

    fn eat(&mut self, text: &str) -> bool {
        let found = self.starts(text);
        if found {
            self.at += text.chars().count()
        }
        found
    }

    fn expect(&mut self, c: char) -> Option<()> {
        (self.peek()? == c).then(|| self.at += 1)
    }

    fn skip_blank(&mut self) -> bool {
        let start = self.at;
        while self.peek().is_some_and(is_blank) {
            self.at += 1
        }
        self.at > start
    }

    fn name(&mut self) -> Option<String> {
        let mut out = String::new();
        if !is_name_start(self.peek()?) {
            return None;
        }
        while let Some(c) = self.peek().filter(|c| is_name_char(*c)) {
            out.push(c);
            self.at += 1
        }
        Some(out)
    }

    /// Text up to the first `terminator`, which is consumed.
    fn until(&mut self, terminator: &str) -> Option<String> {
        let start = self.at;
        while !self.starts(terminator) {
            self.peek()?;
            self.at += 1
        }
        let out = self.chars[start..self.at].iter().collect();
        self.at += terminator.chars().count();
        Some(out)
    }

    fn document(mut self) -> Option<Dom> {
        if self.starts("<?xml") && self.chars.get(5).copied().is_some_and(is_blank) {
            self.declaration()?
        }
        let mut root = false;
        loop {
            self.skip_blank();
            if self.peek().is_none() {
                break;
            }
            if self.starts("<!--") {
                self.comment(ROOT)?
            } else if self.starts("<?") {
                self.pi(ROOT)?
            } else if self.starts("<") && !self.starts("<!") && !self.starts("</") {
                if root {
                    return None;
                }
                root = true;
                self.element(ROOT, 0, false, &[])?
            } else {
                return None;
            }
        }
        root.then_some(self.dom)
    }

    fn declaration(&mut self) -> Option<()> {
        self.at += 5;
        let body = self.until("?>")?;
        let chars: Vec<char> = body.chars().collect();
        let mut pairs = vec![];
        let mut at = 0;
        loop {
            while chars.get(at).copied().is_some_and(is_blank) {
                at += 1
            }
            if at >= chars.len() {
                break;
            }
            let start = at;
            while chars.get(at).copied().is_some_and(is_name_char) {
                at += 1
            }
            if at == start {
                return None;
            }
            let key: String = chars[start..at].iter().collect();
            while chars.get(at).copied().is_some_and(is_blank) {
                at += 1
            }
            if chars.get(at) != Some(&'=') {
                return None;
            }
            at += 1;
            while chars.get(at).copied().is_some_and(is_blank) {
                at += 1
            }
            let quote = *chars.get(at).filter(|c| matches!(c, '"' | '\''))?;
            at += 1;
            let start = at;
            while chars.get(at) != Some(&quote) {
                chars.get(at)?;
                at += 1
            }
            pairs.push((key, chars[start..at].iter().collect::<String>()));
            at += 1;
        }
        let names: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        let valid = matches!(
            names[..],
            ["version"]
                | ["version", "encoding"]
                | ["version", "standalone"]
                | ["version", "encoding", "standalone"]
        ) && pairs[0].1 == "1.0"
            && pairs
                .iter()
                .all(|(k, v)| k != "standalone" || matches!(v.as_str(), "yes" | "no"));
        if !valid {
            return None;
        }
        let mut node = Node::new(Kind::Pi, "xml".into(), String::new());
        node.attrs = pairs;
        self.dom.add(ROOT, node);
        Some(())
    }

    fn comment(&mut self, parent: Id) -> Option<()> {
        self.at += 4;
        let body = self.until("-->")?;
        if body.contains("--") || body.ends_with('-') {
            return None;
        }
        self.dom.add(
            parent,
            Node::new(Kind::Comment, String::new(), normalize_lines(&body)),
        );
        Some(())
    }

    fn pi(&mut self, parent: Id) -> Option<()> {
        self.at += 2;
        let target = self.name()?;
        if target.eq_ignore_ascii_case("xml") {
            return None;
        }
        let spaced = self.skip_blank();
        let data = self.until("?>")?;
        if !spaced && !data.is_empty() {
            return None;
        }
        self.dom
            .add(parent, Node::new(Kind::Pi, target, normalize_lines(&data)));
        Some(())
    }

    fn reference(&mut self) -> Option<char> {
        self.expect('&')?;
        let body = self.until(";")?;
        let c = match body.as_str() {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let digits = body.strip_prefix('#')?;
                let code = match digits.strip_prefix('x') {
                    Some(hex) if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
                        u32::from_str_radix(hex, 16).ok()?
                    }
                    Some(_) => return None,
                    None if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
                        digits.parse().ok()?
                    }
                    None => return None,
                };
                char::from_u32(code).filter(|c| is_xml_char(*c))?
            }
        };
        Some(c)
    }

    fn attribute_value(&mut self) -> Option<String> {
        let quote = self.peek().filter(|c| matches!(c, '"' | '\''))?;
        self.at += 1;
        let mut out = String::new();
        let mut after_return = false;
        loop {
            let c = self.peek()?;
            match c {
                c if c == quote => {
                    self.at += 1;
                    return Some(out);
                }
                '<' => return None,
                '&' => {
                    let c = self.reference()?;
                    push_normalized(&mut out, &mut after_return, c)
                }
                c => {
                    push_normalized(&mut out, &mut after_return, c);
                    self.at += 1
                }
            }
        }
    }

    fn element(
        &mut self,
        parent: Id,
        depth: usize,
        preserve: bool,
        scope: &[String],
    ) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.expect('<')?;
        let name = self.name()?;
        let mut attrs: Vec<(String, String)> = vec![];
        let empty = loop {
            let spaced = self.skip_blank();
            match self.peek()? {
                '/' => {
                    self.at += 1;
                    self.expect('>')?;
                    break true;
                }
                '>' => {
                    self.at += 1;
                    break false;
                }
                _ => {
                    if !spaced {
                        return None;
                    }
                    let key = self.name()?;
                    self.skip_blank();
                    self.expect('=')?;
                    self.skip_blank();
                    let value = self.attribute_value()?;
                    if attrs.iter().any(|(k, _)| *k == key) {
                        return None;
                    }
                    attrs.push((key, value))
                }
            }
        };
        let declared: Vec<String> = attrs
            .iter()
            .filter_map(|(k, _)| k.strip_prefix("xmlns:").map(str::to_string))
            .collect();
        let scope: Vec<String> = scope.iter().cloned().chain(declared).collect();
        let bound = |qualified: &str| {
            qualified
                .split_once(':')
                .is_none_or(|(prefix, _)| prefix == "xml" || scope.iter().any(|p| p == prefix))
        };
        if !bound(&name)
            || name.starts_with("xmlns:")
            || attrs
                .iter()
                .any(|(k, _)| !k.starts_with("xmlns:") && !bound(k))
        {
            return None;
        }
        let preserve = match attrs.iter().find(|(k, _)| k == "xml:space") {
            Some((_, v)) if v == "preserve" => true,
            Some((_, v)) if v == "default" => false,
            _ => preserve,
        };
        let mut node = Node::new(Kind::Element, name.clone(), String::new());
        node.attrs = attrs;
        let id = self.dom.add(parent, node);
        if empty {
            return Some(());
        }
        self.content(id, &name, depth, preserve, &scope)
    }

    /// A run of literal whitespace between markup is dropped. A run that contains a
    /// character reference is kept, as is any run below `xml:space="preserve"`.
    fn flush(&mut self, parent: Id, buffer: &mut String, referenced: &mut bool, preserve: bool) {
        let text = std::mem::take(buffer);
        if !text.is_empty() && (preserve || *referenced || text.chars().any(|c| !is_blank(c))) {
            self.dom
                .add(parent, Node::new(Kind::Text, String::new(), text));
        }
        *referenced = false
    }

    fn content(
        &mut self,
        id: Id,
        name: &str,
        depth: usize,
        preserve: bool,
        scope: &[String],
    ) -> Option<()> {
        let mut buffer = String::new();
        let mut referenced = false;
        let mut after_return = false;
        loop {
            match self.peek()? {
                '<' => {
                    self.flush(id, &mut buffer, &mut referenced, preserve);
                    after_return = false;
                    if self.eat("</") {
                        if self.name()? != name {
                            return None;
                        }
                        self.skip_blank();
                        return self.expect('>');
                    } else if self.starts("<!--") {
                        self.comment(id)?
                    } else if self.eat("<![CDATA[") {
                        let data = normalize_lines(&self.until("]]>")?);
                        if !data.is_empty() {
                            self.dom
                                .add(id, Node::new(Kind::CData, String::new(), data));
                        }
                    } else if self.starts("<?") {
                        self.pi(id)?
                    } else if self.starts("<!") {
                        return None;
                    } else {
                        self.element(id, depth + 1, preserve, scope)?
                    }
                }
                '&' => {
                    let c = self.reference()?;
                    push_normalized(&mut buffer, &mut after_return, c);
                    referenced = true
                }
                ']' if self.starts("]]>") => return None,
                c => {
                    push_normalized(&mut buffer, &mut after_return, c);
                    self.at += 1
                }
            }
        }
    }
}
