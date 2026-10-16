//! Routes each request to the local macOS backend or to a remote node over WS-Management.
use crate::{
    backend::{
        Confirmation, ConnectionOptions, Management, ResolvedRequest,
        macos::{MacManagement, sys::host::is_local_node},
        wsman::{client::Connector, manage::WsmanManagement, port_is_set},
    },
    model::{ManagementError, ManagementResult, WideText},
};

pub struct Dispatcher {
    local: MacManagement,
    remote: WsmanManagement,
    /// The listener port is named explicitly, so an account on the local node reaches a tunnel
    /// instead of being refused.
    tunnel: bool,
}

/// A request is remote when it names another node, or supplies an account: credentials only
/// make sense for a remote logon.
pub fn is_remote(connection: &ConnectionOptions) -> bool {
    !is_local_node(&connection.node.to_string_lossy()) || connection.user.is_some()
}

fn credentials_on_local_node(connection: &ConnectionOptions) -> ManagementError {
    ManagementError {
        code: 0x80041064,
        target: connection.node.clone(),
        message: "User credentials cannot be used for local connections".into(),
        operation: "query".into(),
        extended: vec![],
        partial: None,
    }
}

impl Dispatcher {
    pub fn new() -> Result<Self, ManagementError> {
        Self::with_remote(WsmanManagement::connected(), port_is_set())
    }

    pub fn with_connector(connector: Connector) -> Result<Self, ManagementError> {
        Self::with_remote(WsmanManagement::new(connector), port_is_set())
    }

    /// Like `with_connector`, with the tunnel setting given instead of read from the
    /// environment.
    pub fn with_connector_and_tunnel(
        connector: Connector,
        tunnel: bool,
    ) -> Result<Self, ManagementError> {
        Self::with_remote(WsmanManagement::new(connector), tunnel)
    }

    fn with_remote(remote: WsmanManagement, tunnel: bool) -> Result<Self, ManagementError> {
        Ok(Self {
            local: MacManagement::new()?,
            remote,
            tunnel,
        })
    }

    /// True for the remote backend. A local node with an account is an error unless the
    /// listener port is named explicitly.
    fn routes_remote(&self, connection: &ConnectionOptions) -> Result<bool, ManagementError> {
        if is_local_node(&connection.node.to_string_lossy()) && connection.user.is_some() {
            return if self.tunnel {
                Ok(true)
            } else {
                Err(credentials_on_local_node(connection))
            };
        }
        Ok(is_remote(connection))
    }

    /// Whether the node of a request can be reached, judged by the backend that serves the
    /// request.
    pub fn reachable_for(&mut self, connection: &ConnectionOptions, timeout_ms: u32) -> bool {
        match self.routes_remote(connection) {
            Ok(true) => self.remote.reachable(&connection.node, timeout_ms),
            Ok(false) | Err(_) => self.local.reachable(&connection.node, timeout_ms),
        }
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
        let connection = ConnectionOptions {
            node: node.clone(),
            ..Default::default()
        };
        self.reachable_for(&connection, timeout_ms)
    }

    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        if self.routes_remote(&request.connection)? {
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
        if self.routes_remote(&request.connection)? {
            self.remote.execute_with_confirmation(request, confirmation)
        } else {
            self.local.execute_with_confirmation(request, confirmation)
        }
    }
}
