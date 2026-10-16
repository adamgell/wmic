use crate::model::WideText;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSettings {
    pub output: WideText,
    pub append: WideText,
    pub record: Option<WideText>,
    pub namespace: WideText,
    pub role: WideText,
    pub nodes: Vec<WideText>,
    pub explicit_namespace: bool,
    pub explicit_role: bool,
    pub explicit_nodes: bool,
    pub locale: WideText,
    pub authentication: u32,
    pub impersonation: u32,
    pub privileges: bool,
    pub user: Option<WideText>,
    #[serde(skip)]
    pub password: Option<crate::backend::SecretWide>,
    pub authority: Option<WideText>,
    pub trace: bool,
    pub failfast: bool,
    pub failfast_timeout_ms: u32,
    pub aggregate: bool,
    pub interactive: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            output: "STDOUT".into(),
            append: "STDOUT".into(),
            record: None,
            namespace: r"root\cimv2".into(),
            role: r"root\cli".into(),
            nodes: vec![".".into()],
            explicit_namespace: false,
            explicit_role: false,
            explicit_nodes: false,
            locale: "ms_409".into(),
            authentication: 6,
            impersonation: 3,
            privileges: true,
            user: None,
            password: None,
            authority: None,
            trace: false,
            failfast: false,
            failfast_timeout_ms: 5000,
            aggregate: true,
            interactive: false,
        }
    }
}

