//! Contextual help is generated from alias/schema data and an en-US message catalog.
use super::*;
use crate::alias::{AliasDefinition, AliasParameter, AliasProperty, AliasQualifier, AliasStore};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};
#[derive(Deserialize)]
struct Messages {
    global_header: String,
    global_footer: String,
    get_header: String,
    get_footer: String,
    list_header: String,
    list_footer: String,
    call_header: String,
    method_heading: String,
    schema_get_header: String,
    switches: BTreeMap<String, String>,
    others: BTreeMap<String, String>,
}
fn messages() -> &'static Messages {
    static M: OnceLock<Messages> = OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/help-en-US.json"))
            .expect("embedded help catalog validated by tests")
    })
}
pub struct HelpRequest<'a> {
    pub command: &'a ParsedCommand,
    pub aliases: &'a AliasStore,
    pub schema: Option<&'a ClassSchema>,
}
fn line(out: &mut WideText, value: &str) {
    push(out, value);
    push(out, NL)
}
fn qualifier<'a>(qualifiers: &'a [AliasQualifier], name: &str) -> Option<&'a str> {
    qualifiers
        .iter()
        .find(|q| q.name.eq_ignore_ascii_case(name))
        .and_then(|q| q.qualifier_value.first())
        .map(String::as_str)
}
fn property_row(out: &mut WideText, p: &AliasProperty) {
    let ty = qualifier(&p.qualifiers, "CIMTYPE").unwrap_or("N/A");
    let read = qualifier(&p.qualifiers, "read");
    let write = qualifier(&p.qualifiers, "write");
    let op = match (read, write) {
        (None, None) => "N/A",
        (Some(r), Some(w)) if r.eq_ignore_ascii_case("true") && w.eq_ignore_ascii_case("true") => {
            "Read/Write"
        }
        (_, Some(v)) if v.eq_ignore_ascii_case("true") => "Write",
        _ => "Read",
    };
    line(out, &format!("{:<35}\t{:<20}\t{:<10}", p.name, ty, op));
}
fn full_properties(alias: &AliasDefinition) -> Vec<&AliasProperty> {
    let mut props: Vec<_> = alias
        .formats
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case("full"))
        .map(|f| f.properties.iter().collect())
        .unwrap_or_default();
    props.sort_by_key(|p| &p.name);
    props.dedup_by(|a, b| a.name == b.name);
    props
}
fn property_help(out: &mut WideText, alias: &AliasDefinition, command: &ParsedCommand) {
    let m = messages();
    let props = full_properties(alias);
    if command.properties.is_empty() && props.is_empty() {
        push(
            out,
            &m.get_header[..m.get_header.find("The following properties").unwrap()],
        );
        push(out, &m.get_footer[NL.len()..]);
        return;
    }
    if command.properties.is_empty() {
        push(out, &m.get_header);
        for p in props {
            property_row(out, p)
        }
    } else {
        push(out, NL);
        if command
            .properties
            .iter()
            .any(|name| props.iter().any(|p| name.eq_ascii_ignore_case(&p.name)))
        {
            push(
                out,
                &m.get_header[m.get_header.find("Property\t").unwrap()..],
            );
        }
        for name in &command.properties {
            if let Some(p) = props.iter().find(|p| name.eq_ascii_ignore_case(&p.name)) {
                property_row(out, p)
            } else {
                append(out, name);
                line(out, " - Invalid property(s).");
            }
        }
    }
    push(out, &m.get_footer);
}
fn parameter_text(p: &AliasParameter) -> String {
    let input = qualifier(&p.qualifiers, "in").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let output = qualifier(&p.qualifiers, "out").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    format!(
        "[{}]{}({})",
        match (input, output) {
            (true, true) => "IN/OUT",
            (false, true) => "OUT",
            _ => "IN ",
        },
        p.para_id.as_deref().unwrap_or(""),
        p.r#type.as_deref().unwrap_or("N/A")
    )
}
fn method_help(out: &mut WideText, alias: &AliasDefinition, command: &ParsedCommand) {
    let m = messages();
    if command.method.is_none() && alias.verbs.is_empty() {
        push(
            out,
            &m.call_header[..m.call_header.find("The following alias").unwrap()],
        );
        line(
            out,
            "User defined verb(s) or method(s) not available for the target WMI class.",
        );
        line(out, "");
        return;
    }
    if command.method.is_none() {
        push(out, &m.call_header)
    }
    push(out, &m.method_heading);
    let mut verbs: Vec<_> = alias
        .verbs
        .iter()
        .filter(|v| {
            command
                .method
                .as_ref()
                .is_none_or(|n| n.eq_ascii_ignore_case(&v.name))
        })
        .collect();
    verbs.sort_by_key(|v| &v.name);
    for verb in verbs {
        if verb.parameters.is_empty() {
            push(
                out,
                &format!("{:<24}\t\t\t\t\t{:<15}\r\n", verb.name, "(null)"),
            );
            push(out, NL);
            push(out, NL);
        } else {
            let mut params: Vec<_> = verb.parameters.iter().enumerate().collect();
            params.sort_by_key(|(i, _)| i.to_string());
            for (i, (_, p)) in params.into_iter().enumerate() {
                if i == 0 {
                    push(
                        out,
                        &format!(
                            "{:<24}{:<36}\t{:<15}\r\n",
                            verb.name,
                            parameter_text(p),
                            "(null)"
                        ),
                    );
                } else {
                    push(out, &format!("\t\t\t{:<36}\t\r\n", parameter_text(p)));
                }
                push(out, NL);
            }
        }
    }
    push(out, NL);
}
fn list_help(
    out: &mut WideText,
    alias: &AliasDefinition,
    command: &ParsedCommand,
    schema: Option<&ClassSchema>,
) {
    let m = messages();
    if command.view.is_some() {
        push(out, &m.list_footer);
        return;
    }
    push(out, &m.list_header);
    let mut formats: Vec<_> = alias.formats.iter().collect();
    formats.sort_by_key(|f| &f.name);
    formats.dedup_by(|a, b| a.name == b.name);
    for format in formats {
        let mut names: Vec<_> = format.properties.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            match format.name.to_ascii_lowercase().as_str() {
                "system" => {
                    names = super::system_property_names()
                        .into_iter()
                        .map(|n| n.to_owned())
                        .collect()
                }
                "instance" => names = vec!["__PATH".into()],
                "writeable" => {
                    names = if let Some(schema) = schema {
                        schema
                            .properties
                            .iter()
                            .filter(|p| {
                                p.qualifiers.iter().any(|q| {
                                    q.name.eq_ascii_ignore_case("write")
                                        && q.value == CimValue::Boolean(true)
                                })
                            })
                            .map(|p| p.name.to_string_lossy())
                            .collect()
                    } else {
                        vec![]
                    };
                    if names.is_empty() {
                        continue;
                    }
                }
                _ => {}
            }
        }
        line(out, &format!("{:<26}- {}", format.name, names.join(", ")));
    }
    push(out, &m.list_footer)
}
pub fn render_help(req: HelpRequest<'_>) -> Result<RenderedOutput, RenderError> {
    let m = messages();
    let c = req.command;
    let mut out = WideText::default();
    let Some(target) = &c.target else {
        if c.verb == Verb::Context {
            push(&mut out, &m.others["context"])
        } else if let Some(switch) = c.modifiers.iter().rev().find(|s| {
            s.scope == ModifierScope::Global
                && !s.name.eq_ascii_ignore_case("?")
                && s.value == SwitchValue::Missing
        }) {
            let name = switch.name.to_string_lossy().to_ascii_lowercase();
            if let Some(text) = m.switches.get(&name) {
                push(&mut out, text)
            }
        } else {
            push(&mut out, &m.global_header);
            let mut aliases: Vec<_> = req.aliases.definitions().iter().collect();
            aliases.sort_by_key(|a| a.friendly_name.to_ascii_uppercase());
            for alias in aliases {
                line(
                    &mut out,
                    &format!(
                        "{:<25}- {}",
                        alias.friendly_name.to_ascii_uppercase(),
                        alias
                            .localized_description
                            .as_deref()
                            .or(alias.description.as_deref())
                            .unwrap_or("")
                    ),
                );
            }
            push(&mut out, &m.global_footer);
        }
        return Ok(RenderedOutput {
            stdout: out,
            ..Default::default()
        });
    };
    match target {
        Target::Alias(name) => {
            let alias = req
                .aliases
                .find(name)
                .ok_or_else(|| RenderError("Alias not found.".into()))?;
            if !c.verb_explicit {
                let name = alias.friendly_name.to_ascii_uppercase();
                line(&mut out, "");
                line(
                    &mut out,
                    &format!(
                        "{name} - {}",
                        alias
                            .localized_description
                            .as_deref()
                            .or(alias.description.as_deref())
                            .unwrap_or("")
                    ),
                );
                line(&mut out, "");
                line(&mut out, "HINT: BNF for Alias usage.");
                line(
                    &mut out,
                    "(<alias> [WMIObject] | <alias> [<path where>] | [<alias>] <path where>) [<verb clause>].",
                );
                line(&mut out, "");
                line(&mut out, "USAGE:");
                line(&mut out, "");
                for usage in [
                    "ASSOC [<format specifier>]",
                    "CALL <method name> [<actual param list>]",
                    "CREATE <assign list>",
                    "DELETE",
                    "GET [<property list>] [<get switches>]",
                    "LIST [<list format>] [<list switches>]",
                    "SET [<assign list>]",
                ] {
                    if usage.starts_with("CALL ") && alias.verbs.is_empty() {
                        continue;
                    }
                    if usage.starts_with("SET ")
                        && !alias
                            .formats
                            .iter()
                            .any(|f| f.name.eq_ignore_ascii_case("writeable"))
                    {
                        continue;
                    }
                    line(&mut out, &format!("{name} {usage}"));
                }
                line(&mut out, "");
            } else {
                match c.verb {
                    Verb::Get => property_help(&mut out, alias, c),
                    Verb::List => list_help(&mut out, alias, c, req.schema),
                    Verb::Assoc => push(&mut out, &m.others["assoc"]),
                    Verb::Call => method_help(&mut out, alias, c),
                    _ => return Err(RenderError("Not supported".into())),
                }
            }
        }
        Target::Class(name) => {
            if c.verb == Verb::Get && c.verb_explicit {
                push(&mut out, &m.schema_get_header);
                push(&mut out, &m.get_footer)
            } else {
                push(
                    &mut out,
                    &m.others[if name.0.is_empty() {
                        "class"
                    } else {
                        "class_target"
                    }],
                )
            }
        }
        Target::Path(_) => {
            if c.verb_explicit && c.verb == Verb::Get {
                schema_property_help(&mut out, c, req.schema)
            } else {
                push(&mut out, &m.others["path"])
            }
        }
    }
    Ok(RenderedOutput {
        stdout: out,
        ..Default::default()
    })
}

