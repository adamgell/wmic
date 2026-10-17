use super::*;
pub(crate) fn escape(text: &WideText, attribute: bool) -> WideText {
    let mut out = WideText::default();
    for &u in &text.0 {
        match u {
            38 => push(&mut out, "&amp;"),
            60 => push(&mut out, "&lt;"),
            62 => push(&mut out, "&gt;"),
            34 if attribute => push(&mut out, "&quot;"),
            _ => out.0.push(u),
        }
    }
    out
}
fn attribute(out: &mut WideText, name: &str, value: &WideText) {
    push(out, " ");
    push(out, name);
    push(out, "=\"");
    append(out, &escape(value, true));
    out.0.push(34)
}
fn value(out: &mut WideText, v: &CimValue) -> Result<(), RenderError> {
    match v {
        CimValue::Null | CimValue::Empty => {}
        CimValue::Array(items) => {
            push(out, "<VALUE.ARRAY>");
            for item in items {
                value(out, item)?
            }
            push(out, "</VALUE.ARRAY>")
        }
        CimValue::Object(object) => {
            push(out, "<VALUE.OBJECT>");
            instance(out, object)?;
            push(out, "</VALUE.OBJECT>")
        }
        CimValue::Reference(path) => {
            push(out, "<VALUE.REFERENCE>");
            append(out, &escape(path, false));
            push(out, "</VALUE.REFERENCE>")
        }
        other => {
            push(out, "<VALUE>");
            let text = match other {
                CimValue::String(s) | CimValue::DateTime(s) => s.clone(),
                CimValue::Char16(u) => WideText(vec![*u]),
                _ => display_value(other),
            };
            append(out, &escape(&text, false));
            push(out, "</VALUE>")
        }
    }
    Ok(())
}
fn property(
    out: &mut WideText,
    name: &WideText,
    ty: u32,
    v: Option<&CimValue>,
) -> Result<(), RenderError> {
    let tag = if ty & 0xfff == 13 {
        if ty & 0x2000 != 0 {
            "PROPERTY.OBJECTARRAY"
        } else {
            "PROPERTY.OBJECT"
        }
    } else if ty & 0x2000 != 0 {
        "PROPERTY.ARRAY"
    } else if ty & 0xfff == 102 {
        "PROPERTY.REFERENCE"
    } else {
        "PROPERTY"
    };
    out.0.push(60);
    push(out, tag);
    attribute(out, "NAME", name);
    if !matches!(ty & 0xfff, 102 | 13) {
        attribute(out, "TYPE", &mof::cim_type_name(ty).into());
    }
    out.0.push(62);
    if let Some(v) = v {
        if ty == (13 | 0x2000)
            && let CimValue::Array(items) = v
        {
            push(out, "<VALUE.OBJECTARRAY>");
            for item in items {
                value(out, item)?
            }
            push(out, "</VALUE.OBJECTARRAY>");
        } else {
            value(out, v)?
        }
    }
    push(out, "</");
    push(out, tag);
    out.0.push(62);
    Ok(())
}
fn instance(out: &mut WideText, record: &Record) -> Result<(), RenderError> {
    if let Some(xml) = &record.wmi_xml {
        append(out, &xml.trim_ascii());
        return Ok(());
    }
    push(out, "<INSTANCE");
    attribute(out, "CLASSNAME", &record.class);
    out.0.push(62);
    for p in &record.properties {
        property(out, &p.name, p.cim_type, Some(&p.value))?
    }
    push(out, "</INSTANCE>");
    Ok(())
}
pub fn to_wmi_xml(result: &ManagementResult) -> Result<WideText, RenderError> {
    let mut out: WideText = "<CIM>".into();
    match result {
        ManagementResult::Objects(set) => {
            for object in &set.objects {
                instance(&mut out, object)?
            }
        }
        ManagementResult::Associations(groups) => {
            for group in groups {
                instance(&mut out, &group.source)?;
                for object in &group.associated {
                    instance(&mut out, object)?
                }
            }
        }
        ManagementResult::Schema(schema) => {
            if let Some(xml) = &schema.wmi_xml {
                append(&mut out, &xml.trim_ascii())
            } else {
                push(&mut out, "<CLASS");
                attribute(&mut out, "NAME", &schema.name);
                out.0.push(62);
                for p in &schema.properties {
                    property(&mut out, &p.name, p.cim_type, None)?
                }
                push(&mut out, "</CLASS>");
            }
        }
        _ => return Err(RenderError::new("Not supported")),
    }
    push(&mut out, "</CIM>");
    Ok(out)
}

