//! Legacy diagnostic records are a compatibility protocol, not Rust source locations.
use super::SessionSettings;
use crate::{alias::AliasStore, backend::ResolvedRequest, model::*};

#[path = "trace_errors.rs"]
mod errors;
#[path = "trace_query.rs"]
mod query;
pub(super) use query::{formatting, method_record};

pub(super) fn event(
    out: &mut WideText,
    file: &str,
    line: u32,
    call: &str,
    result: Option<&str>,
    success: bool,
) {
    let label = if success { "SUCCESS" } else { "FAIL" };
    let mut text = format!(
        "{label}: {call}\r\nLine: {line:6} File: onecore\\admin\\wmi\\wbem\\tools\\wmic\\{file}.cpp\r\n"
    );
    if let Some(result) = result {
        text.push_str(&format!("Result:  {result}\r\n"));
    }
    text.push_str("\r\n");
    out.0.extend(text.encode_utf16());
}

fn wbem_dir() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\WINDOWS".into());
    format!("{root}\\system32\\wbem\\")
}

#[derive(Default)]
pub(super) struct TraceState {
    role: Option<WideText>,
    locale: Option<WideText>,
    server_loaded: bool,
    query: query::QueryState,
}

impl TraceState {
    pub(super) fn execution(
        &mut self,
        request: &ResolvedRequest,
        result: Result<&ManagementResult, &ManagementError>,
        node: &WideText,
        enabled: bool,
    ) -> WideText {
        if enabled {
            self.query.execution(request, result, node)
        } else {
            self.query.observe(request, result);
            WideText::default()
        }
    }
    pub(super) fn alias(
        &mut self,
        parsed: &ParsedCommand,
        settings: &SessionSettings,
        aliases: &AliasStore,
    ) -> WideText {
        if parsed
            .modifiers
            .iter()
            .any(|m| m.name.eq_cli("node") && m.value == SwitchValue::Reset)
        {
            self.server_loaded = false;
        }
        let Some(Target::Alias(name)) = &parsed.target else {
            return WideText::default();
        };
        let cached_server = self.server_loaded;
        if aliases.find(name).is_some() {
            self.server_loaded = true;
        }
        let root_changed = self
            .role
            .as_ref()
            .is_none_or(|r| !r.eq_cli_text(&settings.role));
        let locale_changed = root_changed
            || self
                .locale
                .as_ref()
                .is_none_or(|l| !l.eq_cli_text(&settings.locale));
        self.role = Some(settings.role.clone());
        self.locale = Some(settings.locale.clone());
        let mut out = WideText::default();
        if !settings.trace {
            return out;
        }
        let auth = settings
            .authority
            .as_ref()
            .map_or("<null>".into(), WideText::to_string_lossy);
        let connect = |path: &WideText| {
            format!(
                "IWbemLocator::ConnectServer(L\"{path}\", NULL, NULL, L\"{}\", 0L, L\"{auth}\", NULL, -)",
                settings.locale
            )
        };
        if root_changed {
            event(
                &mut out,
                "cmdalias",
                143,
                &connect(&settings.role),
                None,
                true,
            );
            event(
                &mut out,
                "cmdalias",
                162,
                &format!(
                    "CoSetProxyBlanket(-, RPC_C_AUTHN_DEFAULT, RPC_C_AUTHZ_NONE, NULL, {}, {}, -, EOAC_NONE)",
                    settings.authentication, settings.impersonation
                ),
                None,
                true,
            );
        }
        if locale_changed {
            event(
                &mut out,
                "cmdalias",
                2524,
                &connect(&format!("{}\\{}", settings.role, settings.locale).into()),
                None,
                true,
            );
            event(
                &mut out,
                "cmdalias",
                2538,
                &format!(
                    "CoSetProxyBlanket(-, RPC_C_AUTHN_DEFAULT, RPC_C_AUTHZ_NONE, NULL, {},   {}, -, EOAC_NONE)",
                    settings.authentication, settings.impersonation
                ),
                None,
                true,
            );
        }
        let definition = aliases.find(name);
        alias_object(&mut out, 239, &name.to_string_lossy(), definition.is_some());
        let Some(a) = definition else { return out };
        get(&mut out, 258, "Target", "0, -, 0, 0", Some(&a.target));
        get(
            &mut out,
            286,
            "PWhere",
            "0, -,0, 0",
            Some(a.p_where.as_deref().unwrap_or("")),
        );
        get(
            &mut out,
            314,
            "Connection",
            "0, -, 0, 0",
            Some("<Embeded Object>"),
        );
        event(
            &mut out,
            "cmdalias",
            330,
            "QueryInterface(IID_IWbemClassObject, -)",
            None,
            true,
        );
        for (skip, line, property, args, value) in [
            (
                settings.explicit_namespace,
                430,
                "Namespace",
                "0, -,0, 0",
                a.connection.namespace.as_deref(),
            ),
            (
                !settings.locale.eq_cli("ms_409"),
                460,
                "Locale",
                "0,-, 0, 0",
                a.connection.locale.as_deref(),
            ),
            (
                settings.user.is_some(),
                491,
                "User",
                "0,-, 0, 0",
                a.connection.user.as_deref(),
            ),
            (
                settings.password.is_some(),
                524,
                "Password",
                "0, -, 0, 0",
                a.connection.password.as_ref().map(|_| "<redacted>"),
            ),
            (
                cached_server
                    || settings.explicit_nodes && settings.nodes.iter().any(|n| !n.eq_cli(".")),
                556,
                "Server",
                "0, -, 0, 0",
                a.connection.server.as_deref(),
            ),
            (
                false,
                587,
                "Authority",
                "0, -, 0, 0",
                a.connection.authority.as_deref(),
            ),
        ] {
            if !skip {
                get(
                    &mut out,
                    line,
                    property,
                    args,
                    Some(value.unwrap_or("<null>")),
                );
            }
        }
        get(
            &mut out,
            1943,
            "__RELPATH",
            "0,-, 0, 0",
            Some(&format!(
                "MSFT_CliAlias.FriendlyName=\"{}\"",
                a.friendly_name
            )),
        );
        event(
            &mut out,
            "cmdalias",
            2610,
            &format!(
                "IWbemServices::GetObject(L\"MSFT_LocalizablePropertyValue.ObjectLocator=\"\",PropertyName=\"Description\",RelPath=\"MSFT_CliAlias.FriendlyName=\\\"{}\\\"\"\", 0, NULL, -)",
                a.friendly_name
            ),
            None,
            true,
        );
        get(&mut out, 2621, "Text", "0, -, 0, 0", Some("<Array>"));
        if parsed.verb == Verb::Call && !parsed.help {
            method_metadata(&mut out, name, a, parsed);
            return out;
        }
        let list = parsed.verb == Verb::List && !parsed.help;
        if parsed.help {
            alias_object(&mut out, 2966, &a.friendly_name, true);
            get(&mut out, 2977, "Verbs", "0, -, 0, 0", Some("<Array>"));
        }
        let lookup_name = if parsed.help {
            a.friendly_name.as_str()
        } else {
            &name.to_string_lossy()
        };
        alias_object(&mut out, if list { 1210 } else { 1593 }, lookup_name, true);
        get(
            &mut out,
            if list { 1221 } else { 1605 },
            "Formats",
            if list { "0, -,0, 0" } else { "0,-, 0, 0" },
            Some("<Array>"),
        );
        let selected = if parsed.help {
            a.formats.last()
        } else {
            let view = parsed
                .view
                .as_ref()
                .map_or("FULL".into(), WideText::to_string_lossy);
            a.formats
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(&view))
        };
        for f in &a.formats {
            get(
                &mut out,
                if list { 1271 } else { 1657 },
                "Name",
                "0,-, 0, 0",
                Some(&f.name),
            );
            if selected.is_some_and(|s| std::ptr::eq(s, f)) {
                break;
            }
        }
        if let Some(f) = selected {
            if list {
                get(
                    &mut out,
                    1294,
                    "Format",
                    "0, -, 0, 0",
                    Some(f.format.as_deref().unwrap_or("<null>")),
                );
            }
            get(
                &mut out,
                if list { 1339 } else { 1694 },
                "Properties",
                "0, -, 0, 0",
                Some("<Array>"),
            );
            for p in f
                .properties
                .iter()
                .take(if parsed.help { 1 } else { usize::MAX })
            {
                get(
                    &mut out,
                    if list { 1398 } else { 1751 },
                    "Name",
                    "0, -, 0, 0",
                    Some(&p.name),
                );
                get(
                    &mut out,
                    if list { 1414 } else { 1767 },
                    "Derivation",
                    "0, -, 0, 0",
                    Some(p.derivation.as_deref().unwrap_or("<null>")),
                );
                if !list
                    && !parsed.help
                    && parsed.properties.iter().any(|name| name.eq_cli(&p.name))
                {
                    get(
                        &mut out,
                        1923,
                        "Description",
                        "0,-, 0, 0",
                        Some(p.description.as_deref().unwrap_or("<null>")),
                    );
                    get(
                        &mut out,
                        2710,
                        "Qualifiers",
                        "0, -, 0, 0",
                        Some(if p.qualifiers.is_empty() {
                            "<null>"
                        } else {
                            "<Array>"
                        }),
                    );
                    for q in &p.qualifiers {
                        get(&mut out, 2762, "Name", "0, -, 0, 0", Some(&q.name));
                        get(
                            &mut out,
                            2776,
                            "QualifierValue",
                            "0, -, 0, 0",
                            Some("<Array>"),
                        );
                    }
                }
            }
        }
        if parsed.help {
            alias_object(&mut out, 1210, &a.friendly_name, true);
            get(&mut out, 1221, "Formats", "0, -,0, 0", Some("<Array>"));
        }
        out
    }
}

