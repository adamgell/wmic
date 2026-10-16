use super::*;
pub fn render(groups: &[AssociationGroup]) -> RenderedOutput {
    if groups.is_empty() {
        return RenderedOutput {
            stdout: "\r\r\n\r\r\n".into(),
            stderr: "No Instance(s) Available.\r\r\n".into(),
            ..Default::default()
        };
    }
    let mut rows = vec![];
    let mut stderr = WideText::default();
    for group in groups {
        let path = group.source.path.clone().unwrap_or_default();
        rows.push(path.clone());
        if group.associated.is_empty() {
            append(&mut stderr, &path);
            push(&mut stderr, " - No Associator(s) Available.");
            push(&mut stderr, NL)
        }
        for record in &group.associated {
            let mut values = vec![];
            for name in system_property_order() {
                if let Some(p) = record
                    .properties
                    .iter()
                    .find(|p| p.name.eq_ascii_ignore_case(name))
                {
                    values.push(&p.value)
                }
            }
            let mut regular: Vec<_> = record.properties.iter().filter(|p| !p.system).collect();
            regular.sort_by_key(|p| p.name.to_string_lossy().to_ascii_lowercase());
            values.extend(regular.into_iter().map(|p| &p.value));
            let mut row = WideText::default();
            for value in values {
                append(&mut row, &display_value(value));
                push(&mut row, "  ")
            }
            row.0.truncate(row.0.len().saturating_sub(2));
            rows.push(row);
        }
    }
    let objects: Vec<_> = rows
        .into_iter()
        .map(|value| Record {
            properties: vec![Property {
                name: "__PATH".into(),
                value: CimValue::String(value),
                cim_type: 8,
                system: true,
                qualifiers: vec![],
            }],
            ..Default::default()
        })
        .collect();
    RenderedOutput {
        stdout: table::table(&objects, &[("__PATH".into(), "__PATH".into())]),
        stderr,
        record: None,
    }
}
