use crate::model::WideText;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSettings {
    pub namespace: WideText,
    pub role: WideText,
    pub nodes: Vec<WideText>,
    pub explicit_namespace: bool,
    pub explicit_role: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            namespace: r"root\cimv2".into(),
            role: r"root\cli".into(),
            nodes: vec![".".into()],
            explicit_namespace: false,
            explicit_role: false,
        }
    }
}

impl SessionSettings {
    pub fn for_command(
        &self,
        command: &crate::model::ParsedCommand,
    ) -> Result<Self, crate::model::ParseError> {
        use crate::model::{ModifierScope, ParseError, SwitchValue};
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
                    if m.name.eq_ascii_ignore_case("?") {
                        continue;
                    }
                    return Err(ParseError {
                        code: 0xAC5D,
                        message: "Invalid Global Switch.".into(),
                        token: None,
                    });
                }
            };
            if m.name.eq_ascii_ignore_case("namespace") {
                s.explicit_namespace = value.is_some();
                s.namespace = value.unwrap_or_else(|| defaults.namespace.clone());
            } else if m.name.eq_ascii_ignore_case("role") {
                s.explicit_role = value.is_some();
                s.role = value.unwrap_or_else(|| defaults.role.clone());
            } else if m.name.eq_ascii_ignore_case("node") {
                s.nodes = value
                    .map(|v| {
                        v.0.split(|u| *u == 44)
                            .map(|u| WideText(u.to_vec()).trim_ascii())
                            .collect()
                    })
                    .unwrap_or_else(|| defaults.nodes.clone());
            }
        }
        Ok(s)
    }
}