/// The `ERROR` element of a node that returned no result. A fail-fast refusal comes from the tool
/// itself, so it carries no status code and no facility.
fn error_element(error: Option<&ManagementError>) -> WideText {
    let mut body: WideText = "<ERROR>".into();
    let own_text = error.is_some_and(|e| e.operation.eq_ignore_case("failfast"));
    if let Some(error) = error
        && !own_text
    {
        push(&mut body, &format!("<HRESULT>0x{:X}</HRESULT>", error.code));
    }
    push(&mut body, "<DESCRIPTION>");
    append(
        &mut body,
        &escape(
            &error.map_or_else(|| "No Instance(s) Available.".into(), |e| e.message.clone()),
            false,
        ),
    );
    push(&mut body, "\r\n</DESCRIPTION>");
    if let Some(error) = error
        && !own_text
    {
        let facility = match (error.code >> 16) & 0x1fff {
            4 => "WMI",
            7 => "Win32",
            _ => "COM",
        };
        push(
            &mut body,
            &format!("<FACILITYCODE>{facility}</FACILITYCODE>"),
        );
    }
    push(&mut body, "</ERROR>");
    body
}
/// The command document of a node that returned no result.
pub(crate) fn error_command_xml(
    context: &RenderContext,
    node: &WideText,
    error: Option<&ManagementError>,
) -> Result<WideText, RenderError> {
    let empty = ManagementResult::Objects(ObjectSet::default());
    let mut xml = to_command_xml(&empty, context, node)?;
    let body = error_element(error);
    let marker: Vec<u16> = "<CIM></CIM>".encode_utf16().collect();
    let at = xml
        .0
        .windows(marker.len())
        .position(|v| v == marker)
        .ok_or_else(|| RenderError::new("Missing empty query XML"))?;
    xml.0.splice(at..at + marker.len(), body.0);
    Ok(xml)
}
pub fn to_command_xml(
    result: &ManagementResult,
    context: &RenderContext,
    node: &WideText,
) -> Result<WideText, RenderError> {
    command_xml(result, context, node, &[], &[])
}
/// One command document for every node: the results of the nodes that answered and an `ERROR`
/// result for each failed node, in node order.
pub fn to_command_xml_with_failures(
    result: &ManagementResult,
    context: &RenderContext,
    node: &WideText,
    order: &[WideText],
    failures: &[(WideText, ManagementError)],
) -> Result<WideText, RenderError> {
    command_xml(result, context, node, order, failures)
}
fn command_xml(
    result: &ManagementResult,
    context: &RenderContext,
    node: &WideText,
    order: &[WideText],
    failures: &[(WideText, ManagementError)],
) -> Result<WideText, RenderError> {
    let mut out: WideText = "<COMMAND".into();
    attribute(
        &mut out,
        "SEQUENCENUM",
        &context.sequence.to_string().into(),
    );
    attribute(&mut out, "ISSUEDFROM", &context.issued_from);
    attribute(&mut out, "STARTTIME", &context.started_at);
    attribute(
        &mut out,
        "EVERYCOUNT",
        &context.every_count.to_string().into(),
    );
    push(&mut out, "><REQUEST><COMMANDLINE>");
    append(&mut out, &escape(&context.command_line, false));
    push(&mut out, "</COMMANDLINE><COMMANDLINECOMPONENTS><NODELIST>");
    for node in &context.nodes {
        push(&mut out, "<NODE>");
        append(&mut out, &escape(node, false));
        push(&mut out, "</NODE>")
    }
    push(&mut out, "</NODELIST>");
    append(&mut out, &context.components_xml);
    push(&mut out, "</COMMANDLINECOMPONENTS><CONTEXT>");
    for (key, value) in &context.settings {
        out.0.push(60);
        append(&mut out, key);
        out.0.push(62);
        append(&mut out, &escape(value, false));
        push(&mut out, "</");
        append(&mut out, key);
        out.0.push(62)
    }
    push(&mut out, "</CONTEXT></REQUEST>");
    let mut groups: Vec<(WideText, Vec<Record>)> = vec![];
    if let ManagementResult::Objects(set) = result {
        for record in &set.objects {
            let label = record.result_node.as_ref().unwrap_or(node);
            if let Some((_, records)) = groups.iter_mut().find(|(n, _)| n == label) {
                records.push(record.clone())
            } else {
                groups.push((label.clone(), vec![record.clone()]))
            }
        }
    }
    if groups.is_empty() && failures.is_empty() {
        push(&mut out, "<RESULTS");
        attribute(&mut out, "NODE", node);
        out.0.push(62);
        append(&mut out, &to_wmi_xml(result)?);
        push(&mut out, "</RESULTS>");
    } else {
        let mut labels: Vec<&WideText> = order.iter().collect();
        for label in groups
            .iter()
            .map(|(l, _)| l)
            .chain(failures.iter().map(|(l, _)| l))
        {
            if !labels.contains(&label) {
                labels.push(label)
            }
        }
        let mut written: Vec<&WideText> = vec![];
        for label in labels {
            if written.contains(&label) {
                continue;
            }
            written.push(label);
            push(&mut out, "<RESULTS");
            attribute(&mut out, "NODE", label);
            out.0.push(62);
            if let Some((_, error)) = failures.iter().find(|(l, _)| l == label) {
                append(&mut out, &error_element(Some(error)));
            } else if let Some((_, objects)) = groups.iter().find(|(l, _)| l == label) {
                append(
                    &mut out,
                    &to_wmi_xml(&ManagementResult::Objects(ObjectSet {
                        objects: objects.clone(),
                        schema: None,
                    }))?,
                );
            }
            push(&mut out, "</RESULTS>");
        }
    }
    push(&mut out, "</COMMAND>");
    Ok(out)
}
