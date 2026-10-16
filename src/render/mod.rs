mod diagnostic;
mod encoding;
mod table;
mod value;
use crate::{backend::ResolvedRequest, model::*};
pub use encoding::*;
pub use value::display_value;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedOutput {
    pub stdout: WideText,
    pub stderr: WideText,
    pub record: Option<WideText>,
}
#[derive(Debug)]
pub struct RenderError(pub WideText);
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for RenderError {}
pub struct RenderRequest<'a> {
    pub request: &'a ResolvedRequest,
    pub result: Result<&'a ManagementResult, &'a ManagementError>,
    pub node: &'a WideText,
}
pub(crate) fn push(out: &mut WideText, text: &str) {
    out.0.extend(text.encode_utf16());
}
pub(crate) fn append(out: &mut WideText, text: &WideText) {
    out.0.extend(&text.0);
}
pub(crate) const NL: &str = "\r\r\n";
pub(crate) fn columns(request: &ResolvedRequest, objects: &[Record]) -> Vec<(WideText, WideText)> {
    let all = || {
        objects
            .first()
            .map(|r| {
                r.properties
                    .iter()
                    .filter(|p| !p.system)
                    .map(|p| (p.name.clone(), p.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut cols = if request.verb == Verb::Get {
        if request.properties.is_empty() {
            all()
        } else {
            request
                .properties
                .iter()
                .map(|name| {
                    let canonical = objects
                        .iter()
                        .flat_map(|r| &r.properties)
                        .find(|p| p.name.eq_ascii_ignore_case(&name.to_string_lossy()))
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| name.clone());
                    (canonical.clone(), canonical)
                })
                .collect()
        }
    } else if !request.display.is_empty() {
        request.display.clone()
    } else {
        all()
    };
    cols.sort_by_key(|(name, _)| {
        (
            system_property_order()
                .iter()
                .position(|n| name.eq_ascii_ignore_case(n))
                .unwrap_or(10),
            name.to_string_lossy().to_ascii_lowercase(),
        )
    });
    cols
}
pub(crate) fn lookup<'a>(r: &'a Record, name: &WideText) -> Option<&'a CimValue> {
    r.properties
        .iter()
        .find(|p| p.name.eq_ascii_ignore_case(&name.to_string_lossy()))
        .map(|p| &p.value)
}
pub fn render(input: RenderRequest<'_>) -> Result<RenderedOutput, RenderError> {
    let req = input.request;
    let result = match input.result {
        Ok(r) => r,
        Err(e) => {
            let mut output = diagnostic::management(e, input.node);
            if !output.stdout.0.is_empty()
                && req.verb == Verb::List
                && req.view.is_some()
                && req
                    .format
                    .as_ref()
                    .is_some_and(|f| f.eq_ascii_ignore_case("list"))
            {
                push(&mut output.stdout, NL);
            }
            return Ok(output);
        }
    };
    if let ManagementResult::Schema(schema) = result {
        let value = req
            .modifiers
            .iter()
            .any(|m| m.name.eq_ascii_ignore_case("value"));
        return Ok(RenderedOutput {
            stdout: if value {
                "\r\r\n\r\r\n\r\r\n".into()
            } else {
                mof::class(schema)
            },
            ..Default::default()
        });
    }
    if let ManagementResult::Associations(groups) = result {
        return Ok(association::render(groups));
    }
    let ManagementResult::Objects(set) = result else {
        return Err(RenderError("Not supported".into()));
    };
    if set.objects.is_empty() {
        return Ok(RenderedOutput {
            stdout: "\r\r\n\r\r\n".into(),
            stderr: "No Instance(s) Available.\r\r\n".into(),
            record: None,
        });
    }
    let cols = columns(req, &set.objects);
    let format = req
        .modifiers
        .iter()
        .filter(|m| m.name.eq_ascii_ignore_case("format"))
        .filter_map(|m| {
            if let SwitchValue::Value(v) = &m.value {
                Some(v)
            } else {
                None
            }
        })
        .next_back();
    let list = req
        .modifiers
        .iter()
        .any(|m| m.name.eq_ascii_ignore_case("value"))
        || format.is_some_and(|f| f.eq_ascii_ignore_case("list"))
        || (req.view.is_some()
            && req
                .format
                .as_ref()
                .is_some_and(|f| f.eq_ascii_ignore_case("list")));
    if let Some(f) = format
        && !f.eq_ascii_ignore_case("list")
        && !f.eq_ascii_ignore_case("table")
    {
        return Err(RenderError("Invalid XSL format (or) file name.".into()));
    }
    Ok(RenderedOutput {
        stdout: if list {
            value::list(&set.objects, &cols)
        } else {
            table::table(&set.objects, &cols)
        },
        ..Default::default()
    })
}
pub fn render_parse_error(e: &ParseError) -> RenderedOutput {
    diagnostic::text(
        &e.message,
        if e.code == 0xABFD {
            e.token.as_ref()
        } else {
            None
        },
    )
}
pub fn render_resolve_error(e: &crate::alias::ResolveError) -> RenderedOutput {
    if e.code & 0x80000000 != 0 {
        return diagnostic::management(
            &ManagementError {
                code: e.code,
                target: WideText::default(),
                message: e.message.clone(),
                operation: "role".into(),
                extended: vec![],
                partial: None,
            },
            &WideText::default(),
        );
    }
    diagnostic::text(&e.message, e.token.as_ref())
}
pub mod help;
pub mod mof;
pub(crate) fn system_property_names() -> Vec<&'static str> {
    vec![
        "__CLASS",
        "__DERIVATION",
        "__DYNASTY",
        "__GENUS",
        "__NAMESPACE",
        "__PATH",
        "__PROPERTY_COUNT",
        "__RELPATH",
        "__SERVER",
        "__SUPERCLASS",
    ]
}

mod association;
pub(crate) fn system_property_order() -> [&'static str; 10] {
    [
        "__PATH",
        "__NAMESPACE",
        "__SERVER",
        "__DERIVATION",
        "__PROPERTY_COUNT",
        "__RELPATH",
        "__DYNASTY",
        "__SUPERCLASS",
        "__CLASS",
        "__GENUS",
    ]
}
