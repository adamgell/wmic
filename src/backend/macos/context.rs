use super::{error, provider::registry, record::RecordBuilder, sys::host};
use crate::{
    backend::{ResolvedRequest, keyword},
    model::{ManagementError, Verb, WideText},
};
use std::collections::HashSet;

/// Per-request state handed to providers.
pub struct Context {
    node: WideText,
    server: String,
    /// Lower-case names of properties the request reads; `None` when it reads all of them.
    wanted: Option<HashSet<String>>,
}

fn words(text: &WideText, out: &mut HashSet<String>) {
    let text = text.to_string_lossy();
    out.extend(
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .map(str::to_ascii_lowercase),
    );
}

impl Context {
    pub fn new(request: &ResolvedRequest) -> Self {
        let names: Vec<&WideText> = match request.verb {
            Verb::Get => request.properties.iter().collect(),
            Verb::List => request.display.iter().map(|(_, source)| source).collect(),
            _ => vec![],
        };
        let wanted = if names.is_empty() || names.iter().any(|n| n.0 == [42]) {
            None
        } else {
            let mut set = HashSet::new();
            for name in names {
                words(name, &mut set);
            }
            if let Some(selector) = &request.selector {
                words(selector, &mut set);
            }
            if let Some(query) = &request.query
                && let Some(at) = keyword(&query.0, "where")
            {
                words(&WideText(query.0[at..].to_vec()), &mut set);
            }
            Some(set)
        };
        Self {
            node: request.connection.node.clone(),
            server: host::server_name(),
            wanted,
        }
    }

    /// Context that reads every property.
    pub fn unrestricted() -> Self {
        Self {
            node: ".".into(),
            server: host::server_name(),
            wanted: None,
        }
    }

    /// Upper-cased first host-name label used in object paths.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// False when no part of the request reads the property. Providers use it to skip costly,
    /// non-key properties.
    pub fn wants(&self, property: &str) -> bool {
        self.wanted
            .as_ref()
            .is_none_or(|set| set.contains(&property.to_ascii_lowercase()))
    }

    /// Starts a record of a registered class.
    pub fn record(&self, class: &str) -> RecordBuilder<'_> {
        let entry = registry()
            .get(class)
            .unwrap_or_else(|| panic!("class {class} is not registered"));
        RecordBuilder::new(&entry.info, &self.server)
    }

    pub fn error(&self, code: u32, message: &str, operation: &str) -> ManagementError {
        error::error(&self.node, code, message, operation)
    }

    pub fn not_supported(&self, operation: &str) -> ManagementError {
        error::not_supported(&self.node, operation)
    }
}
