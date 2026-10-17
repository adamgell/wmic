//! WS-Management operations against one WinRM endpoint per node, with fault mapping.
use super::{
    Endpoint, Response, Transport, TransportError,
    fault::{self, Fault, FaultContext, detail_properties},
    parse::{self, Epr, RawInstance, RawItem, ReplyError},
    record::{self, RecordContext},
    soap::{self, Call},
    transport::ntlm,
};
use crate::model::{
    CimValue, ManagementError, ManagementResult, MethodSchema, ObjectSet, ParameterSchema,
    Property, Record, WideText,
};
use std::{
    collections::HashMap,
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

/// Items per Enumerate and per Pull. The service rejects a Pull whose value differs from the
/// value the enumeration started with.
pub const MAX_ELEMENTS: u32 = 32;

/// Creates the transport for an endpoint.
pub type Connector = Box<dyn FnMut(&Endpoint) -> Result<Box<dyn Transport>, TransportError>>;

/// NTLM over HTTP with sealed messages.
pub fn default_connector() -> Connector {
    Box::new(|endpoint| {
        super::transport::NtlmHttpTransport::connect(endpoint)
            .map(|transport| Box::new(transport) as Box<dyn Transport>)
    })
}

/// Everything a request needs to know about the node it addresses.
#[derive(Clone, Debug)]
pub struct Target {
    pub endpoint: Endpoint,
    /// Namespace as typed, for example `root\cimv2`.
    pub namespace: String,
    /// Node as typed; the target of errors.
    pub node: WideText,
    /// Value of `__SERVER` when the service does not report its computer name.
    pub server: String,
    /// The command names several nodes, so no class lookup precedes the query.
    pub multiple_nodes: bool,
}

impl Target {
    fn to(&self) -> String {
        soap::endpoint_url(&self.endpoint.host, self.endpoint.port)
    }
}

/// An instance with the selectors that address it.
#[derive(Clone, Debug)]
pub struct Found {
    pub record: Record,
    pub selectors: Vec<(String, String)>,
}

/// Result of a method call: the return value and the out parameters, typed by the schema.
#[derive(Clone, Debug, Default)]
pub struct MethodOutcome {
    pub return_value: Option<CimValue>,
    pub out_parameters: Vec<Property>,
}

enum Failure {
    Fault(Box<Fault>),
    Transport(TransportError),
    Malformed(String),
    Cancelled,
}

struct Partial {
    failure: Failure,
    items: Vec<RawItem>,
}

impl From<ReplyError> for Failure {
    fn from(error: ReplyError) -> Self {
        match error {
            ReplyError::Fault(fault) => Self::Fault(fault),
            ReplyError::Malformed(text) => Self::Malformed(text),
        }
    }
}

/// How a request addressed its target, which decides the operation an error reports.
#[derive(Clone, Copy)]
struct Scope {
    operation: &'static str,
    by_path: bool,
}

const QUERY: Scope = Scope {
    operation: "query",
    by_path: false,
};
const LOOKUP: Scope = Scope {
    operation: "query",
    by_path: true,
};
const METHOD: Scope = Scope {
    operation: "method",
    by_path: true,
};

fn error_for(failure: Failure, target: &Target, scope: Scope) -> ManagementError {
    let plain = |code: u32, message: &str, operation: &str, detail: &str| ManagementError {
        code,
        target: target.node.clone(),
        message: message.into(),
        operation: operation.into(),
        extended: detail_properties(detail, None),
        partial: None,
    };
    match failure {
        Failure::Fault(fault) => fault::to_error(
            &fault,
            &FaultContext {
                node: &target.node,
                operation: scope.operation,
                by_path: scope.by_path && !target.multiple_nodes,
            },
        ),
        Failure::Transport(TransportError::Connect(e)) => plain(
            0x800706ba,
            "The RPC server is unavailable.",
            "query",
            &e.to_string(),
        ),
        Failure::Transport(TransportError::Authentication) => plain(
            0x80070005,
            "Access is denied.",
            "query",
            "authentication failed",
        ),
        Failure::Transport(other) => plain(
            0x80041001,
            "Generic failure",
            scope.operation,
            &other.to_string(),
        ),
        Failure::Malformed(text) => plain(0x80041001, "Generic failure", scope.operation, &text),
        Failure::Cancelled => ManagementError {
            code: 0xC000013A,
            target: target.node.clone(),
            message: "Operation cancelled".into(),
            operation: "cancel".into(),
            extended: vec![],
            partial: None,
        },
    }
}

/// Connections are shared by node and account. The password enters the key as a keyed hash, so a
/// changed password never reuses a connection opened with another one.
fn connection_key(endpoint: &Endpoint, salt: &[u8; 16]) -> String {
    let mut password: Vec<u8> = endpoint
        .password
        .expose()
        .0
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let mut proof = ntlm::hmac_md5(salt, &[&password]);
    ntlm::wipe(&mut password);
    let key = format!(
        "{}\0{}\0{}\0{}\0{}",
        endpoint.host.to_ascii_lowercase(),
        endpoint.port,
        endpoint.domain.to_ascii_lowercase(),
        endpoint.user.to_ascii_lowercase(),
        proof.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    ntlm::wipe(&mut proof);
    key
}

pub struct WsmanClient {
    connector: Connector,
    connections: HashMap<String, Box<dyn Transport>>,
    /// Computer name each connection reported while authenticating.
    names: HashMap<String, String>,
    salt: [u8; 16],
    interrupted: fn() -> bool,
}

impl WsmanClient {
    pub fn new(connector: Connector) -> Self {
        Self {
            connector,
            connections: HashMap::new(),
            names: HashMap::new(),
            salt: ntlm::random_bytes::<16>().unwrap_or_default(),
            interrupted: crate::platform::unix::console::interrupted,
        }
    }

    /// Replaces the interrupt check polled between pulls.
    pub fn with_interrupt(mut self, interrupted: fn() -> bool) -> Self {
        self.interrupted = interrupted;
        self
    }

    fn key(&self, endpoint: &Endpoint) -> String {
        connection_key(endpoint, &self.salt)
    }

    fn send(&mut self, endpoint: &Endpoint, envelope: &str) -> Result<Response, TransportError> {
        let key = self.key(endpoint);
        if !self.connections.contains_key(&key) {
            let transport = (self.connector)(endpoint)?;
            self.connections.insert(key.clone(), transport);
        }
        let outcome = match self.connections.get_mut(&key) {
            Some(transport) => {
                let outcome = transport.post(envelope);
                if outcome.is_ok()
                    && let Some(name) = transport.server_name()
                {
                    self.names.insert(key.clone(), name);
                }
                outcome
            }
            None => return Err(TransportError::Protocol("no connection".into())),
        };
        if outcome.is_err() {
            self.connections.remove(&key);
        }
        outcome
    }

    /// Value of `__SERVER` for records of a node: the computer name the service reported, else
    /// the typed host.
    fn server_name(&self, target: &Target) -> String {
        self.names
            .get(&self.key(&target.endpoint))
            .cloned()
            .unwrap_or_else(|| target.server.clone())
    }

    /// Posts an envelope. A request that cannot change state is repeated once on a new
    /// connection when a reused connection fails. A connection the peer already closed is
    /// replaced before the request is sent, so a request that can change state is sent once.
    fn exchange(
        &mut self,
        endpoint: &Endpoint,
        envelope: &str,
        repeatable: bool,
    ) -> Result<Response, Failure> {
        let key = self.key(endpoint);
        if let Some(transport) = self.connections.get_mut(&key)
            && !transport.is_reusable()
        {
            self.connections.remove(&key);
        }
        let reused = self.connections.contains_key(&key);
        let mut outcome = self.send(endpoint, envelope);
        if repeatable
            && reused
            && matches!(
                outcome,
                Err(TransportError::Io(_) | TransportError::Protocol(_))
            )
        {
            outcome = self.send(endpoint, envelope);
        }
        let response = outcome.map_err(Failure::Transport)?;
        if response.status == 401 {
            self.connections.remove(&key);
            return Err(Failure::Transport(TransportError::Authentication));
        }
        Ok(response)
    }

    fn call<T>(
        &mut self,
        endpoint: &Endpoint,
        envelope: &str,
        repeatable: bool,
        read: impl FnOnce(&str) -> Result<T, ReplyError>,
    ) -> Result<T, Failure> {
        let response = self.exchange(endpoint, envelope, repeatable)?;
        read(&response.body).map_err(|error| match error {
            ReplyError::Malformed(text) if response.status != 200 => {
                Failure::Malformed(format!("HTTP {}: {text}", response.status))
            }
            other => other.into(),
        })
    }

    fn release(&mut self, target: &Target, uri: &str, context: &str) {
        let id = soap::new_message_id();
        let to = target.to();
        let envelope = soap::release(
            &Call {
                to: &to,
                resource_uri: uri,
                message_id: &id,
            },
            context,
        );
        let _ = self.exchange(&target.endpoint, &envelope, false);
    }

    /// One enumeration from the first Enumerate to the last Pull. `limit` stops early and
    /// releases the server's context.
    fn enumerate_pass(
        &mut self,
        target: &Target,
        class: &str,
        wql: Option<&str>,
        max_elements: u32,
        limit: Option<usize>,
    ) -> Result<Vec<RawItem>, Partial> {
        let uri = soap::resource_uri(&target.namespace, if wql.is_some() { "*" } else { class });
        let to = target.to();
        let call = |id: &str| -> String {
            soap::enumerate(
                &Call {
                    to: &to,
                    resource_uri: &uri,
                    message_id: id,
                },
                wql,
                max_elements,
            )
        };
        let start = call(&soap::new_message_id());
        let mut page = self
            .call(&target.endpoint, &start, true, parse::parse_enumeration)
            .map_err(|failure| Partial {
                failure,
                items: vec![],
            })?;
        let mut items = vec![];
        loop {
            items.append(&mut page.items);
            let context = page.context.clone().filter(|_| !page.end_of_sequence);
            if let Some(limit) = limit
                && items.len() >= limit
            {
                items.truncate(limit);
                if let Some(context) = &context {
                    self.release(target, &uri, context);
                }
                return Ok(items);
            }
            let Some(context) = context else {
                return Ok(items);
            };
            if (self.interrupted)() {
                self.release(target, &uri, &context);
                return Err(Partial {
                    failure: Failure::Cancelled,
                    items,
                });
            }
            let id = soap::new_message_id();
            let envelope = soap::pull(
                &Call {
                    to: &to,
                    resource_uri: &uri,
                    message_id: &id,
                },
                &context,
                max_elements,
            );
            match self.call(&target.endpoint, &envelope, false, parse::parse_enumeration) {
                Ok(next) => page = next,
                Err(Failure::Fault(fault)) if fault.is_timed_out() => {
                    // The service keeps the context after a Pull deadline, so the same context is
                    // pulled again. Only the interrupt check above ends this loop.
                    continue;
                }
                Err(failure) => {
                    let alive = match &failure {
                        Failure::Fault(fault) => !fault.is_stale_context(),
                        Failure::Malformed(_) => true,
                        Failure::Transport(TransportError::Io(_) | TransportError::Protocol(_)) => {
                            true
                        }
                        _ => false,
                    };
                    if alive {
                        self.release(target, &uri, &context);
                    }
                    return Err(Partial { failure, items });
                }
            }
        }
    }

    /// Enumerates with one restart when the server drops the context mid-way.
    fn enumerate_raw(
        &mut self,
        target: &Target,
        class: &str,
        wql: Option<&str>,
        max_elements: u32,
        limit: Option<usize>,
    ) -> Result<Vec<RawItem>, Partial> {
        let first = self.enumerate_pass(target, class, wql, max_elements, limit);
        match first {
            Err(Partial {
                failure: Failure::Fault(fault),
                ..
            }) if fault.is_stale_context() => {
                self.enumerate_pass(target, class, wql, max_elements, limit)
            }
            other => other,
        }
    }

    fn found(&self, target: &Target, item: RawItem, class: &str) -> Option<Found> {
        let raw = item.instance?;
        let server = self.server_name(target);
        let context = RecordContext {
            server: &server,
            namespace: &target.namespace,
        };
        let record = record::build(&raw, item.epr.as_ref(), &context, class);
        let selectors = item.epr.map(|e| e.selectors).unwrap_or_default();
        Some(Found { record, selectors })
    }

    /// Every instance matching the class, or the WQL query when one is given. The class names
    /// the target of fragments that carry no class of their own.
    pub fn enumerate(
        &mut self,
        target: &Target,
        class: &str,
        wql: Option<&str>,
    ) -> Result<Vec<Found>, ManagementError> {
        self.enumerate_scoped(target, class, wql, QUERY)
    }

    /// Like `enumerate`, for a request that names a class or an object path rather than a
    /// query: an unknown class reports "Not found" for the class.
    pub fn enumerate_class(
        &mut self,
        target: &Target,
        class: &str,
        wql: Option<&str>,
    ) -> Result<Vec<Found>, ManagementError> {
        self.enumerate_scoped(target, class, wql, LOOKUP)
    }

    fn enumerate_scoped(
        &mut self,
        target: &Target,
        class: &str,
        wql: Option<&str>,
        scope: Scope,
    ) -> Result<Vec<Found>, ManagementError> {
        match self.enumerate_raw(target, class, wql, MAX_ELEMENTS, None) {
            Ok(items) => Ok(items
                .into_iter()
                .filter_map(|item| self.found(target, item, class))
                .collect()),
            Err(Partial { failure, items }) => {
                let mut error = error_for(failure, target, scope);
                let objects: Vec<Record> = items
                    .into_iter()
                    .filter_map(|item| self.found(target, item, class))
                    .map(|f| f.record)
                    .collect();
                if !objects.is_empty() {
                    error.partial = Some(Box::new(ManagementResult::Objects(ObjectSet {
                        objects,
                        schema: None,
                    })));
                }
                Err(error)
            }
        }
    }

    /// The first instance of a class as it appears on the wire.
    pub fn first_instance(
        &mut self,
        target: &Target,
        class: &str,
    ) -> Result<Option<RawInstance>, ManagementError> {
        match self.enumerate_raw(target, class, None, 1, Some(1)) {
            Ok(items) => Ok(items.into_iter().find_map(|item| item.instance)),
            Err(Partial { failure, .. }) => Err(error_for(failure, target, LOOKUP)),
        }
    }

    /// One instance by key selectors; `None` when the class exists and the instance does not.
    pub fn get(
        &mut self,
        target: &Target,
        class: &str,
        selectors: &[(String, String)],
    ) -> Result<Option<Found>, ManagementError> {
        let uri = soap::resource_uri(&target.namespace, class);
        let to = target.to();
        let envelope = soap::get(
            &Call {
                to: &to,
                resource_uri: &uri,
                message_id: &soap::new_message_id(),
            },
            selectors,
        );
        match self.call(&target.endpoint, &envelope, true, parse::parse_get) {
            Ok(raw) => {
                let epr = Epr {
                    resource_uri: uri,
                    selectors: selectors.to_vec(),
                    references: vec![],
                };
                let server = self.server_name(target);
                let context = RecordContext {
                    server: &server,
                    namespace: &target.namespace,
                };
                let record = record::build(&raw, Some(&epr), &context, class);
                Ok(Some(Found {
                    record,
                    selectors: selectors.to_vec(),
                }))
            }
            Err(Failure::Fault(fault)) if fault.is_instance_not_found() => Ok(None),
            Err(failure) => Err(error_for(failure, target, LOOKUP)),
        }
    }

    /// Calls a method on a class (static) or an instance (selectors). A request that was sent
    /// is not repeated.
    pub fn invoke(
        &mut self,
        target: &Target,
        class: &str,
        method: &str,
        selectors: &[(String, String)],
        schema: Option<&MethodSchema>,
        inputs: &[Property],
    ) -> Result<MethodOutcome, ManagementError> {
        let uri = soap::resource_uri(&target.namespace, class);
        let to = target.to();
        let envelope = soap::invoke(
            &Call {
                to: &to,
                resource_uri: &uri,
                message_id: &soap::new_message_id(),
            },
            &target.namespace,
            method,
            selectors,
            inputs,
        );
        let raw = self
            .call(&target.endpoint, &envelope, false, parse::parse_invoke)
            .map_err(|failure| error_for(failure, target, METHOD))?;
        let declared: &[ParameterSchema] = schema.map_or(&[], |m| m.output.as_slice());
        let mut outcome = MethodOutcome::default();
        let server = self.server_name(target);
        let context = RecordContext {
            server: &server,
            namespace: &target.namespace,
        };
        for property in record::parameters(&raw, declared, &context) {
            if property.name.eq_ignore_case("ReturnValue") {
                outcome.return_value = Some(property.value);
            } else {
                outcome.out_parameters.push(property);
            }
        }
        Ok(outcome)
    }
}

/// True when a TCP connection to the listener opens within the timeout.
pub fn listener_reachable(host: &str, port: u16, timeout_ms: u32) -> bool {
    let timeout = Duration::from_millis(u64::from(timeout_ms.max(1)));
    (host, port).to_socket_addrs().is_ok_and(|mut addresses| {
        addresses.any(|a| TcpStream::connect_timeout(&a, timeout).is_ok())
    })
}