fn schema_property_help(out: &mut WideText, command: &ParsedCommand, schema: Option<&ClassSchema>) {
    let m = messages();
    let mut props: Vec<_> = schema
        .map(|s| s.properties.iter().collect())
        .unwrap_or_default();
    props.sort_by_key(|p| &p.name);
    if command.properties.is_empty() {
        push(out, &m.get_header)
    } else {
        push(out, NL);
        if command.properties.iter().any(|name| {
            props
                .iter()
                .any(|p| name == &p.name || name.eq_ascii_ignore_case(&p.name.to_string_lossy()))
        }) {
            push(
                out,
                &m.get_header[m.get_header.find("Property\t").unwrap()..],
            );
        }
    }
    for p in props {
        if !command.properties.is_empty()
            && !command
                .properties
                .iter()
                .any(|name| name.eq_ascii_ignore_case(&p.name.to_string_lossy()))
        {
            continue;
        }
        let base = mof::cim_type_name(p.cim_type);
        let ty = if p.cim_type & 0x2000 != 0 {
            format!("array of {base}")
        } else {
            base.to_owned()
        };
        let read = p
            .qualifiers
            .iter()
            .find(|q| q.name.eq_ascii_ignore_case("read"))
            .map(|q| q.value == CimValue::Boolean(true))
            .unwrap_or(true);
        let write = p
            .qualifiers
            .iter()
            .any(|q| q.name.eq_ascii_ignore_case("write") && q.value == CimValue::Boolean(true));
        let operation = match (read, write) {
            (true, true) => "Read/Write",
            (false, true) => "Write",
            (true, false) => "Read",
            _ => "N/A",
        };
        line(
            out,
            &format!(
                "{:<35}\t{:<20}\t{:<10}",
                p.name.to_string_lossy(),
                ty,
                operation
            ),
        );
    }
    push(out, &m.get_footer);
}
