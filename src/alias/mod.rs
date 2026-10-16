//! Alias metadata is data, never startup provider enumeration or executable code.
mod predicate;
mod virtual_role;
use crate::{
    backend::{ConnectionOptions, ResolvedRequest},
    model::*,
    session::SessionSettings,
};
use serde::Deserialize;
use std::fmt;
fn null_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasDefinition {
    pub friendly_name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub localized_description: Option<String>,
    pub target: String,
    pub p_where: Option<String>,
    pub connection: AliasConnection,
    #[serde(default, deserialize_with = "null_vec")]
    pub formats: Vec<AliasFormat>,
    #[serde(default, deserialize_with = "null_vec")]
    pub verbs: Vec<AliasVerb>,
    #[serde(default, deserialize_with = "null_vec")]
    pub qualifiers: Vec<AliasQualifier>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasConnection {
    #[serde(rename = "NameSpace")]
    pub namespace: Option<String>,
    pub server: Option<String>,
    pub locale: Option<String>,
    pub authority: Option<String>,
    pub user: Option<String>,
    pub password: Option<crate::backend::SecretWide>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasFormat {
    pub name: String,
    pub format: Option<String>,
    #[serde(default, deserialize_with = "null_vec")]
    pub properties: Vec<AliasProperty>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasProperty {
    pub name: String,
    pub derivation: Option<String>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "null_vec")]
    pub qualifiers: Vec<AliasQualifier>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasQualifier {
    pub name: String,
    #[serde(default, deserialize_with = "null_vec")]
    pub qualifier_value: Vec<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasVerb {
    pub name: String,
    pub derivation: String,
    pub description: Option<String>,
    pub verb_type: Option<u32>,
    pub usage: Option<String>,
    #[serde(default, deserialize_with = "null_vec")]
    pub parameters: Vec<AliasParameter>,
    #[serde(default, deserialize_with = "null_vec")]
    pub qualifiers: Vec<AliasQualifier>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AliasParameter {
    pub default: Option<String>,
    pub description: Option<String>,
    pub optional: Option<bool>,
    pub para_id: Option<String>,
    pub r#type: Option<String>,
    #[serde(default, deserialize_with = "null_vec")]
    pub qualifiers: Vec<AliasQualifier>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveError {
    pub code: u32,
    pub message: WideText,
    pub token: Option<WideText>,
}
impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for ResolveError {}
fn error(code: u32, msg: &str, token: Option<WideText>) -> ResolveError {
    ResolveError {
        code,
        message: msg.into(),
        token,
    }
}
pub trait RoleLoader {
    fn load(&mut self, role: &WideText) -> Result<Option<Vec<AliasDefinition>>, ResolveError>;
}
#[derive(Clone, Debug)]
pub struct DerivedCommand {
    pub template: WideText,
    pub selection: Box<ResolvedRequest>,
    pub arguments: Vec<WideText>,
}
impl DerivedCommand {
    pub fn expand(&self, record: &Record) -> Result<WideText, ResolveError> {
        let mut out = self.template.clone();
        for property in &record.properties {
            if let CimValue::String(v) | CimValue::Reference(v) = &property.value {
                out = replace(&out, &format!("#{}", property.name), v);
            }
        }
        if let Some(path) = &record.path {
            out = replace(&out, "#__PATH", path);
        }
        Ok(out)
    }
}
#[derive(Clone, Debug)]
pub enum ResolvedCommand {
    Management(Box<ResolvedRequest>),
    Derived(DerivedCommand),
    External(DerivedCommand),
    Control(Verb),
}
#[derive(Clone, Debug)]
pub struct AliasStore {
    fallback: bool,
    definitions: Vec<AliasDefinition>,
    role: WideText,
}
impl AliasStore {
    pub fn stock() -> Result<Self, ResolveError> {
        let mut store = Self::from_json(include_str!("../../assets/aliases.json"))?;
        store.fallback = false;
        Ok(store)
    }
    pub fn from_json(json: &str) -> Result<Self, ResolveError> {
        let definitions: Vec<AliasDefinition> = serde_json::from_str(json)
            .map_err(|e| error(0x80041008, &format!("Invalid alias metadata: {e}"), None))?;
        Self::validated(definitions, "root\\cli".into())
    }
    fn validated(definitions: Vec<AliasDefinition>, role: WideText) -> Result<Self, ResolveError> {
        let mut names: Vec<WideText> = vec![];
        for a in &definitions {
            if a.friendly_name.is_empty()
                || names.iter().any(|n| n.eq_ignore_case(&a.friendly_name))
            {
                return Err(error(0x80041008, "Invalid alias metadata.", None));
            }
            names.push(a.friendly_name.as_str().into());
        }
        Ok(Self {
            definitions,
            role,
            fallback: false,
        })
    }
    pub fn virtual_query(
        &self,
        r: &ResolvedRequest,
        node: &WideText,
    ) -> Option<Result<ManagementResult, ManagementError>> {
        (self.fallback
            && r.class_name.eq_ignore_case("MSFT_CliAlias")
            && r.connection.namespace.eq_ignore_case("root\\cli")
            && r.connection.node.eq_ignore_case("."))
        .then(|| virtual_role::query(r, node))
    }
    pub fn definitions(&self) -> &[AliasDefinition] {
        &self.definitions
    }
    pub fn find(&self, name: &WideText) -> Option<&AliasDefinition> {
        self.definitions
            .iter()
            .find(|a| name.eq_ignore_case(&a.friendly_name))
    }
    pub fn load_role(
        &mut self,
        settings: &SessionSettings,
        loader: &mut dyn RoleLoader,
    ) -> Result<(), ResolveError> {
        match loader.load(&settings.role)? {
            Some(defs) => *self = Self::validated(defs, settings.role.clone())?,
            None if settings.role.eq_ignore_case("root\\cli") => {
                *self = Self::stock()?;
                self.fallback = true;
            }
            None => return Err(error(0x8004100e, "Invalid namespace", None)),
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        command: &ParsedCommand,
        settings: &SessionSettings,
    ) -> Result<ResolvedCommand, ResolveError> {
        if settings.explicit_role
            && settings.role != self.role
            && !settings.role.eq_ignore_case(&self.role.to_string_lossy())
        {
            return Err(error(0x8004100e, "Invalid namespace", None));
        }
        let Some(target) = &command.target else {
            return Ok(ResolvedCommand::Control(command.verb));
        };
        let mut req = ResolvedRequest {
            render_context: None,
            format_spec: None,
            connection: ConnectionOptions {
                node: settings
                    .nodes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ".".into()),
                namespace: settings.namespace.clone(),
                locale: None,
                ..Default::default()
            },
            class_name: WideText::default(),
            instance_path: None,
            class_mode: matches!(target, Target::Class(_)),
            query: None,
            selector: command.selector.clone(),
            verb: if !command.verb_explicit && command.verb == Verb::List {
                Verb::Get
            } else {
                command.verb
            },
            properties: command.properties.clone(),
            method: command.method.clone(),
            arguments: if command.raw_arguments.is_empty() {
                command.arguments.clone()
            } else {
                command.raw_arguments.clone()
            },
            modifiers: command
                .modifiers
                .iter()
                .filter(|m| m.scope != ModifierScope::Global || !m.name.eq_ignore_case("password"))
                .cloned()
                .collect(),
            view: command.view.clone(),
            display: vec![],
            format: None,
            association: None,
        };
        if !matches!(target, Target::Alias(_))
            && command.verb == Verb::List
            && command.verb_explicit
        {
            return Err(error(
                0xABE7,
                "LIST verb is allowed only with <alias> name.",
                None,
            ));
        }
        if command.verb == Verb::Assoc {
            let mut filters = crate::backend::AssociationFilters::default();
            for m in &command.modifiers {
                if let SwitchValue::Value(v) = &m.value {
                    if m.name.eq_ignore_case("resultclass") {
                        filters.result_class = Some(v.clone())
                    } else if m.name.eq_ignore_case("resultrole") {
                        filters.result_role = Some(v.clone())
                    } else if m.name.eq_ignore_case("assocclass") {
                        filters.assoc_class = Some(v.clone())
                    }
                }
            }
            req.association = Some(filters);
        }
        if let Target::Alias(name) = target {
            let a = self
                .find(name)
                .ok_or_else(|| error(0xAC67, "Alias not found.", Some(name.clone())))?;
            if let Some(ns) = &a.connection.namespace {
                req.connection.namespace = ns.as_str().into();
            }
            req.connection.locale = a.connection.locale.as_deref().map(Into::into);
            req.connection.user = a.connection.user.as_deref().map(Into::into);
            req.connection.password = a.connection.password.clone();
            if !settings.explicit_nodes
                && let Some(server) = &a.connection.server
            {
                req.connection.node = server.as_str().into();
            }
            req.connection.authority = a.connection.authority.as_deref().map(Into::into);
            let mut pos = command.positionals.clone();
            let mut method = command.method.clone();
            if !command.verb_explicit
                && let Some(last) = pos.last()
                && a.verbs.iter().any(|v| last.eq_ignore_case(&v.name))
            {
                method = pos.pop();
                req.verb = Verb::Call;
            }
            req.query = Some(a.target.as_str().into());
            req.class_name = target_class(&a.target);
            if !pos.is_empty() {
                let template = a
                    .p_where
                    .as_deref()
                    .filter(|p| !p.is_empty())
                    .ok_or_else(|| {
                        error(0xABEA, "'PWhere' expression for the alias undefined.", None)
                    })?;
                if pos.len() == 1 && pos[0].0.contains(&46) && pos[0].0.contains(&61) {
                    req.instance_path = Some(pos[0].clone());
                    req.query = None;
                    req.selector = None;
                } else {
                    let expanded = substitute(template, &pos);
                    let text = expanded.trim_ascii();
                    let prefix: Vec<u16> = "where ".encode_utf16().collect();
                    if text.0.len() > 5 && WideText(text.0[..5].to_vec()).eq_ignore_case("from ") {
                        let mut query: WideText = "SELECT * ".into();
                        query.0.extend(&text.0);
                        req.class_name = target_class(&query.to_string_lossy());
                        req.query = Some(query);
                        req.selector = None;
                    } else {
                        req.selector = Some(
                            if text.0.len() >= prefix.len()
                                && text.0[..prefix.len()]
                                    .iter()
                                    .zip(prefix)
                                    .all(|(a, b)| a.to_ascii_lowercase() == b)
                            {
                                WideText(text.0[6..].to_vec())
                            } else {
                                text
                            },
                        );
                    }
                }
                if let Some(explicit) = &command.selector {
                    let mut combined = req.selector.take().unwrap_or_default();
                    combined.0.extend(" WHERE ".encode_utf16());
                    combined.0.extend(&explicit.0);
                    req.selector = Some(combined);
                }
            }
            let view = command
                .view
                .as_ref()
                .map(|v| v.to_string_lossy())
                .unwrap_or_else(|| "FULL".to_owned());
            if let Some(f) = a
                .formats
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(&view))
            {
                req.display = f
                    .properties
                    .iter()
                    .map(|p| {
                        (
                            p.name.as_str().into(),
                            p.derivation.as_deref().unwrap_or(&p.name).into(),
                        )
                    })
                    .collect();
                req.format = f.format.as_deref().map(Into::into);
            }
            if req.verb == Verb::Call
                && let Some(m) = &method
            {
                if let Some(v) = a.verbs.iter().find(|v| m.eq_ignore_case(&v.name)) {
                    if v.verb_type == Some(2) {
                        let d = DerivedCommand {
                            template: v.derivation.as_str().into(),
                            selection: Box::new(req),
                            arguments: if command.raw_arguments.is_empty() {
                                command.arguments.clone()
                            } else {
                                command.raw_arguments.clone()
                            },
                        };
                        let first = v.derivation.split_whitespace().next().unwrap_or("");
                        return Ok(
                            if first.eq_ignore_ascii_case("wmic")
                                || first.eq_ignore_ascii_case("wmic.exe")
                            {
                                ResolvedCommand::Derived(d)
                            } else {
                                ResolvedCommand::External(d)
                            },
                        );
                    }
                    req.method = Some(v.derivation.as_str().into());
                } else {
                    req.method = method;
                }
            }
        } else if let Target::Path(path) | Target::Class(path) = target {
            req.class_name = WideText(
                path.0
                    .iter()
                    .copied()
                    .take_while(|u| *u != b'.' as u16 && *u != b'=' as u16)
                    .collect(),
            );
            if path.0.contains(&(b'=' as u16)) {
                req.instance_path = Some(path.clone());
            }
        }
        Ok(ResolvedCommand::Management(Box::new(req)))
    }
}
fn target_class(target: &str) -> WideText {
    let words: Vec<_> = target.split_whitespace().collect();
    words
        .iter()
        .position(|w| w.eq_ignore_ascii_case("from"))
        .and_then(|i| words.get(i + 1))
        .copied()
        .unwrap_or(target)
        .into()
}
fn substitute(template: &str, values: &[WideText]) -> WideText {
    let mut out = Vec::new();
    let mut it = template.encode_utf16().peekable();
    let mut n = 0;
    while let Some(u) = it.next() {
        if u == 35 {
            out.extend(values.get(n).cloned().unwrap_or_default().0);
            n += 1;
            while it
                .peek()
                .is_some_and(|u| u.is_ascii_alphanumeric() || *u == 95)
            {
                it.next();
            }
        } else {
            out.push(u);
        }
    }
    WideText(out)
}
fn replace(source: &WideText, key: &str, value: &WideText) -> WideText {
    let key: Vec<u16> = key.encode_utf16().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < source.0.len() {
        if source.0[i..].starts_with(&key) {
            out.extend_from_slice(&value.0);
            i += key.len();
        } else {
            out.push(source.0[i]);
            i += 1;
        }
    }
    WideText(out)
}
trait AsciiUnit {
    fn to_ascii_lowercase(self) -> u16;
    fn is_ascii_alphanumeric(&self) -> bool;
}
impl AsciiUnit for u16 {
    fn to_ascii_lowercase(self) -> u16 {
        if (65..=90).contains(&self) {
            self + 32
        } else {
            self
        }
    }
    fn is_ascii_alphanumeric(&self) -> bool {
        (65..=90).contains(self) || (97..=122).contains(self) || (48..=57).contains(self)
    }
}
