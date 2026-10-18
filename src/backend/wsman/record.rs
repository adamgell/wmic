//! Typed records from wire instances. Types come from the embedded schema when it knows the
//! class; any other class reads every property as text.
use super::{
    catalog,
    convert::{
        is_cim_datetime, text_value, xs_date_to_cim, xs_datetime_to_cim, xs_duration_to_cim,
        xs_time_to_cim,
    },
    parse::{Epr, RawField, RawInstance, RawValue},
    path::quoted,
    schema::ClassInfo,
};
use crate::model::{
    CimValue, ClassSchema, ParameterSchema, Property, PropertySchema, Qualifier, Record,
};

/// Where a record came from.
#[derive(Clone, Copy, Debug)]
pub struct RecordContext<'a> {
    /// Value of `__SERVER`.
    pub server: &'a str,
    /// Namespace the request addressed, for example `root\cimv2`.
    pub namespace: &'a str,
}

const ARRAY: u32 = 0x2000;

fn infer_type(value: &RawValue) -> u32 {
    match value {
        RawValue::DateTime(_)
        | RawValue::Interval(_)
        | RawValue::Date(_)
        | RawValue::Time(_)
        | RawValue::CimDateTime(_) => 101,
        RawValue::Reference(_) => 102,
        RawValue::Object(_) => 13,
        RawValue::Nil | RawValue::Text(_) => 8,
    }
}

/// Namespace with backslashes, taking the first of the endpoint reference, the instance and
/// the request.
fn namespace_text(epr: Option<&Epr>, raw: Option<&RawInstance>, ctx: &RecordContext) -> String {
    epr.and_then(Epr::namespace)
        .or_else(|| raw.and_then(|r| r.namespace.clone()))
        .map_or_else(
            || ctx.namespace.trim_matches('\\').to_owned(),
            |n| n.replace('/', "\\"),
        )
}

/// Key literal for a selector value, quoted by the key property's type.
fn key_literal_for(info: Option<&ClassInfo>, name: &str, value: &str) -> (String, String) {
    let Some((_, property)) = info.and_then(|i| i.property(name)) else {
        return (name.to_owned(), quoted(value));
    };
    let literal = match property.cim_type & 0xfff {
        11 => {
            if value.eq_ignore_ascii_case("true") || value == "1" {
                "TRUE".to_owned()
            } else {
                "FALSE".to_owned()
            }
        }
        2 | 3 | 16..=21 | 103 if value.parse::<i128>().is_ok() => value.to_owned(),
        _ => quoted(value),
    };
    (property.name.clone(), literal)
}

/// Relative object path: `Class.Key="v",Key2=5`, or `Class=@` without keys. Keys sort by name.
/// A reference key holds the quoted full path of the instance it names.
pub fn relative_path(
    class: &str,
    selectors: &[(String, String)],
    references: &[(String, Epr)],
    info: Option<&ClassInfo>,
    ctx: &RecordContext,
) -> String {
    if selectors.is_empty() && references.is_empty() {
        return format!("{class}=@");
    }
    let mut keys: Vec<(String, String)> = selectors
        .iter()
        .map(|(name, value)| key_literal_for(info, name, value))
        .collect();
    keys.extend(references.iter().map(|(name, nested)| {
        let name = info
            .and_then(|i| i.property(name))
            .map_or(name.as_str(), |(_, p)| p.name.as_str());
        (name.to_owned(), quoted(&full_path(nested, ctx)))
    }));
    keys.sort_by_key(|(name, _)| name.to_ascii_lowercase());
    let parts: Vec<String> = keys.iter().map(|(n, l)| format!("{n}={l}")).collect();
    format!("{class}.{}", parts.join(","))
}

/// Full object path of an instance reached by endpoint reference.
pub fn full_path(epr: &Epr, ctx: &RecordContext) -> String {
    let class = epr.class();
    let info = catalog::lookup(class);
    let class = info.as_ref().map_or(class, |i| i.name.as_str());
    format!(
        "\\\\{}\\{}:{}",
        ctx.server,
        namespace_text(Some(epr), None, ctx).to_uppercase(),
        relative_path(class, &epr.selectors, &epr.references, info.as_deref(), ctx)
    )
}

