//! Platform-independent rendering of the built-in stylesheets.
//!
//! Every built-in works on the command XML document, so a stage fed with the plain text of an
//! earlier stage fails with "Invalid XML content." exactly like the XSL pipeline does.
mod cim;
mod dom;
mod markup;
mod text;
mod write;
mod xmlview;

use super::{BuiltinKind, RenderError, RenderedOutput, builtin_kind, finish_text, translate};
use crate::model::{FormatSpec, TransformSpec, TranslationTable, WideText};
use dom::Dom;

/// A prepared stage that turns the text of the previous stage into new text.
pub type Applier = Box<dyn Fn(&WideText) -> Result<WideText, RenderError>>;

/// Resolves a stylesheet that is not a built-in name.
pub type Loader<'a> = &'a dyn Fn(&TransformSpec) -> Result<Applier, RenderError>;

fn format_error() -> RenderError {
    RenderError::new("Invalid XSL format (or) file name.")
}

fn invalid_xml() -> RenderError {
    RenderError::status(0xACB5, "Invalid XML content.")
}

fn wide(text: &str) -> WideText {
    WideText(text.encode_utf16().collect())
}

/// Every stage result leaves the XSL engine with CR LF line breaks only.
fn crlf(text: &WideText) -> WideText {
    let mut out = Vec::with_capacity(text.0.len());
    let mut units = text.0.iter().copied().peekable();
    while let Some(u) = units.next() {
        match u {
            13 => {
                if units.peek() == Some(&10) {
                    units.next();
                }
                out.extend([13, 10])
            }
            10 => out.extend([13, 10]),
            u => out.push(u),
        }
    }
    WideText(out)
}

fn parameter(parameters: &[(WideText, WideText)], name: &str) -> String {
    parameters
        .iter()
        .rev()
        .find(|(n, _)| n.to_string_lossy() == name)
        .map(|(_, v)| v.to_string_lossy())
        .unwrap_or_default()
}

fn apply(
    kind: BuiltinKind,
    parameters: &[(WideText, WideText)],
    input: &WideText,
) -> Result<WideText, RenderError> {
    let dom = Dom::parse(input).ok_or_else(invalid_xml)?;
    let output = match kind {
        BuiltinKind::Rawxml => Some(write::rawxml(&dom)),
        BuiltinKind::Xml => xmlview::xml(&dom),
        BuiltinKind::Mof => Some(markup::mof(&dom)),
        BuiltinKind::Hform => Some(markup::hform(&dom)),
        BuiltinKind::Htable => Some(markup::htable(
            &dom,
            &parameter(parameters, "sortby"),
            &parameter(parameters, "datatype"),
        )),
        BuiltinKind::Csv => Some(text::csv(&dom, &parameter(parameters, "norefcomma"))),
        BuiltinKind::Table => text::table(&dom),
        BuiltinKind::List => Some(text::list(&dom)),
    };
    output.map(|s| wide(&s)).ok_or_else(format_error)
}

/// Translation tables run over the decoded text of every VALUE and KEYVALUE element.
fn translate_values(xml: &WideText, tables: &[TranslationTable]) -> Result<WideText, RenderError> {
    if tables.is_empty() {
        return Ok(xml.clone());
    }
    let mut dom = Dom::parse(xml).ok_or_else(invalid_xml)?;
    for id in dom.result_values() {
        let mut value = wide(&dom.text_property(id));
        for table in tables {
            for (from, to) in &table.entries {
                value = super::replace(&value, from, to)
            }
        }
        dom.set_text(id, value.to_string_lossy())
    }
    Ok(wide(&write::compact(&dom)))
}

enum Stage<'a> {
    Builtin(BuiltinKind, &'a [(WideText, WideText)]),
    External(Applier),
}

/// Runs the whole format chain. Every stylesheet is resolved before any stage runs.
pub fn transform_with(
    xml: &WideText,
    spec: &FormatSpec,
    external: Loader<'_>,
) -> Result<RenderedOutput, RenderError> {
    let stages = spec
        .transforms
        .iter()
        .map(|step| match builtin_kind(&step.name) {
            Some(kind) => Ok(Stage::Builtin(kind, &step.parameters)),
            None => external(step).map(Stage::External),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut text = translate_values(xml, &spec.before)?;
    for stage in stages {
        text = crlf(&match stage {
            Stage::Builtin(kind, parameters) => apply(kind, parameters, &text)?,
            Stage::External(run) => run(&text)?,
        })
    }
    text = translate(&text, &spec.after);
    Ok(RenderedOutput {
        stdout: finish_text(&text),
        ..Default::default()
    })
}

/// Built-in stylesheets only; any other name fails like a missing stylesheet file.
pub fn transform_builtin(xml: &WideText, spec: &FormatSpec) -> Result<RenderedOutput, RenderError> {
    transform_with(xml, spec, &|_| Err(format_error()))
}
