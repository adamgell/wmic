mod csv;
mod diagnostic;
mod encoding;
mod method;
mod operation;
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
pub struct RenderError {
    pub code: u32,
    pub message: WideText,
}
impl RenderError {
    pub fn new(message: impl Into<WideText>) -> Self {
        Self {
            code: 0xACB2,
            message: message.into(),
        }
    }
    pub fn status(code: u32, message: impl Into<WideText>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
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
                        .find(|p| p.name.eq_ignore_case(&name.to_string_lossy()))
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
                .position(|n| name.eq_ignore_case(n))
                .unwrap_or(10),
            name.to_string_lossy().to_ascii_lowercase(),
        )
    });
    cols
}
pub(crate) fn lookup<'a>(r: &'a Record, name: &WideText) -> Option<&'a CimValue> {
    r.properties
        .iter()
        .find(|p| p.name.eq_ignore_case(&name.to_string_lossy()))
        .map(|p| &p.value)
}
fn empty_output(
    req: &ResolvedRequest,
    node: &WideText,
    error: Option<&ManagementError>,
) -> Result<RenderedOutput, RenderError> {
    #[cfg(windows)]
    if let (Some(context), Some(spec)) = (&req.render_context, &req.format_spec) {
        let mut output = transform(&xml::error_command_xml(context, node, error)?, spec)?;
        if error.is_none() {
            output.stderr = "No Instance(s) Available.\r\r\n".into();
        }
        return Ok(output);
    }
    let _ = node;
    let explicit = req.modifiers.iter().rev().find_map(|m| {
        if m.name.eq_ignore_case("format") {
            if let SwitchValue::Value(v) = &m.value {
                Some(v)
            } else {
                None
            }
        } else {
            None
        }
    });
    let format = explicit.or_else(|| req.view.as_ref().and(req.format.as_ref()));
    let value = req.modifiers.iter().any(|m| m.name.eq_ignore_case("value"))
        || format.is_some_and(|f| {
            f.eq_ignore_case("list")
                || f.eq_ignore_case("value")
                || f.eq_ignore_case("wmiclivalueformat")
        });
    let stdout = if value {
        "\r\r\n\r\r\n\r\r\n"
    } else if format.is_some_and(|f| f.eq_ignore_case("csv")) {
        "\r\r\nNode,\r\r\n"
    } else if format.is_some_and(|f| f.eq_ignore_case("mof")) {
        "\r\r\n"
    } else {
        "\r\r\n\r\r\n"
    };
    Ok(RenderedOutput {
        stdout: stdout.into(),
        stderr: if error.is_none() {
            "No Instance(s) Available.\r\r\n".into()
        } else {
            WideText::default()
        },
        record: None,
    })
}
pub fn render(input: RenderRequest<'_>) -> Result<RenderedOutput, RenderError> {
    let req = input.request;
    let result = match input.result {
        Ok(r) => r,
        Err(e) => {
            if e.code == 0xC000013A {
                return match e.partial.as_deref() {
                    Some(partial)
                        if matches!(partial, ManagementResult::Methods(methods) if !methods.is_empty())
                            || matches!(partial, ManagementResult::Operation(operation) if !operation.paths.is_empty()) =>
                    {
                        render(RenderRequest {
                            request: req,
                            result: Ok(partial),
                            node: input.node,
                        })
                    }
                    _ => Ok(RenderedOutput::default()),
                };
            }
            if e.operation.eq_ignore_case("input") {
                return Ok(diagnostic::text(&e.message, None));
            }
            let mut output = diagnostic::management(e, input.node);
            if e.operation.eq_ignore_case("query")
                || e.operation.eq_ignore_case("enumerate")
                || e.operation.eq_ignore_case("convert")
            {
                output.stdout = empty_output(req, input.node, Some(e))?.stdout;
            }
            if e.operation.eq_ignore_case("method") {
                if !crate::backend::write::interactive(req) {
                    output.stdout =
                        method::start(&e.target, &req.method.clone().unwrap_or_default());
                }
                if let Some(partial) = &e.partial
                    && let ManagementResult::Methods(methods) = partial.as_ref()
                {
                    let mut completed = method::render(
                        methods,
                        &req.method.clone().unwrap_or_default(),
                        !crate::backend::write::interactive(req),
                    );
                    append(&mut completed.stdout, &output.stdout);
                    append(&mut completed.stderr, &output.stderr);
                    output = completed;
                }
            }
            if e.operation.eq_ignore_case("update") || e.operation.eq_ignore_case("delete") {
                if let Some(partial) = &e.partial
                    && let ManagementResult::Operation(operation) = partial.as_ref()
                {
                    output.stdout =
                        operation::render(operation, crate::backend::write::interactive(req))
                            .stdout;
                }
                if !crate::backend::write::interactive(req) || req.verb == Verb::Delete {
                    append(&mut output.stdout, &operation::start(req.verb, &e.target));
                }
            }
            return Ok(output);
        }
    };
    if let (Some(context), Some(spec)) = (&req.render_context, &req.format_spec) {
        let schema_default = matches!(result, ManagementResult::Schema(_))
            && !req
                .modifiers
                .iter()
                .any(|m| m.name.eq_ignore_case("format") || m.name.eq_ignore_case("translate"));
        let empty = matches!(result,ManagementResult::Objects(s) if s.objects.is_empty());
        if empty {
            return empty_output(req, input.node, None);
        }
        let association_default = matches!(result, ManagementResult::Associations(_))
            && !req
                .modifiers
                .iter()
                .any(|m| m.name.eq_ignore_case("format") || m.name.eq_ignore_case("translate"));
        if !schema_default && !empty && !association_default {
            let narrowed;
            let result = if let ManagementResult::Objects(set) = result {
                narrowed = ManagementResult::Objects(restrict::selected(req, set));
                &narrowed
            } else {
                result
            };
            return transform(&to_command_xml(result, context, input.node)?, spec);
        }
    }
    if let ManagementResult::Operation(operation) = result {
        return Ok(operation::render(
            operation,
            crate::backend::write::interactive(req),
        ));
    }
    if let ManagementResult::Methods(methods) = result {
        return Ok(method::render(
            methods,
            &req.method.clone().unwrap_or_default(),
            !crate::backend::write::interactive(req),
        ));
    }
    if let ManagementResult::Schema(schema) = result {
        let value = req.modifiers.iter().any(|m| m.name.eq_ignore_case("value"));
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
        return Err(RenderError::new("Not supported"));
    };
    if set.objects.is_empty() {
        return empty_output(req, input.node, None);
    }
    let cols = columns(req, &set.objects);
    let format = req
        .modifiers
        .iter()
        .filter(|m| m.name.eq_ignore_case("format"))
        .filter_map(|m| {
            if let SwitchValue::Value(v) = &m.value {
                Some(v)
            } else {
                None
            }
        })
        .next_back();
    let list = req.modifiers.iter().any(|m| m.name.eq_ignore_case("value"))
        || format.is_some_and(|f| f.eq_ignore_case("list"))
        || (req.view.is_some()
            && req
                .format
                .as_ref()
                .is_some_and(|f| f.eq_ignore_case("list")));
    if let Some(f) = format {
        if f.eq_ignore_case("csv") {
            return Ok(RenderedOutput {
                stdout: csv::render(&set.objects, &cols, input.node),
                ..Default::default()
            });
        }
        if f.eq_ignore_case("mof") {
            return Ok(RenderedOutput {
                stdout: mof::instances(&set.objects, &cols),
                ..Default::default()
            });
        }
    }
    if let Some(f) = format
        && !f.eq_ignore_case("list")
        && !f.eq_ignore_case("table")
    {
        return Err(RenderError::new("Invalid XSL format (or) file name."));
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
    render_resolve_error_for(e, &WideText::default())
}
/// Withholds the error token when the command line carries a password switch.
pub fn render_resolve_error_for(
    e: &crate::alias::ResolveError,
    command_line: &WideText,
) -> RenderedOutput {
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
    let secret = command_line
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("/password");
    diagnostic::text(&e.message, e.token.as_ref().filter(|_| !secret))
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
mod restrict;
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

mod xml;
pub use xml::{to_command_xml, to_wmi_xml};

mod builtin;
pub use builtin::{BuiltinKind, builtin_kind, builtin_source, logical_format_name};

pub mod native;

mod pipeline;
pub use pipeline::*;
