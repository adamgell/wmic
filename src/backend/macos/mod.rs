//! macOS backend: Win32 classes in `root\cimv2` served from native system interfaces.
mod methods;
mod query;
mod write;

pub mod context;
pub mod error;
pub mod provider;
pub mod providers;
pub mod record;
pub mod schema;
pub mod sys;

use crate::{
    backend::{Confirmation, Management, ResolvedRequest},
    model::{ManagementError, ManagementResult, ObjectSet, Verb, WideText},
};
use context::Context;
use provider::registry;

pub struct MacManagement;

impl MacManagement {
    pub fn new() -> Result<Self, ManagementError> {
        Ok(Self)
    }
}

/// Accepts `root\cimv2` with any case and any number of leading backslashes.
fn served_namespace(namespace: &WideText) -> bool {
    let text = namespace.to_string_lossy();
    text.trim_start_matches('\\')
        .eq_ignore_ascii_case(record::NAMESPACE)
}

impl Management for MacManagement {
    fn reachable(&mut self, node: &WideText, _timeout_ms: u32) -> bool {
        sys::host::is_local_node(&node.to_string_lossy())
    }

    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        self.run(request, None)
    }

    fn execute_with_confirmation(
        &mut self,
        request: &ResolvedRequest,
        confirmation: &mut dyn Confirmation,
    ) -> Result<ManagementResult, ManagementError> {
        self.run(request, Some(confirmation))
    }
}

impl MacManagement {
    fn run(
        &mut self,
        request: &ResolvedRequest,
        confirmation: Option<&mut dyn Confirmation>,
    ) -> Result<ManagementResult, ManagementError> {
        let relative = query::relative_request(request)?;
        let request = relative.as_ref();
        let node = &request.connection.node;
        if error::interrupted() {
            return Err(error::cancelled(node, None));
        }
        if !sys::host::is_local_node(&node.to_string_lossy()) {
            return Err(error::rpc_unavailable(node));
        }
        if !served_namespace(&request.connection.namespace) {
            return Err(error::invalid_namespace(node));
        }
        if request.verb == Verb::Assoc {
            return Err(error::not_supported(node, "query"));
        }
        let reads = matches!(request.verb, Verb::Get | Verb::List);
        let entry = registry()
            .get(&request.class_name.to_string_lossy())
            .ok_or_else(|| {
                error::unknown_class(
                    node,
                    reads && !request.class_mode && request.query.is_some(),
                )
            })?;
        let ctx = Context::new(request);
        match request.verb {
            Verb::Get | Verb::List if request.class_mode => {
                Ok(ManagementResult::Schema(entry.info.schema.clone()))
            }
            Verb::Get | Verb::List => Ok(ManagementResult::Objects(ObjectSet {
                objects: query::select(request, entry, &ctx)?,
                schema: None,
            })),
            Verb::Call => methods::call(request, entry, &ctx, confirmation),
            Verb::Create | Verb::Set | Verb::Delete => {
                write::execute(request, entry, &ctx, confirmation)
            }
            _ => Err(error::not_supported(node, "unimplemented operation")),
        }
    }
}
