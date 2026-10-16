use super::*;
fn field(text: &WideText) -> WideText {
    super::xml::escape(text, false)
}
fn value(v: &CimValue) -> WideText {
    match v {
        CimValue::String(s) | CimValue::DateTime(s) | CimValue::Reference(s) => s.clone(),
        CimValue::Array(items) => {
            let mut out: WideText = "{".into();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.0.push(44)
                }
                append(&mut out, &value(item))
            }
            out.0.push(125);
            out
        }
        _ => display_value(v),
    }
}
pub fn render(objects: &[Record], cols: &[(WideText, WideText)], node: &WideText) -> WideText {
    let mut out: WideText = NL.into();
    push(&mut out, "Node");
    for (name, _) in cols {
        out.0.push(44);
        append(&mut out, &field(name))
    }
    push(&mut out, NL);
    for object in objects {
        append(&mut out, &field(object.server.as_ref().unwrap_or(node)));
        for (_, source) in cols {
            out.0.push(44);
            if let Some(v) = lookup(object, source) {
                append(&mut out, &field(&value(v)))
            }
        }
        push(&mut out, NL)
    }
    out
}
