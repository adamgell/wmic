use super::SessionSettings;
use crate::model::*;
fn on(value: bool) -> WideText {
    if value { "ON" } else { "OFF" }.into()
}
pub(super) fn context(
    command: &ParsedCommand,
    settings: &SessionSettings,
    local: &WideText,
    sequence: u32,
    every_count: u32,
) -> RenderContext {
    let keys = vec![
        ("NAMESPACE", settings.namespace.clone()),
        ("ROLE", settings.role.clone()),
        (
            "IMPLEVEL",
            [
                "DEFAULT",
                "ANONYMOUS",
                "IDENTIFY",
                "IMPERSONATE",
                "DELEGATE",
            ][settings.impersonation.min(4) as usize]
                .into(),
        ),
        (
            "AUTHLEVEL",
            [
                "DEFAULT",
                "NONE",
                "CONNECT",
                "CALL",
                "PKT",
                "PKTINTEGRITY",
                "PKTPRIVACY",
            ][settings.authentication.min(6) as usize]
                .into(),
        ),
        ("LOCALE", settings.locale.clone()),
        (
            "PRIVILEGES",
            if settings.privileges {
                "ENABLE"
            } else {
                "DISABLE"
            }
            .into(),
        ),
        ("TRACE", on(settings.trace)),
        (
            "RECORD",
            settings.record.clone().unwrap_or_else(|| "N/A".into()),
        ),
        ("INTERACTIVE", on(settings.interactive)),
        ("FAILFAST", on(settings.failfast)),
        ("OUTPUT", settings.output.clone()),
        ("APPEND", settings.append.clone()),
        (
            "USER",
            settings.user.clone().unwrap_or_else(|| "N/A".into()),
        ),
        ("AGGREGATE", on(settings.aggregate)),
    ];
    RenderContext {
        components_xml: WideText::default(),
        sequence,
        every_count,
        issued_from: local.clone(),
        started_at: timestamp(),
        command_line: command.command_line.clone(),
        nodes: settings
            .nodes
            .iter()
            .map(|n| {
                if n.eq_ignore_case(".") {
                    local.clone()
                } else {
                    n.clone()
                }
            })
            .collect(),
        settings: keys.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    }
}
#[cfg(windows)]
fn timestamp() -> WideText {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:02}-{:02}-{:04}T{:02}:{:02}:{:02}",
        t.wMonth, t.wDay, t.wYear, t.wHour, t.wMinute, t.wSecond
    )
    .into()
}
#[cfg(unix)]
fn timestamp() -> WideText {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
        return WideText::default();
    }
    format!(
        "{:02}-{:02}-{:04}T{:02}:{:02}:{:02}",
        tm.tm_mon + 1,
        tm.tm_mday,
        i64::from(tm.tm_year) + 1900,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
    .into()
}
#[cfg(not(any(windows, unix)))]
fn timestamp() -> WideText {
    WideText::default()
}

pub(super) fn prepare(
    request: &mut crate::backend::ResolvedRequest,
    settings: &SessionSettings,
    backend: &mut dyn crate::backend::Management,
) -> Result<(), crate::render::RenderError> {
    #[derive(serde::Deserialize)]
    struct Wire {
        name: String,
        entries: Vec<(String, String)>,
    }
    let stock: Vec<TranslationTable> =
        serde_json::from_str::<Vec<Wire>>(include_str!("../../assets/translations.json"))
            .expect("embedded translations are tested")
            .into_iter()
            .map(|w| TranslationTable {
                name: w.name.into(),
                entries: w
                    .entries
                    .into_iter()
                    .map(|(a, b)| (a.into(), b.into()))
                    .collect(),
            })
            .collect();
    let mut tables = vec![];
    for m in &request.modifiers {
        if m.name.eq_ignore_case("translate")
            && let SwitchValue::Value(name) = &m.value
        {
            let table = backend
                .load_translation(&settings.role, name)
                .map_err(|e| crate::render::RenderError::status(e.code, e.message))?
                .or_else(|| {
                    if settings.role.eq_ignore_case("root\\cli") {
                        stock
                            .iter()
                            .find(|t| name.eq_ignore_case(&t.name.to_string_lossy()))
                            .map(|t| t.entries.clone())
                    } else {
                        None
                    }
                });
            if let Some(table) = table {
                tables.push((name.clone(), table))
            }
        }
    }
    request.format_spec = Some(crate::render::compile_format(request, &tables)?);
    Ok(())
}

