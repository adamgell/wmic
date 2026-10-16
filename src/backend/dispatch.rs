//! Routes each request to the local macOS backend or to a remote node over WS-Management.
use crate::{
    backend::{
        Confirmation, ConnectionOptions, Management, ResolvedRequest,
        macos::{MacManagement, sys::host::is_local_node},
        wsman::{client::Connector, manage::WsmanManagement},
    },
    model::{ManagementError, ManagementResult, WideText},
};

pub struct Dispatcher {
    local: MacManagement,
    remote: WsmanManagement,
}

/// A request is remote when it names another node, or supplies an account: credentials only
/// make sense for a remote logon.
pub fn is_remote(connection: &ConnectionOptions) -> bool {
    !is_local_node(&connection.node.to_string_lossy()) || connection.user.is_some()
}

impl Dispatcher {
    pub fn new() -> Result<Self, ManagementError> {
        Self::with_remote(WsmanManagement::connected())
    }

    pub fn with_connector(connector: Connector) -> Result<Self, ManagementError> {
        Self::with_remote(WsmanManagement::new(connector))
    }

    fn with_remote(remote: WsmanManagement) -> Result<Self, ManagementError> {
        Ok(Self {
            local: MacManagement::new()?,
            remote,
        })
    }
}

impl Management for Dispatcher {
    fn load_translation(
        &mut self,
        role: &WideText,
        name: &WideText,
    ) -> Result<Option<crate::render::TranslationEntries>, crate::alias::ResolveError> {
        self.local.load_translation(role, name)
    }

    fn launch_external(&mut self, command: &WideText) -> Result<u32, ManagementError> {
        self.local.launch_external(command)
    }

    fn load_alias_inventory(
        &mut self,
        role: &WideText,
        locale: &WideText,
    ) -> Result<Option<Vec<crate::alias::AliasDefinition>>, crate::alias::ResolveError> {
        self.local.load_alias_inventory(role, locale)
    }

    fn load_aliases(
        &mut self,
        role: &WideText,
        selected: Option<&WideText>,
        locale: &WideText,
    ) -> Result<Option<Vec<crate::alias::AliasDefinition>>, crate::alias::ResolveError> {
        self.local.load_aliases(role, selected, locale)
    }

    fn reachable(&mut self, node: &WideText, timeout_ms: u32) -> bool {
        if is_local_node(&node.to_string_lossy()) {
            self.local.reachable(node, timeout_ms)
        } else {
            self.remote.reachable(node, timeout_ms)
        }
    }

    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        if is_remote(&request.connection) {
            self.remote.execute(request)
        } else {
            self.local.execute(request)
        }
    }

    fn execute_with_confirmation(
        &mut self,
        request: &ResolvedRequest,
        confirmation: &mut dyn Confirmation,
    ) -> Result<ManagementResult, ManagementError> {
        if is_remote(&request.connection) {
            self.remote.execute_with_confirmation(request, confirmation)
        } else {
            self.local.execute_with_confirmation(request, confirmation)
        }
    }
}
