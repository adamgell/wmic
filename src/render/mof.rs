use super::*;
pub fn cim_type_name(ty: u32) -> &'static str {
    match ty & 0xfff {
        2 => "sint16",
        3 => "sint32",
        4 => "real32",
        5 => "real64",
        8 => "string",
        11 => "boolean",
        13 => "object",
        16 => "sint8",
        17 => "uint8",
        18 => "uint16",
        19 => "uint32",
        20 => "sint64",
        21 => "uint64",
        101 => "datetime",
        102 => "reference",
        103 => "char16",
        _ => "void",
    }
}
fn line(out: &mut WideText, s: &str) {
    push(out, s);
    push(out, NL)
}
fn named(out: &mut WideText, text: &WideText) {
    for u in &text.0 {
        match *u {
            38 => push(out, "&amp;"),
            60 => push(out, "&lt;"),
            62 => push(out, "&gt;"),
            34 => push(out, "&quot;"),
            _ => out.0.push(*u),
        }
    }
}
fn span(out: &mut WideText, class: &str, text: &str) {
    push(out, "<SPAN CLASS=\"");
    push(out, class);
    push(out, "\">");
    push(out, text);
    push(out, "</SPAN>")
}
pub fn class(schema: &ClassSchema) -> WideText {
    let mut out = WideText::default();
    line(&mut out, "<DIV CLASS=\"mofclass\">");
    line(&mut out, "<SPAN CLASS=\"mofqualifierset\"> <br /></SPAN>");
    span(&mut out, "mofkeyword", "class");
    named(&mut out, &schema.name);
    line(&mut out, "<BR />");
    line(&mut out, "<SPAN CLASS=\"mofsymbol\">{</SPAN><BR />");
    let mut props: Vec<_> = schema.properties.iter().collect();
    props.sort_by_key(|p| {
        (
            p.cim_type & 0x2000 != 0,
            p.name.to_string_lossy().to_ascii_lowercase(),
        )
    });
    for p in props {
        line(&mut out, "<DD>");
        line(&mut out, "<DIV CLASS=\"mofproperty\">");
        line(&mut out, "<SPAN CLASS=\"mofqualifierset\"> </SPAN>");
        span(&mut out, "mofkeyword", cim_type_name(p.cim_type));
        push(&mut out, NL);
        push(&mut out, "<SPAN CLASS=\"mofproperty\">");
        named(&mut out, &p.name);
        line(&mut out, "</SPAN>");
        if p.cim_type & 0x2000 != 0 {
            line(
                &mut out,
                "<SPAN CLASS=\"mofsymbol\">[</SPAN><SPAN CLASS=\"mofsymbol\">]</SPAN>",
            );
        }
        line(&mut out, "<SPAN CLASS=\"mofsymbol\">;</SPAN><BR />");
        line(&mut out, "</DIV>");
        line(&mut out, "</DD>");
    }
    for method in &schema.methods {
        line(&mut out, "<DIV CLASS=\"mofmethod\">");
        line(&mut out, "<SPAN CLASS=\"mofqualifierset\"> </SPAN>");
        let ty = method
            .output
            .iter()
            .find(|p| p.name.eq_ignore_case("ReturnValue"))
            .map_or(0, |p| p.cim_type);
        span(&mut out, "mofkeyword", cim_type_name(ty));
        push(&mut out, NL);
        push(&mut out, "<SPAN CLASS=\"mofmethod\">");
        named(&mut out, &method.name);
        line(&mut out, "</SPAN>");
        line(&mut out, "<SPAN CLASS=\"mofsymbol\">(</SPAN>");
        let mut params: Vec<_> = method
            .input
            .iter()
            .chain(&method.output)
            .filter(|p| !p.name.eq_ignore_case("ReturnValue"))
            .collect();
        params.sort_by_key(|p| p.id.unwrap_or(u32::MAX));
        let mut seen = std::collections::BTreeSet::new();
        params.retain(|p| seen.insert(p.name.clone()));
        for (i, p) in params.iter().enumerate() {
            line(&mut out, "<DIV CLASS=\"mofmethodparameter\">");
            line(&mut out, "<SPAN CLASS=\"mofqualifierset\"> </SPAN>");
            span(&mut out, "mofkeyword", cim_type_name(p.cim_type));
            named(&mut out, &p.name);
            if p.cim_type & 0x2000 != 0 {
                push(
                    &mut out,
                    "<SPAN CLASS=\"mofsymbol\">[</SPAN><SPAN CLASS=\"mofsymbol\">]</SPAN>",
                );
            }
            if i + 1 < params.len() {
                line(&mut out, "<SPAN CLASS=\"mofsymbol\">,</SPAN>");
            }
            line(&mut out, "</DIV>");
        }
        line(&mut out, "<SPAN CLASS=\"mofsymbol\">);</SPAN>");
        line(&mut out, "</DIV>");
    }
    line(&mut out, "<SPAN CLASS=\"mofsymbol\">};</SPAN>");
    line(&mut out, "</DIV>");
    push(&mut out, NL);
    out
}