fn fallback_text(value: &RawValue, ctx: &RecordContext) -> CimValue {
    match value {
        RawValue::Nil => CimValue::Null,
        RawValue::Text(t)
        | RawValue::DateTime(t)
        | RawValue::Interval(t)
        | RawValue::Date(t)
        | RawValue::Time(t)
        | RawValue::CimDateTime(t) => CimValue::String(t.as_str().into()),
        RawValue::Reference(epr) => CimValue::String(full_path(epr, ctx).into()),
        RawValue::Object(_) => CimValue::String("".into()),
    }
}

fn scalar(value: &RawValue, base: u32, ctx: &RecordContext) -> Option<CimValue> {
    Some(match (value, base) {
        (RawValue::Nil, _) => CimValue::Null,
        (RawValue::Text(t), 101) => {
            let cim = if is_cim_datetime(t) {
                t.clone()
            } else {
                xs_datetime_to_cim(t)?
            };
            CimValue::DateTime(cim.as_str().into())
        }
        (RawValue::Text(t), 102) => CimValue::Reference(t.as_str().into()),
        (RawValue::Text(t), _) => text_value(t, base)?,
        (RawValue::DateTime(t), 101) => CimValue::DateTime(xs_datetime_to_cim(t)?.into()),
        (RawValue::Interval(t), 101) => CimValue::DateTime(xs_duration_to_cim(t)?.into()),
        (RawValue::CimDateTime(t), 101) => CimValue::DateTime(t.as_str().into()),
        (RawValue::Date(t), 101) => CimValue::DateTime(xs_date_to_cim(t)?.into()),
        (RawValue::Time(t), 101) => CimValue::DateTime(xs_time_to_cim(t)?.into()),
        (RawValue::Reference(epr), 102) => CimValue::Reference(full_path(epr, ctx).into()),
        (RawValue::Object(instance), 13) => CimValue::Object(Box::new(embedded(instance, ctx))),
        _ => return None,
    })
}

/// Value and type of one property from its elements. A value that does not fit the declared
/// type is kept as text with the string type.
fn field_value(
    values: &[&RawValue],
    declared: Option<u32>,
    ctx: &RecordContext,
) -> (CimValue, u32) {
    let array = values.len() > 1 || declared.is_some_and(|t| t & ARRAY != 0);
    let flag = if array { ARRAY } else { 0 };
    let cim_type = declared.map_or_else(|| infer_type(values[0]) | flag, |t| t | flag);
    if let [RawValue::Nil] = values {
        return (CimValue::Null, cim_type);
    }
    let converted: Option<Vec<CimValue>> = values
        .iter()
        .map(|v| scalar(v, cim_type & 0xfff, ctx))
        .collect();
    match converted {
        Some(items) if array => (CimValue::Array(items), cim_type),
        Some(items) => (items.into_iter().next().unwrap_or(CimValue::Null), cim_type),
        None => {
            let mut items: Vec<CimValue> = values.iter().map(|v| fallback_text(v, ctx)).collect();
            if array {
                (CimValue::Array(items), 8 | ARRAY)
            } else {
                (items.pop().unwrap_or(CimValue::Null), 8)
            }
        }
    }
}

/// Groups repeated elements by name in first-seen order.
fn group(fields: &[RawField]) -> Vec<(&str, Vec<&RawValue>)> {
    let mut groups: Vec<(&str, Vec<&RawValue>)> = vec![];
    for field in fields {
        match groups
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(&field.name))
        {
            Some((_, values)) => values.push(&field.value),
            None => groups.push((field.name.as_str(), vec![&field.value])),
        }
    }
    groups
}

