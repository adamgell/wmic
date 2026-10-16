use super::*;
pub(super) fn start(path: &WideText, method: &WideText) -> WideText {
    let mut out: WideText = "Executing (".into();
    append(&mut out, path);
    push(&mut out, ")->");
    append(&mut out, method);
    push(&mut out, "()");
    push(&mut out, NL);
    out
}
fn literal(value: &CimValue) -> WideText {
    match value {
        CimValue::String(s) | CimValue::Reference(s) | CimValue::DateTime(s) => {
            let mut out: WideText = "\"".into();
            for u in &s.0 {
                match *u {
                    34 => push(&mut out, "\\\""),
                    92 => push(&mut out, "\\\\"),
                    10 => push(&mut out, "\\n"),
                    13 => push(&mut out, "\\r"),
                    _ => out.0.push(*u),
                }
            }
            push(&mut out, "\"");
            out
        }
        CimValue::Null | CimValue::Empty => "NULL".into(),
        _ => display_value(value),
    }
}
pub fn render(results: &[MethodResult], method: &WideText, show_start: bool) -> RenderedOutput {
    if results.is_empty() {
        return RenderedOutput {
            stdout: "No Instance(s) Available.\r\r\n".into(),
            ..Default::default()
        };
    }
    let mut output = RenderedOutput::default();
    for result in results {
        if show_start {
            append(&mut output.stdout, &start(&result.path, method));
        }
        push(&mut output.stdout, "Method execution successful.");
        push(&mut output.stdout, NL);
        push(&mut output.stdout, "Out Parameters:\r\n");
        if let Some(text) = &result.text {
            let text = text.trim_ascii();
            let mut previous = 0;
            for &u in &text.0 {
                if u == 10 && previous != 13 {
                    output.stdout.0.push(13);
                }
                output.stdout.0.push(u);
                previous = u;
            }
        } else {
            push(&mut output.stdout, "instance of __PARAMETERS\r\n{\r\n");
            let mut properties: Vec<_> = result
                .out_parameters
                .iter()
                .map(|p| (p.name.clone(), p.value.clone()))
                .collect();
            if let Some(value) = &result.return_value {
                properties.push(("ReturnValue".into(), value.clone()))
            }
            properties.sort_by_key(|(name, _)| name.to_string_lossy().to_ascii_lowercase());
            for (name, value) in properties {
                if !matches!(value, CimValue::Null | CimValue::Empty) {
                    push(&mut output.stdout, "\t");
                    append(&mut output.stdout, &name);
                    push(&mut output.stdout, " = ");
                    append(&mut output.stdout, &literal(&value));
                    push(&mut output.stdout, ";\r\n")
                }
            }
            push(&mut output.stdout, "};");
        }
        push(&mut output.stdout, "\r\n");
        push(&mut output.stderr, "\r\n");
    }
    output
}
