use super::*;
pub fn display_value(value: &CimValue) -> WideText {
    match value {
        CimValue::Empty | CimValue::Null => WideText::default(),
        CimValue::String(s) | CimValue::DateTime(s) | CimValue::Reference(s) => s.clone(),
        CimValue::Char16(u) => WideText(vec![*u]),
        CimValue::Boolean(b) => (if *b { "TRUE" } else { "FALSE" }).into(),
        CimValue::Sint8(v) => v.to_string().into(),
        CimValue::Uint8(v) => v.to_string().into(),
        CimValue::Sint16(v) => v.to_string().into(),
        CimValue::Uint16(v) => v.to_string().into(),
        CimValue::Sint32(v) => v.to_string().into(),
        CimValue::Uint32(v) => v.to_string().into(),
        CimValue::Sint64(v) => v.to_string().into(),
        CimValue::Uint64(v) => v.to_string().into(),
        CimValue::Real32(v) => f32::from_bits(*v).to_string().into(),
        CimValue::Real64(v) => f64::from_bits(*v).to_string().into(),
        CimValue::Object(r) => r.path.clone().unwrap_or_else(|| r.class.clone()),
        CimValue::Array(items) => {
            let mut out: WideText = "{".into();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    push(&mut out, ", ");
                }
                let quoted = matches!(
                    item,
                    CimValue::String(_)
                        | CimValue::DateTime(_)
                        | CimValue::Reference(_)
                        | CimValue::Char16(_)
                );
                if quoted {
                    push(&mut out, "\"");
                }
                append(&mut out, &display_value(item));
                if quoted {
                    push(&mut out, "\"");
                }
            }
            push(&mut out, "}");
            out
        }
    }
}
pub fn list(objects: &[Record], cols: &[(WideText, WideText)]) -> WideText {
    let mut out: WideText = "\r\r\n\r\r\n".into();
    for record in objects {
        for (name, source) in cols {
            append(&mut out, name);
            push(&mut out, "=");
            if let Some(value) = lookup(record, source) {
                append(&mut out, &display_value(value));
            }
            push(&mut out, NL);
        }
        push(&mut out, NL);
        push(&mut out, NL);
    }
    push(&mut out, NL);
    out
}