/// Properties in schema order, then properties the schema does not know in wire order.
fn properties(raw: &RawInstance, info: Option<&ClassInfo>, ctx: &RecordContext) -> Vec<Property> {
    let mut ordered: Vec<(usize, Property)> = group(&raw.fields)
        .into_iter()
        .map(|(wire_name, values)| {
            let known = info.and_then(|i| i.property(wire_name));
            let (index, name, declared) = match known {
                Some((index, p)) => (index, p.name.as_str(), Some(p.cim_type)),
                None => (usize::MAX, wire_name, None),
            };
            let (value, cim_type) = field_value(&values, declared, ctx);
            (
                index,
                Property {
                    name: name.into(),
                    value,
                    cim_type,
                    system: false,
                    qualifiers: vec![],
                },
            )
        })
        .collect();
    ordered.sort_by_key(|(index, _)| *index);
    ordered.into_iter().map(|(_, p)| p).collect()
}

/// An embedded object: its properties and class, without a path or system properties.
fn embedded(raw: &RawInstance, ctx: &RecordContext) -> Record {
    let info = catalog::lookup(&raw.class);
    Record {
        class: info
            .as_ref()
            .map_or(raw.class.as_str(), |i| i.name.as_str())
            .into(),
        properties: properties(raw, info.as_deref(), ctx),
        ..Default::default()
    }
}

fn system(name: &str, value: CimValue, cim_type: u32) -> Property {
    Property {
        name: name.into(),
        value,
        cim_type,
        system: true,
        qualifiers: vec![],
    }
}

/// Key selectors read from converted key properties, for items that arrive without a
/// reference. `None` when the class is unknown or a key is absent.
fn selectors_from_keys(
    info: Option<&ClassInfo>,
    properties: &[Property],
) -> Option<Vec<(String, String)>> {
    let info = info?;
    let keys = info.keys();
    if keys.is_empty() {
        return None;
    }
    keys.into_iter()
        .map(|i| {
            let key = &info.properties[i];
            let property = properties
                .iter()
                .find(|p| p.name.eq_ignore_case(&key.name))?;
            Some((
                key.name.clone(),
                super::convert::value_text(&property.value)?,
            ))
        })
        .collect()
}

/// A record with the ten system properties. `class_hint` names the class when neither the
/// instance nor the reference does, as for projections.
pub fn build(
    raw: &RawInstance,
    epr: Option<&Epr>,
    ctx: &RecordContext,
    class_hint: &str,
) -> Record {
    let wire_class = if !raw.class.is_empty() {
        raw.class.as_str()
    } else {
        epr.map_or(class_hint, Epr::class)
    };
    let info = catalog::lookup(wire_class);
    let class = info.as_ref().map_or(wire_class, |i| i.name.as_str());
    let values = properties(raw, info.as_deref(), ctx);
    let (selectors, references) = match epr {
        Some(epr) => (Some(epr.selectors.clone()), epr.references.as_slice()),
        None => (selectors_from_keys(info.as_deref(), &values), &[][..]),
    };
    let namespace = namespace_text(epr, Some(raw), ctx);
    let (path, relpath) = match &selectors {
        Some(selectors) => {
            let relpath = relative_path(class, selectors, references, info.as_deref(), ctx);
            (
                Some(format!(
                    "\\\\{}\\{}:{relpath}",
                    ctx.server,
                    namespace.to_uppercase()
                )),
                relpath,
            )
        }
        None => (None, format!("{class}=@")),
    };
    let text = |s: &str| CimValue::String(s.into());
    let (derivation, dynasty, superclass, count) = match &info {
        Some(info) => (
            info.derivation.clone(),
            info.dynasty.clone(),
            info.superclass.clone(),
            info.properties.len(),
        ),
        None => (vec![], class.to_owned(), None, values.len()),
    };
    let mut all = vec![
        system("__PATH", path.as_deref().map_or(CimValue::Null, text), 8),
        system("__NAMESPACE", text(&namespace.to_uppercase()), 8),
        system("__SERVER", text(ctx.server), 8),
        system(
            "__DERIVATION",
            CimValue::Array(derivation.iter().map(|d| text(d)).collect()),
            8 | ARRAY,
        ),
        system("__PROPERTY_COUNT", CimValue::Sint32(count as i32), 3),
        system("__RELPATH", text(&relpath), 8),
        system("__DYNASTY", text(&dynasty), 8),
        system(
            "__SUPERCLASS",
            superclass.as_deref().map_or(CimValue::Null, text),
            8,
        ),
        system("__CLASS", text(class), 8),
        system("__GENUS", CimValue::Sint32(2), 3),
    ];
    all.extend(values);
    Record {
        result_node: None,
        wmi_xml: None,
        class: class.into(),
        path: path.map(Into::into),
        server: Some(ctx.server.into()),
        properties: all,
    }
}