fn get(out: &mut WideText, line: u32, name: &str, args: &str, value: Option<&str>) {
    let gap = if matches!(line, 314 | 1294 | 1339 | 794) {
        ""
    } else {
        " "
    };
    event(
        out,
        "cmdalias",
        line,
        &format!("IWbemClassObject::Get(L\"{name}\",{gap}{args})"),
        value,
        true,
    );
}
fn alias_object(out: &mut WideText, line: u32, name: &str, success: bool) {
    event(
        out,
        "cmdalias",
        line,
        &format!("IWbemServices::GetObject(L\"MSFT_CliAlias.FriendlyName='{name}'\", 0, NULL, -)"),
        None,
        success,
    );
}
fn method_metadata(
    out: &mut WideText,
    name: &WideText,
    a: &crate::alias::AliasDefinition,
    parsed: &ParsedCommand,
) {
    alias_object(out, 682, &name.to_string_lossy(), true);
    get(out, 708, "Verbs", "0, -, 0, 0", Some("<Array>"));
    let Some(method) = &parsed.method else { return };
    for verb in &a.verbs {
        get(out, 757, "Name", "0, -,0, 0", Some(&verb.name));
        if !method.eq_cli(&verb.name) {
            continue;
        }
        get(
            out,
            1923,
            "Description",
            "0,-, 0, 0",
            Some(verb.description.as_deref().unwrap_or("<null>")),
        );
        get(out, 794, "Parameters", "0, -, 0, 0", Some("<Array>"));
        let kind = verb.verb_type.map(|v| v.to_string());
        get(
            out,
            812,
            "VerbType",
            "0, -, 0, 0",
            Some(kind.as_deref().unwrap_or("<null>")),
        );
        get(out, 838, "Derivation", "0, -, 0, 0", Some(&verb.derivation));
        for p in &verb.parameters {
            get(
                out,
                902,
                "ParaId",
                "0, -, 0, 0",
                Some(p.para_id.as_deref().unwrap_or("<null>")),
            );
            get(
                out,
                917,
                "Type",
                "0, -, 0, 0",
                Some(p.r#type.as_deref().unwrap_or("<null>")),
            );
            get(
                out,
                933,
                "Default",
                "0, -, 0, 0",
                Some(p.default.as_deref().unwrap_or("<null>")),
            );
            get(out, 2710, "Qualifiers", "0, -, 0, 0", Some("<Array>"));
            for q in &p.qualifiers {
                get(out, 2762, "Name", "0, -, 0, 0", Some(&q.name));
                get(out, 2776, "QualifierValue", "0, -, 0, 0", Some("<Array>"));
            }
        }
        break;
    }
}

pub(super) fn parser(
    request: &ResolvedRequest,
    result: Result<&ManagementResult, &ManagementError>,
) -> WideText {
    let mut out = WideText::default();
    if request.from_alias {
        return out;
    }
    let error = result.err();
    let connected = error.is_none_or(|e| e.code != 0x8004100e);
    let c = &request.connection;
    event(
        &mut out,
        "parserengine",
        4244,
        &format!(
            "IWbemLocator::ConnectServer(L\"{}\", NULL, NULL, L\"{}\", 0L, L\"{}\", NULL, -)",
            c.namespace,
            c.locale.clone().unwrap_or_default(),
            c.authority
                .as_ref()
                .map_or("<null>".into(), WideText::to_string_lossy)
        ),
        None,
        connected,
    );
    if !connected {
        return out;
    }
    event(
        &mut out,
        "parserengine",
        4281,
        &format!(
            "CoSetProxyBlanket(-, RPC_C_AUTHN_DEFAULT, RPC_C_AUTHZ_NONE, NULL, {},   {}, -, EOAC_NONE)",
            c.authentication, c.impersonation
        ),
        None,
        true,
    );
    let found = error.is_none_or(|e| e.code != 0x80041002 && e.code != 0x80041010);
    event(
        &mut out,
        "parserengine",
        3865,
        &format!(
            "IWbemServices::GetObject(L\"{}\", WBEM_FLAG_USE_AMENDED_QUALIFIERS, 0, NULL, -, -)",
            request.class_name
        ),
        None,
        found,
    );
    if found {
        event(
            &mut out,
            "parserengine",
            3877,
            "IWbemClassObject::GetNames(NULL, WBEM_FLAG_ALWAYS | WBEM_FLAG_NONSYSTEM_ONLY, NULL, -)",
            None,
            true,
        );
    }
    out
}

pub(super) fn error_diagnostic(error: &ManagementError, node: Option<&WideText>) -> WideText {
    let description = errors::description(error.code)
        .map_or_else(|| error.message.to_string_lossy(), str::to_owned)
        .replace("\r\n", "\r\r\n");
    let prefix = node.map_or(String::new(), |node| format!("Node - {node}\r\r\n"));
    let facility = match (error.code >> 16) & 0x1fff {
        4 => "WMI",
        7 => "Win32",
        _ => "COM",
    };
    format!("{prefix}ERROR:\r\r\nCode = 0x{:08x}\r\r\nDescription = {description}\r\r\nFacility = {facility}\r\r\n",error.code).into()
}