impl SessionSettings {
    pub fn for_command(
        &self,
        command: &crate::model::ParsedCommand,
    ) -> Result<Self, crate::model::ParseError> {
        use crate::model::{ModifierScope, SwitchValue};
        let mut s = self.clone();
        let defaults = Self::default();
        for m in command
            .modifiers
            .iter()
            .filter(|m| m.scope == ModifierScope::Global)
        {
            let value = match &m.value {
                SwitchValue::Value(v) => Some(v.clone()),
                SwitchValue::Reset => None,
                SwitchValue::Missing => {
                    if m.name.eq_ignore_case("?") || command.help {
                        continue;
                    }
                    return Err(setting_error(0xAC5D, "Invalid Global Switch."));
                }
            };
            let name = m.name.to_string_lossy().to_ascii_lowercase();
            match name.as_str() {
                "output" => s.output = value.unwrap_or_else(|| "STDOUT".into()),
                "append" => s.append = value.unwrap_or_else(|| "STDOUT".into()),
                "record" => s.record = value,
                "namespace" => {
                    s.explicit_namespace = value.is_some();
                    s.namespace = namespace(
                        value,
                        &s.namespace,
                        &defaults.namespace,
                        m.raw_value.as_ref(),
                        0xAC4B,
                        "Invalid Namespace.",
                    )?;
                }
                "role" => {
                    s.explicit_role = value.is_some();
                    s.role = namespace(
                        value,
                        &s.role,
                        &defaults.role,
                        m.raw_value.as_ref(),
                        0xAC61,
                        "Invalid Role.",
                    )?;
                }
                "node" => {
                    s.explicit_nodes = value.is_some();
                    s.nodes = match value {
                        Some(v) => super::load_nodes(&v)
                            .map_err(|e| setting_error(e.code, &e.message.to_string_lossy()))?,
                        None if m.raw_value.as_ref().is_some_and(|v| v.0.is_empty()) => {
                            return Err(setting_error(0xAC47, "Invalid Node Name."));
                        }
                        None => defaults.nodes.clone(),
                    };
                }
                "authlevel" => {
                    if value.is_none() {
                        return Err(setting_error(0xAC49, "Invalid Authentication Level."));
                    }
                    s.authentication = level(
                        value.as_ref(),
                        &[
                            "default",
                            "none",
                            "connect",
                            "call",
                            "pkt",
                            "pktintegrity",
                            "pktprivacy",
                        ],
                        0,
                        6,
                    )
                    .ok_or_else(|| setting_error(0xAC49, "Invalid Authentication Level."))?;
                }
                "implevel" => {
                    if value.is_none() {
                        return Err(setting_error(0xAC48, "Invalid Impersonation Level."));
                    }
                    s.impersonation = level(
                        value.as_ref(),
                        &["anonymous", "identify", "impersonate", "delegate"],
                        1,
                        3,
                    )
                    .ok_or_else(|| setting_error(0xAC48, "Invalid Impersonation Level."))?;
                }
                "locale" => s.locale = value.unwrap_or_else(|| defaults.locale.clone()),
                "privileges" => {
                    if value.is_none() {
                        return Err(setting_error(
                            0xAC5E,
                            "Invalid Privileges Option Value (ENABLE|DISABLE expected).",
                        ));
                    }
                    s.privileges =
                        toggle(value.as_ref(), "enable", "disable", true).ok_or_else(|| {
                            setting_error(
                                0xAC5E,
                                "Invalid Privileges Option Value (ENABLE|DISABLE expected).",
                            )
                        })?
                }
                "failfast" => {
                    if value.is_none() {
                        return Err(setting_error(
                            0xAC68,
                            "Invalid FailFast Option Value (ON|OFF expected).",
                        ));
                    }
                    if let Some(ms) = value
                        .as_ref()
                        .and_then(|v| v.to_string_lossy().parse::<u32>().ok())
                        .filter(|ms| *ms > 0)
                    {
                        s.failfast = true;
                        s.failfast_timeout_ms = ms;
                    } else {
                        s.failfast =
                            toggle(value.as_ref(), "on", "off", false).ok_or_else(|| {
                                setting_error(
                                    0xAC68,
                                    "Invalid FailFast Option Value (ON|OFF expected).",
                                )
                            })?;
                    }
                }
                "interactive" => {
                    if value.is_none() {
                        return Err(setting_error(
                            0xAC66,
                            "Invalid Interactive Option Value (ON|OFF expected).",
                        ));
                    }
                    s.interactive = value
                        .as_ref()
                        .and_then(|v| toggle(Some(v), "on", "off", false))
                        .ok_or_else(|| {
                            setting_error(
                                0xAC66,
                                "Invalid Interactive Option Value (ON|OFF expected).",
                            )
                        })?
                }
                "trace" | "aggregate" => {
                    let (code, message) = if name == "trace" {
                        (0xAC54, "Invalid Trace Option Value (ON|OFF expected).")
                    } else {
                        (0xAC6F, "Invalid Aggregate Option Value (ON|OFF expected).")
                    };
                    let v = value
                        .as_ref()
                        .and_then(|v| toggle(Some(v), "on", "off", false))
                        .ok_or_else(|| setting_error(code, message))?;
                    if name == "trace" {
                        s.trace = v
                    } else {
                        s.aggregate = v
                    }
                }
                "user" => {
                    s.user = value;
                    if !command
                        .modifiers
                        .iter()
                        .any(|m| m.name.eq_ignore_case("password"))
                    {
                        s.password = None;
                    }
                }
                "password" => s.password = value.map(crate::backend::SecretWide::new),
                "authority" => s.authority = value,
                _ => {}
            }
        }
        Ok(s)
    }
}
fn level(v: Option<&WideText>, names: &[&str], base: u32, default: u32) -> Option<u32> {
    match v {
        None => Some(default),
        Some(v) => names
            .iter()
            .position(|n| v.eq_ignore_case(n))
            .map(|i| i as u32 + base),
    }
}
fn toggle(v: Option<&WideText>, on: &str, off: &str, default: bool) -> Option<bool> {
    match v {
        None => Some(default),
        Some(v) if v.eq_ignore_case(on) => Some(true),
        Some(v) if v.eq_ignore_case(off) => Some(false),
        _ => None,
    }
}
fn setting_error(code: u32, message: &str) -> crate::model::ParseError {
    crate::model::ParseError {
        code,
        message: message.into(),
        token: None,
    }
}

fn namespace(
    value: Option<WideText>,
    current: &WideText,
    default: &WideText,
    raw: Option<&WideText>,
    code: u32,
    message: &str,
) -> Result<WideText, crate::model::ParseError> {
    match value {
        None if raw.is_some_and(|v| v.0.is_empty()) => Err(setting_error(code, message)),
        None => Ok(default.clone()),
        Some(v) if v.0.starts_with(&[92, 92]) => Ok(WideText(v.0[2..].to_vec())),
        Some(v) => {
            let mut result = current.clone();
            result.0.push(92);
            result.0.extend(v.0);
            Ok(result)
        }
    }
}
