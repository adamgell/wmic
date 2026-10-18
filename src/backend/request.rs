use crate::model::{Modifier, Verb, WideText};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionOptions {
    pub node: WideText,
    pub namespace: WideText,
    pub locale: Option<WideText>,
    pub user: Option<WideText>,
    #[serde(skip)]
    pub password: Option<SecretWide>,
    pub authority: Option<WideText>,
    pub authentication: u32,
    pub impersonation: u32,
    pub privileges: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRequest {
    #[serde(default)]
    pub format_spec: Option<crate::model::FormatSpec>,
    #[serde(default)]
    pub render_context: Option<crate::model::RenderContext>,
    pub connection: ConnectionOptions,
    /// Preserve alias versus PATH/CLASS syntax for native method diagnostics.
    #[serde(default)]
    pub from_alias: bool,
    pub class_name: WideText,
    pub instance_path: Option<WideText>,
    pub class_mode: bool,
    pub query: Option<WideText>,
    pub selector: Option<WideText>,
    pub verb: Verb,
    pub properties: Vec<WideText>,
    pub method: Option<WideText>,
    pub arguments: Vec<WideText>,
    pub modifiers: Vec<Modifier>,
    pub view: Option<WideText>,
    pub display: Vec<(WideText, WideText)>,
    pub format: Option<WideText>,
    pub association: Option<AssociationFilters>,
}

/// A credential can be passed to COM, but cannot accidentally enter Debug/JSON output.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretWide(WideText);
impl SecretWide {
    pub fn new(value: WideText) -> Self {
        Self(value)
    }
    pub fn expose(&self) -> &WideText {
        &self.0
    }
}
impl std::fmt::Debug for SecretWide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<redacted>")
    }
}
impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            node: ".".into(),
            namespace: "root\\cimv2".into(),
            locale: Some("ms_409".into()),
            user: None,
            password: None,
            authority: None,
            authentication: 6,
            impersonation: 3,
            privileges: true,
        }
    }
}
impl ConnectionOptions {
    pub fn validate(&self) -> Result<(), crate::model::ParseError> {
        if self.authority.as_ref().is_some_and(|a| {
            a.to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("ntlmdomain:")
        }) && self.user.as_ref().is_some_and(|u| u.0.contains(&92))
        {
            return Err(crate::model::ParseError {
                code: 0x80041008,
                message: "Invalid parameter".into(),
                token: None,
            });
        }
        Ok(())
    }
}

impl ConnectionOptions {
    pub fn identity_parts(&self) -> (WideText, WideText) {
        let user = self.user.as_ref().cloned().unwrap_or_default();
        if let Some(i) = user.0.iter().position(|u| *u == 92) {
            return (
                WideText(user.0[i + 1..].to_vec()),
                WideText(user.0[..i].to_vec()),
            );
        }
        let domain = self
            .authority
            .as_ref()
            .filter(|a| {
                a.0.len() >= 11 && WideText(a.0[..11].to_vec()).eq_ignore_case("NTLMDOMAIN:")
            })
            .map(|a| WideText(a.0[11..].to_vec()))
            .unwrap_or_default();
        (user, domain)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociationFilters {
    pub result_class: Option<WideText>,
    pub result_role: Option<WideText>,
    pub assoc_class: Option<WideText>,
}

impl<'de> Deserialize<'de> for SecretWide {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self::new(s.into()))
    }
}
