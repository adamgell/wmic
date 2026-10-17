use super::*;
pub type TranslationEntries = Vec<(WideText, WideText)>;
pub fn compile_format(
    request: &ResolvedRequest,
    tables: &[(WideText, TranslationEntries)],
) -> Result<FormatSpec, RenderError> {
    let explicit = request
        .modifiers
        .iter()
        .enumerate()
        .rev()
        .find(|(_, m)| m.name.eq_ignore_case("format"));
    let mut spec = if let Some((_, m)) = explicit {
        let SwitchValue::Value(value) = &m.value else {
            return Err(RenderError::status(0xACB4, "Invalid GET switch."));
        };
        crate::parse::parse_format_spec(m.raw_value.as_ref().unwrap_or(value))
            .map_err(|e| RenderError::status(e.code, e.message))?
    } else {
        let list = request
            .modifiers
            .iter()
            .any(|m| m.name.eq_ignore_case("value"))
            || request.view.is_some()
                && request
                    .format
                    .as_ref()
                    .is_some_and(|f| f.eq_ignore_case("list"));
        FormatSpec {
            transforms: vec![TransformSpec {
                name: if request.class_mode {
                    "mof"
                } else if list {
                    "list"
                } else {
                    "table"
                }
                .into(),
                parameters: vec![],
            }],
            before: vec![],
            after: vec![],
        }
    };
    for (i, m) in request
        .modifiers
        .iter()
        .enumerate()
        .filter(|(_, m)| m.name.eq_ignore_case("translate"))
    {
        let SwitchValue::Value(name) = &m.value else {
            return Err(RenderError::status(
                0xABED,
                "Translate table does not exists in Alias Namespace.",
            ));
        };
        let entries = tables
            .iter()
            .find(|(n, _)| n.eq_ignore_case(&name.to_string_lossy()))
            .map(|(_, t)| t.clone())
            .ok_or_else(|| {
                RenderError::status(
                    0xABED,
                    "Translate table does not exists in Alias Namespace.",
                )
            })?;
        let table = TranslationTable {
            name: name.clone(),
            entries,
        };
        if explicit.is_some_and(|(format, _)| i > format) {
            spec.after.push(table)
        } else {
            spec.before.push(table)
        }
    }
    Ok(spec)
}
pub(crate) fn replace(text: &WideText, from: &WideText, to: &WideText) -> WideText {
    if from.0.is_empty() {
        return text.clone();
    }
    let mut out = WideText::default();
    let mut at = 0;
    while at < text.0.len() {
        if text.0[at..].starts_with(&from.0) {
            out.0.extend(&to.0);
            at += from.0.len()
        } else {
            out.0.push(text.0[at]);
            at += 1
        }
    }
    out
}
pub(crate) fn translate(text: &WideText, tables: &[TranslationTable]) -> WideText {
    let mut out = text.clone();
    for table in tables {
        for (from, to) in &table.entries {
            out = replace(&out, from, to)
        }
    }
    out
}
/// Original formatting appends CRLF, then the CRT expands every LF once more.
pub fn finish_text(text: &WideText) -> WideText {
    let mut out = WideText::default();
    for &u in text.0.iter().chain([13u16, 10].iter()) {
        if u == 10 {
            out.0.push(13)
        }
        out.0.push(u)
    }
    out
}
#[cfg(windows)]
pub fn transform(xml: &WideText, spec: &FormatSpec) -> Result<RenderedOutput, RenderError> {
    let processors = spec
        .transforms
        .iter()
        .map(|step| {
            if let Some(source) = builtin_source(&step.name) {
                crate::platform::windows::xslt::prepare_source(&source, &step.parameters)
            } else {
                crate::platform::windows::xslt::prepare(step)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut text = crate::platform::windows::xslt::translate_values(xml, &spec.before)?;
    for processor in processors {
        text = processor.apply(&text)?;
    }
    text = translate(&text, &spec.after);
    Ok(RenderedOutput {
        stdout: finish_text(&text),
        ..Default::default()
    })
}

#[cfg(not(windows))]
pub fn transform(xml: &WideText, spec: &FormatSpec) -> Result<RenderedOutput, RenderError> {
    super::native::transform_with(xml, spec, &external_stylesheet)
}

#[cfg(target_os = "macos")]
fn external_stylesheet(step: &TransformSpec) -> Result<super::native::Applier, RenderError> {
    let prepared = crate::platform::macos::xslt::prepare(step)?;
    Ok(Box::new(move |text| prepared.apply(text)))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn external_stylesheet(_: &TransformSpec) -> Result<super::native::Applier, RenderError> {
    Err(RenderError::new("Invalid XSL format (or) file name."))
}
