//! Contextual help is generated from alias/schema data and an en-US message catalog.
use super::*;
use crate::alias::{AliasDefinition, AliasParameter, AliasProperty, AliasQualifier, AliasStore};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};
#[derive(Deserialize)]
struct Messages {
    verb_switches: BTreeMap<String, String>,
    set_header: String,
    create_header: String,
    delete_alias: String,
    delete_class: String,
    schema_call_header: String,
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
            .any(|name| props.iter().any(|p| name.eq_ignore_case(&p.name)))
        {
            push(
                out,
                &m.get_header[m.get_header.find("Property\t").unwrap()..],
            );
        }
        for name in &command.properties {
            if let Some(p) = props.iter().find(|p| name.eq_ignore_case(&p.name)) {
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
                .is_none_or(|n| n.eq_ignore_case(&v.name))
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
                                    q.name.eq_ignore_case("write")
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
    if let Some(at) = c
        .modifiers
        .iter()
        .position(|m| m.name.eq_ignore_case("repeat"))
        && !c.modifiers[..at]
            .iter()
            .any(|m| m.name.eq_ignore_case("every"))
    {
        return Err(RenderError::status(
            0xB0A6,
            "HINT: /REPEAT should always follow /EVERY switch.",
        ));
    }
    if c.help
        && let Some(modifier) = c
            .modifiers
            .iter()
            .rev()
            .find(|v| v.scope == ModifierScope::Verb && !v.name.eq_ignore_case("?"))
    {
        let key = modifier.name.to_string_lossy().to_ascii_lowercase();
        if key == "nointeractive" {
            return Err(RenderError::status(0xABE5, "Invalid command."));
        }
        if let Some(text) = m.verb_switches.get(&key) {
            return Ok(RenderedOutput {
                stdout: text.as_str().into(),
                ..Default::default()
            });
        }
    }
    let Some(target) = &c.target else {
        if c.verb == Verb::Context {
            push(&mut out, &m.others["context"])
        } else if let Some(switch) = c.modifiers.iter().rev().find(|s| {
            s.scope == ModifierScope::Global
                && !s.name.eq_ignore_case("?")
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
                let description = alias
                    .localized_description
                    .as_deref()
                    .or(alias.description.as_deref())
                    .unwrap_or("");
                let brief = c.modifiers.iter().any(|m| {
                    m.name.eq_ignore_case("?")
                        && matches!(&m.value,SwitchValue::Value(v) if v.eq_ignore_case("brief"))
                });
                let mut description: WideText = description.into();
                if brief && description.0.len() > 48 {
                    description.0.truncate(48);
                    description.0.extend("...".encode_utf16());
                }
                line(
                    &mut out,
                    &format!(
                        "{:<25}- {}",
                        alias.friendly_name.to_ascii_uppercase(),
                        description
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
                .ok_or_else(|| RenderError::new("Alias not found."))?;
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
                    Verb::Set | Verb::Create => {
                        write_help(&mut out, Some(alias), req.schema, c.verb)
                    }
                    Verb::Delete => push(&mut out, &m.delete_alias),
                    _ => return Err(RenderError::new("Not supported")),
                }
            }
        }
        Target::Class(name) => {
            if c.verb == Verb::Set || c.verb == Verb::Create {
                write_help(&mut out, None, req.schema, c.verb)
            } else if c.verb == Verb::Delete {
                push(&mut out, &m.delete_class)
            } else if c.verb == Verb::Call {
                schema_method_help(&mut out, c, req.schema)
            } else if c.verb == Verb::Get && c.verb_explicit {
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
            if c.verb == Verb::Set || c.verb == Verb::Create {
                write_help(&mut out, None, req.schema, c.verb)
            } else if c.verb == Verb::Delete {
                push(&mut out, &m.delete_alias)
            } else if c.verb == Verb::Call {
                schema_method_help(&mut out, c, req.schema)
            } else if c.verb_explicit && c.verb == Verb::Get {
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
                .any(|p| name == &p.name || name.eq_ignore_case(&p.name.to_string_lossy()))
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
                .any(|name| name.eq_ignore_case(&p.name.to_string_lossy()))
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
            .find(|q| q.name.eq_ignore_case("read"))
            .map(|q| q.value == CimValue::Boolean(true))
            .unwrap_or(true);
        let write = p
            .qualifiers
            .iter()
            .any(|q| q.name.eq_ignore_case("write") && q.value == CimValue::Boolean(true));
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

fn schema_property(property: &PropertySchema) -> AliasProperty {
    let ty = if property.cim_type & 0x2000 != 0 {
        format!("array of {}", mof::cim_type_name(property.cim_type))
    } else {
        mof::cim_type_name(property.cim_type).to_owned()
    };
    let write = property
        .qualifiers
        .iter()
        .any(|q| q.name.eq_ignore_case("write") && q.value == CimValue::Boolean(true));
    AliasProperty {
        name: property.name.to_string_lossy(),
        derivation: None,
        description: None,
        qualifiers: vec![
            AliasQualifier {
                name: "CIMTYPE".into(),
                qualifier_value: vec![ty],
            },
            AliasQualifier {
                name: "read".into(),
                qualifier_value: vec!["true".into()],
            },
            AliasQualifier {
                name: "write".into(),
                qualifier_value: vec![write.to_string()],
            },
        ],
    }
}
fn write_help(
    out: &mut WideText,
    alias: Option<&AliasDefinition>,
    schema: Option<&ClassSchema>,
    verb: Verb,
) {
    let m = messages();
    let props: Vec<AliasProperty> = if let Some(alias) = alias {
        if verb == Verb::Create {
            full_properties(alias).into_iter().cloned().collect()
        } else {
            alias
                .formats
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case("writeable"))
                .map(|f| f.properties.clone())
                .unwrap_or_default()
        }
    } else {
        schema
            .map(|s| {
                s.properties
                    .iter()
                    .filter(|p| {
                        verb == Verb::Create
                            || p.qualifiers.iter().any(|q| {
                                q.name.eq_ignore_case("write") && q.value == CimValue::Boolean(true)
                            })
                    })
                    .map(schema_property)
                    .collect()
            })
            .unwrap_or_default()
    };
    let header = if verb == Verb::Create {
        &m.create_header
    } else {
        &m.set_header
    };
    if props.is_empty() {
        push(out, &header[..header.find("The following").unwrap()]);
        if verb == Verb::Set {
            line(
                out,
                "INFO: Writeable properties are not available for this alias or class.",
            );
            line(out, "");
        }
        return;
    }
    push(out, header);
    let mut props = props.iter().collect::<Vec<_>>();
    props.sort_by_key(|p| &p.name);
    props.dedup_by(|a, b| a.name == b.name);
    for property in props {
        property_row(out, property)
    }
    line(out, "");
}
fn schema_method_help(out: &mut WideText, command: &ParsedCommand, schema: Option<&ClassSchema>) {
    let m = messages();
    if schema.is_none_or(|s| s.methods.is_empty()) && command.method.is_none() {
        push(
            out,
            &m.schema_call_header[..m.schema_call_header.find("The following").unwrap()],
        );
        line(
            out,
            "User defined verb(s) or method(s) not available for the target WMI class.",
        );
        line(out, "");
        return;
    }
    if command.method.is_none() {
        push(out, &m.schema_call_header)
    }
    push(out, &m.method_heading);
    let mut methods = schema
        .map(|s| s.methods.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    methods.sort_by_key(|m| &m.name);
    for method in methods.into_iter().filter(|m| {
        command
            .method
            .as_ref()
            .is_none_or(|n| n.eq_ordinal_ignore_case(&m.name))
    }) {
        let status = method
            .qualifiers
            .iter()
            .find(|q| q.name.eq_ignore_case("implemented"))
            .map_or("Not Found", |q| {
                if q.value == CimValue::Boolean(true) {
                    "Implemented"
                } else {
                    "Not Implemented"
                }
            });
        let mut input_names = method
            .input
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>();
        input_names.sort();
        input_names.dedup();
        let mut all_names = method
            .input
            .iter()
            .chain(&method.output)
            .map(|p| p.name.clone())
            .collect::<Vec<_>>();
        all_names.sort();
        all_names.dedup();
        let mut indices = (0..input_names.len()).collect::<Vec<_>>();
        indices.sort_by_key(usize::to_string);
        let mut names = indices
            .into_iter()
            .map(|i| input_names[i].clone())
            .collect::<Vec<_>>();
        let mut outputs = method
            .output
            .iter()
            .map(|p| p.name.clone())
            .filter(|n| !input_names.contains(n))
            .collect::<Vec<_>>();
        outputs.sort();
        outputs.dedup();
        for name in outputs {
            let at = (all_names.iter().position(|n| n == &name).unwrap() + 1).min(names.len());
            names.insert(at, name);
        }
        let mut params = vec![];
        for name in names {
            let input = method.input.iter().find(|p| p.name == name);
            let output = method.output.iter().find(|p| p.name == name);
            let ty = input.or(output).unwrap().cim_type;
            let ty = if ty & 0x2000 != 0 {
                format!("array of {}", mof::cim_type_name(ty))
            } else if ty & 0xfff == 101 {
                "DateTime".into()
            } else {
                input
                    .or(output)
                    .and_then(|p| p.cim_type_name.as_ref())
                    .filter(|_| ty & 0xfff == 13)
                    .map(WideText::to_string_lossy)
                    .unwrap_or_else(|| mof::cim_type_name(ty).to_owned())
            };
            params.push(format!(
                "[{}]{}({ty})",
                match (input.is_some(), output.is_some()) {
                    (true, true) => "IN/OUT",
                    (false, true) => "OUT",
                    _ => "IN ",
                },
                name
            ));
        }
        let indices = 0..params.len();
        if params.is_empty() {
            push(
                out,
                &format!(
                    "{:<24}\t\t\t\t\t{status:<15}\r\n",
                    method.name.to_string_lossy()
                ),
            );
            push(out, NL);
            push(out, NL)
        } else {
            for (row, index) in indices.into_iter().enumerate() {
                if row == 0 {
                    push(
                        out,
                        &format!(
                            "{:<24}{:<36}\t{status:<15}\r\n",
                            method.name.to_string_lossy(),
                            params[index]
                        ),
                    );
                } else {
                    push(out, &format!("\t\t\t{:<36}\t\r\n", params[index]));
                }
                push(out, NL);
            }
        }
    }
    push(out, NL);
}
