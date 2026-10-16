use super::*;
use crate::{
    alias::{AliasDefinition, ResolveError},
    backend::{ConnectionOptions, ResolvedRequest},
};
use serde_json::{Map, Value};
fn resolve_error(code: u32) -> ResolveError {
    ResolveError {
        code,
        message: diagnostic(code).into(),
        token: None,
    }
}
fn json_text(text: &WideText) -> Result<String, ResolveError> {
    String::from_utf16(&text.0).map_err(|_| resolve_error(0x80041008))
}
fn object(record: &Record) -> Result<Value, ResolveError> {
    let mut map = Map::new();
    for p in record.properties.iter().filter(|p| !p.system) {
        map.insert(json_text(&p.name)?, value(&p.value)?);
    }
    Ok(Value::Object(map))
}
fn value(v: &CimValue) -> Result<Value, ResolveError> {
    Ok(match v {
        CimValue::Empty | CimValue::Null => Value::Null,
        CimValue::String(s) | CimValue::Reference(s) | CimValue::DateTime(s) => {
            Value::String(json_text(s)?)
        }
        CimValue::Boolean(v) => Value::Bool(*v),
        CimValue::Sint8(v) => Value::from(*v),
        CimValue::Uint8(v) => Value::from(*v),
        CimValue::Sint16(v) => Value::from(*v),
        CimValue::Uint16(v) => Value::from(*v),
        CimValue::Sint32(v) => Value::from(*v),
        CimValue::Uint32(v) => Value::from(*v),
        CimValue::Sint64(v) => Value::from(*v),
        CimValue::Uint64(v) => Value::from(*v),
        CimValue::Object(r) => object(r)?,
        CimValue::Array(a) => Value::Array(a.iter().map(value).collect::<Result<_, _>>()?),
        _ => return Err(resolve_error(0x80041008)),
    })
}
fn request(options: ConnectionOptions, class: &str) -> ResolvedRequest {
    ResolvedRequest {
        render_context: None,
        format_spec: None,
        connection: options,
        class_name: class.into(),
        instance_path: None,
        class_mode: false,
        query: None,
        selector: None,
        verb: Verb::Get,
        properties: vec![],
        method: None,
        arguments: vec![],
        modifiers: vec![],
        view: None,
        display: vec![],
        format: None,
        association: None,
    }
}
fn literal(value: &WideText) -> WideText {
    let mut out: WideText = "'".into();
    for u in &value.0 {
        if *u == 39 || *u == 92 {
            out.0.push(92)
        }
        out.0.push(*u)
    }
    out.0.push(39);
    out
}
pub fn load(
    role: &WideText,
    selected: Option<&WideText>,
    locale: &WideText,
) -> Result<Option<Vec<AliasDefinition>>, ResolveError> {
    load_impl(role, selected, locale, false)
}
pub fn inventory(
    role: &WideText,
    locale: &WideText,
) -> Result<Option<Vec<AliasDefinition>>, ResolveError> {
    load_impl(role, None, locale, true)
}
fn load_impl(
    role: &WideText,
    selected: Option<&WideText>,
    locale: &WideText,
    inventory: bool,
) -> Result<Option<Vec<AliasDefinition>>, ResolveError> {
    let options = ConnectionOptions {
        namespace: role.clone(),
        locale: Some(locale.clone()),
        ..Default::default()
    };
    let svc = match connect::connect(&options) {
        Ok(s) => s,
        Err(e) if e.code().0 as u32 == 0x8004100e => return Ok(None),
        Err(e) => return Err(resolve_error(e.code().0 as u32)),
    };
    match get_object(&svc, &"MSFT_CliAlias".into()) {
        Ok(_) => {}
        Err(e) if matches!(e.code().0 as u32, 0x80041002 | 0x80041010) => return Ok(None),
        Err(e) => return Err(resolve_error(e.code().0 as u32)),
    }
    let r = request(options.clone(), "MSFT_CliAlias");
    let mut wql: WideText = if inventory {
        "SELECT Connection,Description,FriendlyName,PWhere,Target FROM MSFT_CliAlias"
    } else {
        "SELECT * FROM MSFT_CliAlias"
    }
    .into();
    if let Some(selected) = selected {
        wql.0.extend(" WHERE FriendlyName=".encode_utf16());
        wql.0.extend(literal(selected).0)
    }
    let definitions = query::query_text(&svc, &r, &wql).map_err(|e| resolve_error(e.code))?;
    let mut localized_ns = role.clone();
    localized_ns.0.push(92);
    localized_ns.0.extend(&locale.0);
    let localized_options = ConnectionOptions {
        namespace: localized_ns,
        locale: Some(locale.clone()),
        ..Default::default()
    };
    let localized =
        connect::connect(&localized_options).map_err(|e| resolve_error(e.code().0 as u32))?;
    let localization = query::query_text(
        &localized,
        &request(localized_options, "MSFT_LocalizablePropertyValue"),
        &"SELECT * FROM MSFT_LocalizablePropertyValue WHERE PropertyName='Description' AND ObjectLocator=''".into(),
    )
    .map_err(|e| resolve_error(e.code))?;
    let mut aliases = vec![];
    for record in definitions.objects {
        let mut wire = object(&record)?;
        let mut alias: AliasDefinition =
            serde_json::from_value(wire.clone()).map_err(|_| resolve_error(0x80041008))?;
        let relpath = format!("MSFT_CliAlias.FriendlyName=\"{}\"", alias.friendly_name);
        for entry in &localization.objects {
            let rel = entry
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_case("RelPath"));
            let text = entry
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_case("Text"));
            if rel.is_some_and(
                |p| matches!(&p.value,CimValue::String(s)if s.eq_ignore_case(&relpath)),
            ) && let Some(Property {
                value: CimValue::Array(values),
                ..
            }) = text
                && let Some(CimValue::String(s)) = values.first()
            {
                alias.localized_description = Some(json_text(s)?);
            }
        }
        // The transient JSON is never logged or persisted; passwords enter SecretWide.
        if let Value::Object(map) = &mut wire {
            map.clear();
        }
        aliases.push(alias);
    }
    Ok(Some(aliases))
}