/// Method output parameters typed by the method's declared outputs. Outputs the schema does
/// not declare read as text; `ReturnValue` reads as uint32.
pub fn parameters(
    raw: &RawInstance,
    declared: &[ParameterSchema],
    ctx: &RecordContext,
) -> Vec<Property> {
    group(&raw.fields)
        .into_iter()
        .map(|(wire_name, values)| {
            let known = declared.iter().find(|p| p.name.eq_ignore_case(wire_name));
            let cim_type = known
                .map(|p| p.cim_type)
                .or_else(|| wire_name.eq_ignore_ascii_case("ReturnValue").then_some(19));
            let (value, cim_type) = field_value(&values, cim_type, ctx);
            Property {
                name: known.map_or_else(|| wire_name.into(), |p| p.name.clone()),
                value,
                cim_type,
                system: false,
                qualifiers: vec![],
            }
        })
        .collect()
}

/// Class definition read off the first instance of a class the embedded schema lacks.
pub fn infer_schema(class: &str, raw: &RawInstance) -> ClassSchema {
    let properties = group(&raw.fields)
        .into_iter()
        .map(|(name, values)| {
            let flag = if values.len() > 1 { ARRAY } else { 0 };
            PropertySchema {
                name: name.into(),
                cim_type: infer_type(values[0]) | flag,
                qualifiers: vec![Qualifier {
                    name: "read".into(),
                    value: CimValue::Boolean(true),
                }],
            }
        })
        .collect();
    ClassSchema {
        wmi_xml: None,
        name: class.into(),
        properties,
        methods: vec![],
        qualifiers: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(name: &str, value: &str) -> RawField {
        RawField {
            name: name.into(),
            value: RawValue::Text(value.into()),
        }
    }

    fn ctx() -> RecordContext<'static> {
        RecordContext {
            server: "SRV",
            namespace: "root\\cimv2",
        }
    }

    #[test]
    fn relative_paths_quote_by_key_type_and_sort_keys() {
        let selectors = vec![
            ("Name".to_owned(), "a\"b\\c".to_owned()),
            ("Handle".to_owned(), "4".to_owned()),
        ];
        let process = catalog::lookup("win32_process");
        assert_eq!(
            relative_path("Win32_Process", &selectors, &[], process.as_deref(), &ctx()),
            "Win32_Process.Handle=\"4\",Name=\"a\\\"b\\\\c\""
        );
        assert_eq!(
            relative_path("Unknown", &[], &[], None, &ctx()),
            "Unknown=@"
        );
    }

    #[test]
    fn unknown_classes_read_as_strings_with_system_properties() {
        let raw = RawInstance {
            class: "Vendor_Widget".into(),
            namespace: None,
            fragment: false,
            fields: vec![text("Id", "7"), text("Id", "8"), text("Name", "w")],
        };
        let epr = Epr {
            resource_uri: "http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Vendor_Widget"
                .into(),
            selectors: vec![("Id".into(), "7".into())],
            references: vec![],
        };
        let record = build(&raw, Some(&epr), &ctx(), "");
        let plain: Vec<_> = record.properties.iter().filter(|p| !p.system).collect();
        assert_eq!(plain.len(), 2);
        assert_eq!(plain[0].cim_type, 8 | ARRAY);
        assert_eq!(plain[1].cim_type, 8);
        assert_eq!(record.properties.iter().filter(|p| p.system).count(), 10);
        assert_eq!(
            record.path.unwrap().to_string_lossy(),
            "\\\\SRV\\ROOT\\CIMV2:Vendor_Widget.Id=\"7\""
        );
    }
}