pub(super) fn components(
    command: &ParsedCommand,
    request: &crate::backend::ResolvedRequest,
    aliases: &crate::alias::AliasStore,
) -> WideText {
    if request.verb != Verb::List {
        return WideText::default();
    }
    let Some(Target::Alias(name)) = &command.target else {
        return WideText::default();
    };
    let Some(alias) = aliases.find(name) else {
        return WideText::default();
    };
    fn escape(value: &WideText) -> WideText {
        let mut out = WideText::default();
        for &u in &value.0 {
            out.0.extend(match u {
                38 => "&amp;".encode_utf16().collect(),
                60 => "&lt;".encode_utf16().collect(),
                62 => "&gt;".encode_utf16().collect(),
                _ => vec![u],
            })
        }
        out
    }
    fn tag(out: &mut WideText, name: &str, value: &WideText) {
        out.0.extend(format!("<{name}>").encode_utf16());
        out.0.extend(escape(value).0);
        out.0.extend(format!("</{name}>").encode_utf16())
    }
    let mut out = WideText::default();
    tag(&mut out, "FRIENDLYNAME", name);
    tag(&mut out, "TARGET", &alias.target.as_str().into());
    tag(&mut out, "ALIASTARGET", &request.class_name);
    tag(
        &mut out,
        "PWHERE",
        &alias.p_where.as_deref().unwrap_or("").into(),
    );
    tag(&mut out, "NAMESPACE", &request.connection.namespace);
    tag(
        &mut out,
        "DESCRIPTION",
        &alias.description.as_deref().unwrap_or("").into(),
    );
    let mut properties = request.display.clone();
    properties.sort_by(|a, b| a.0.cmp(&b.0));
    let mut query: WideText = "SELECT ".into();
    for (i, (_, source)) in properties.iter().enumerate() {
        if i > 0 {
            query.0.extend(", ".encode_utf16())
        }
        query.0.extend(&source.0)
    }
    query.0.extend(" FROM ".encode_utf16());
    query.0.extend(&request.class_name.0);
    if let Some(selector) = &request.selector {
        query.0.extend(" WHERE ".encode_utf16());
        query.0.extend(&selector.0)
    }
    tag(&mut out, "RESULTANTQUERY", &query);
    out.0.extend("<FORMATS>".encode_utf16());
    if let Some(spec) = &request.format_spec {
        for step in &spec.transforms {
            out.0.extend("<FORMAT>".encode_utf16());
            let label = crate::render::logical_format_name(&step.name);
            tag(&mut out, "NAME", &label);
            out.0.extend("</FORMAT>".encode_utf16());
        }
    }
    out.0.extend("</FORMATS><PROPERTIES>".encode_utf16());
    for (name, source) in properties {
        out.0.extend("<PROPERTY>".encode_utf16());
        tag(&mut out, "NAME", &name);
        tag(&mut out, "DERIVATION", &source);
        out.0.extend("</PROPERTY>".encode_utf16());
    }
    out.0.extend("</PROPERTIES>".encode_utf16());
    out
}

pub(super) fn context_text(settings: &SessionSettings, local: &WideText) -> WideText {
    let command = ParsedCommand::empty(WideText::default());
    let c = context(&command, settings, local, 0, 0);
    let mut out = WideText::default();
    let nodes = c
        .nodes
        .iter()
        .map(WideText::to_string_lossy)
        .collect::<Vec<_>>()
        .join(",");
    for (name, value) in c.settings {
        let key = name.to_string_lossy();
        if key == "IMPLEVEL" {
            out.0
                .extend(format!("{:<22}: {nodes}\r\r\n", "NODE(S)").encode_utf16());
        }
        out.0.extend(format!("{key:<22}: ").encode_utf16());
        out.0.extend(&value.0);
        out.0.extend("\r\r\n".encode_utf16());
        if key == "IMPLEVEL" {
            out.0.extend(
                format!(
                    "[AUTHORITY            : {}]\r\r\n",
                    settings.authority.as_ref().unwrap_or(&"N/A".into())
                )
                .encode_utf16(),
            );
        }
    }
    out.0.extend("\r\r\n".encode_utf16());
    out
}