pub fn instances(objects: &[Record], cols: &[(WideText, WideText)]) -> WideText {
    let mut out = WideText::default();
    for object in objects {
        line(&mut out, "<DIV CLASS=\"mofinstance\">");
        line(&mut out, "<SPAN CLASS=\"mofqualifierset\"> <br /></SPAN>");
        span(&mut out, "mofkeyword", "instance of");
        push(&mut out, " ");
        named(&mut out, &object.class);
        line(
            &mut out,
            " <SPAN CLASS=\"mofsymbol\">{</SPAN><BR /> <DL> <DD>",
        );
        let mut properties: Vec<_> = cols
            .iter()
            .filter_map(|(_, source)| object.properties.iter().find(|p| p.name == *source))
            .collect();
        properties.sort_by_key(|p| {
            (
                p.cim_type & 0x2000 != 0,
                p.name.to_string_lossy().to_ascii_lowercase(),
            )
        });
        for (i, p) in properties.iter().enumerate() {
            if i > 0 {
                line(&mut out, "<DD>")
            }
            line(&mut out, "<DIV CLASS=\"mofproperty\">");
            line(&mut out, "<SPAN CLASS=\"mofqualifierset\"> </SPAN>");
            span(&mut out, "mofkeyword", cim_type_name(p.cim_type));
            push(&mut out, NL);
            push(&mut out, "<SPAN CLASS=\"mofproperty\">");
            named(&mut out, &p.name);
            line(&mut out, "</SPAN>");
            if p.cim_type & 0x2000 != 0 {
                line(
                    &mut out,
                    "<SPAN CLASS=\"mofsymbol\">[</SPAN><SPAN CLASS=\"mofsymbol\">]</SPAN>",
                )
            }
            if !matches!(p.value, CimValue::Null | CimValue::Empty) {
                span(&mut out, "mofsymbol", "=");
                instance_value(&mut out, &p.value);
                if p.cim_type & 0x2000 != 0 {
                    push(&mut out, NL)
                }
            }
            line(&mut out, "<SPAN CLASS=\"mofsymbol\">;</SPAN><BR />");
            line(&mut out, "</DIV>");
            line(&mut out, "</DD>");
        }
        line(&mut out, "</DL>");
        line(&mut out, "<SPAN CLASS=\"mofsymbol\">};</SPAN>");
        line(&mut out, "</DIV>");
    }
    push(&mut out, NL);
    out
}
fn instance_value(out: &mut WideText, value: &CimValue) {
    match value {
        CimValue::Array(items) => {
            span(out, "mofsymbol", "{");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    span(out, "mofsymbol", ",")
                }
                instance_value(out, item)
            }
            span(out, "mofsymbol", "}")
        }
        CimValue::String(s) | CimValue::DateTime(s) | CimValue::Reference(s) => {
            push(out, "<SPAN CLASS=\"mofstring\">\"");
            for u in &s.0 {
                match *u {
                    38 => push(out, "&amp;"),
                    60 => push(out, "&lt;"),
                    62 => push(out, "&gt;"),
                    _ => out.0.push(*u),
                }
            }
            push(out, "\"</SPAN>")
        }
        _ => {
            push(out, "<SPAN CLASS=\"mofvalue\">");
            append(out, &display_value(value));
            push(out, "</SPAN>")
        }
    }
}