pub fn translation(
    role: &WideText,
    name: &WideText,
) -> Result<Option<Vec<(WideText, WideText)>>, ResolveError> {
    let options = ConnectionOptions {
        namespace: role.clone(),
        ..Default::default()
    };
    let svc = match connect::connect(&options) {
        Ok(s) => s,
        Err(e) if e.code().0 as u32 == 0x8004100e => return Ok(None),
        Err(e) => return Err(resolve_error(e.code().0 as u32)),
    };
    let mut query: WideText = "SELECT * FROM MSFT_CliTranslateTable WHERE Name=".into();
    query.0.extend(literal(name).0);
    let records = match query::query_text(&svc, &request(options, "MSFT_CliTranslateTable"), &query)
    {
        Ok(s) => s,
        Err(e) if e.code == 0x80041010 => return Ok(None),
        Err(e) => return Err(resolve_error(e.code)),
    };
    let Some(record) = records.objects.first() else {
        return Err(ResolveError {
            code: 0xABED,
            message: "Translate table does not exists in Alias Namespace.".into(),
            token: None,
        });
    };
    let Some(Property {
        value: CimValue::Array(entries),
        ..
    }) = record
        .properties
        .iter()
        .find(|p| p.name.eq_ignore_case("Tbl"))
    else {
        return Ok(Some(vec![]));
    };
    let mut out = vec![];
    for entry in entries {
        if let CimValue::Object(r) = entry {
            let get = |name: &str| {
                r.properties
                    .iter()
                    .find(|p| p.name.eq_ignore_case(name))
                    .and_then(|p| match &p.value {
                        CimValue::String(v) => Some(v.clone()),
                        _ => None,
                    })
            };
            if let (Some(from), Some(to)) = (get("FromValue"), get("ToValue")) {
                out.push((from, to))
            }
        }
    }
    Ok(Some(out))
}
