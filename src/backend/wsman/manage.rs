//! Management backend for nodes reached over WS-Management.
use super::{
    Endpoint, InvalidPort, catalog,
    client::{Connector, Found, Target, WsmanClient, default_connector, listener_reachable},
    fault::detail_properties,
    listener_port,
    path::{Keys, ObjectPath, parse_object_path},
    record::infer_schema,
    schema::ClassInfo,
};
use crate::{
    alias::predicate::Predicate,
    backend::{
        Confirmation, Management, ResolvedRequest, build_query, cancellable_targets, keyword,
        methods::{InputError, bind_method},
    },
    model::{
        ClassSchema, ManagementError, ManagementResult, MethodResult, MethodSchema, ObjectSet,
        OperationResult, ParameterSchema, Record, Verb, WideText,
    },
};

/// Where the listener port comes from.
pub type PortSource = fn() -> Result<u16, InvalidPort>;

pub struct WsmanManagement {
    client: WsmanClient,
    port: PortSource,
}

fn error(node: &WideText, code: u32, message: &str, operation: &str) -> ManagementError {
    ManagementError {
        code,
        target: node.clone(),
        message: message.into(),
        operation: operation.into(),
        extended: vec![],
        partial: None,
    }
}

fn not_supported(node: &WideText, operation: &str) -> ManagementError {
    error(node, 0x8004100c, "Not supported", operation)
}

fn invalid_query(node: &WideText) -> ManagementError {
    error(node, 0x80041017, "Invalid query", "query")
}

fn invalid_port(node: &WideText, invalid: &InvalidPort) -> ManagementError {
    ManagementError {
        extended: detail_properties(&invalid.to_string(), None),
        ..error(node, 0x80041008, "Invalid parameter", "query")
    }
}

fn invalid_object_path(node: &WideText) -> ManagementError {
    error(node, 0x8004103a, "Invalid object path", "query")
}

/// Argument binding failures name the class, like the other backends.
fn input_error(class: &WideText, e: InputError) -> ManagementError {
    ManagementError {
        code: e.code,
        target: class.clone(),
        message: e.message,
        operation: "input".into(),
        extended: vec![],
        partial: None,
    }
}

fn cancelled(node: &WideText, partial: Option<ManagementResult>) -> ManagementError {
    ManagementError {
        code: 0xC000013A,
        target: node.clone(),
        message: "Operation cancelled".into(),
        operation: "cancel".into(),
        extended: vec![],
        partial: partial.map(Box::new),
    }
}

fn interrupted() -> bool {
    crate::platform::unix::console::interrupted()
}

/// Host name or address to connect to: the node without leading backslashes; `.` is this
/// machine.
pub fn host_of(node: &WideText) -> String {
    let text = node.to_string_lossy();
    let host = text.trim().trim_start_matches('\\');
    if host == "." { "localhost" } else { host }.to_owned()
}

/// Connection settings for a request: account from the `/user` and `/password` options.
fn target_of(request: &ResolvedRequest, port: u16) -> Target {
    let connection = &request.connection;
    let host = host_of(&connection.node);
    let (user, domain) = connection.identity_parts();
    let namespace = connection.namespace.to_string_lossy();
    let namespace = namespace.trim().trim_start_matches('\\');
    Target {
        endpoint: Endpoint {
            host: host.clone(),
            port,
            user: user.to_string_lossy(),
            domain: domain.to_string_lossy(),
            password: connection.password.clone().unwrap_or_default(),
        },
        namespace: if namespace.is_empty() {
            "root\\cimv2".into()
        } else {
            namespace.to_owned()
        },
        node: connection.node.clone(),
        server: host.to_uppercase(),
    }
}

/// Property names the command asks for; empty means all of them.
fn projection(request: &ResolvedRequest) -> Vec<String> {
    match request.verb {
        Verb::Get => request
            .properties
            .iter()
            .map(WideText::to_string_lossy)
            .collect(),
        Verb::List => request
            .display
            .iter()
            .map(|(_, source)| source.to_string_lossy())
            .collect(),
        _ => vec![],
    }
}

/// Drops the properties the server added, such as keys, that the command did not name.
fn keep_columns(records: &mut [Record], columns: &[String]) {
    if columns.is_empty() || columns.iter().any(|c| c == "*") {
        return;
    }
    for record in records {
        record
            .properties
            .retain(|p| p.system || columns.iter().any(|c| p.name.eq_ignore_case(c)));
    }
}

/// True when the object path names exactly the key properties of the class.
fn keys_fit(info: &ClassInfo, keys: &Keys) -> bool {
    match keys {
        Keys::None | Keys::Singleton => info.singleton || info.keys().is_empty(),
        Keys::Assigned(assigned) => {
            let class_keys = info.keys();
            assigned.len() == class_keys.len()
                && assigned.iter().all(|(name, _)| {
                    info.property(name)
                        .is_some_and(|(index, _)| class_keys.contains(&index))
                })
        }
    }
}

/// Query text for the instances a request selects.
fn wql_of(request: &ResolvedRequest) -> String {
    let mut shaped = request.clone();
    if shaped.verb == Verb::List && shaped.display.is_empty() {
        shaped.verb = Verb::Get;
    }
    build_query(&shaped).to_string_lossy()
}

/// A method the embedded schema does not describe: parameters are the named arguments, read as
/// text. Without a path or condition the call addresses the class.
fn unknown_method(request: &ResolvedRequest, name: &WideText) -> MethodSchema {
    let has_condition = request.instance_path.is_some()
        || request.selector.is_some()
        || request
            .query
            .as_ref()
            .is_some_and(|q| keyword(&q.0, "where").is_some());
    let input = request
        .arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| {
            let at = argument.0.iter().position(|u| *u == 61)?;
            Some(ParameterSchema {
                cim_type_name: None,
                name: WideText(argument.0[..at].to_vec()).trim_ascii(),
                cim_type: 8,
                id: Some(index as u32),
                optional: true,
                default_value: None,
            })
        })
        .collect();
    MethodSchema {
        name: name.clone(),
        input,
        output: vec![],
        is_static: !has_condition,
        qualifiers: vec![],
    }
}

fn confirm(
    request: &ResolvedRequest,
    confirmation: &mut Option<&mut dyn Confirmation>,
    prompt: &WideText,
) -> Result<bool, ManagementError> {
    match confirmation.as_deref_mut() {
        Some(c) => c.confirm(prompt),
        None => Err(input_error(
            &request.class_name,
            InputError {
                code: 0x80041008,
                message: "Confirmation input unavailable".into(),
            },
        )),
    }
}

impl WsmanManagement {
    pub fn new(connector: Connector) -> Self {
        Self::with_client(WsmanClient::new(connector))
    }

    pub fn with_client(client: WsmanClient) -> Self {
        Self {
            client,
            port: listener_port,
        }
    }

    /// Replaces the source of the listener port.
    pub fn with_port_source(mut self, port: PortSource) -> Self {
        self.port = port;
        self
    }

    /// Reaches nodes with NTLM over HTTP.
    pub fn connected() -> Self {
        Self::new(default_connector())
    }

    fn run(
        &mut self,
        request: &ResolvedRequest,
        confirmation: Option<&mut dyn Confirmation>,
    ) -> Result<ManagementResult, ManagementError> {
        let node = &request.connection.node;
        if interrupted() {
            return Err(cancelled(node, None));
        }
        let port = (self.port)().map_err(|e| invalid_port(node, &e))?;
        let target = target_of(request, port);
        match request.verb {
            Verb::Get | Verb::List if request.class_mode => self.schema(&target, request),
            Verb::Get | Verb::List => {
                let mut records = match self.select(&target, request) {
                    Ok(records) => records,
                    Err(mut error) => {
                        if let Some(ManagementResult::Objects(set)) = error.partial.as_deref_mut() {
                            keep_columns(&mut set.objects, &projection(request));
                        }
                        return Err(error);
                    }
                };
                keep_columns(&mut records, &projection(request));
                Ok(ManagementResult::Objects(ObjectSet {
                    objects: records,
                    schema: None,
                }))
            }
            Verb::Call => self.call(&target, request, confirmation),
            Verb::Create => Err(not_supported(node, "write")),
            Verb::Set => Err(not_supported(node, "update")),
            Verb::Delete => Err(not_supported(node, "delete")),
            Verb::Assoc => Err(not_supported(node, "query")),
            _ => Err(not_supported(node, "unimplemented operation")),
        }
    }

    fn schema(
        &mut self,
        target: &Target,
        request: &ResolvedRequest,
    ) -> Result<ManagementResult, ManagementError> {
        let class = request.class_name.to_string_lossy();
        if let Some(info) = catalog::lookup(&class) {
            return Ok(ManagementResult::Schema(info.schema.clone()));
        }
        Ok(ManagementResult::Schema(
            match self.client.first_instance(target, &class)? {
                Some(raw) => infer_schema(&class, &raw),
                None => ClassSchema {
                    name: class.as_str().into(),
                    ..Default::default()
                },
            },
        ))
    }

    /// Instances the request selects, as server records.
    fn select(
        &mut self,
        target: &Target,
        request: &ResolvedRequest,
    ) -> Result<Vec<Record>, ManagementError> {
        Ok(self
            .select_found(target, request)?
            .into_iter()
            .map(|f| f.record)
            .collect())
    }

    fn select_found(
        &mut self,
        target: &Target,
        request: &ResolvedRequest,
    ) -> Result<Vec<Found>, ManagementError> {
        let node = &request.connection.node;
        let class = request.class_name.to_string_lossy();
        let class = catalog::lookup(&class).map_or(class, |info| info.name.clone());
        let Some(path) = &request.instance_path else {
            let wql = wql_of(request);
            return if request.query.is_none() {
                self.client.enumerate_class(target, &class, Some(&wql))
            } else {
                self.client.enumerate(target, &class, Some(&wql))
            };
        };
        let parsed: ObjectPath =
            parse_object_path(&path.to_string_lossy()).ok_or_else(|| invalid_object_path(node))?;
        if let Some(info) = catalog::lookup(&class)
            && !keys_fit(&info, &parsed.keys)
        {
            return Err(invalid_object_path(node));
        }
        let predicate = request
            .selector
            .as_ref()
            .map(|text| Predicate::parse(text).map_err(|_| invalid_query(node)))
            .transpose()?;
        let mut found: Vec<Found> = self
            .client
            .get(target, &class, &parsed.selectors())?
            .into_iter()
            .collect();
        if let Some(predicate) = predicate {
            let mut kept = vec![];
            for item in found {
                if predicate
                    .matches(&item.record)
                    .map_err(|_| invalid_query(node))?
                {
                    kept.push(item);
                }
            }
            found = kept;
        }
        Ok(found)
    }

    fn call(
        &mut self,
        target: &Target,
        request: &ResolvedRequest,
        mut confirmation: Option<&mut dyn Confirmation>,
    ) -> Result<ManagementResult, ManagementError> {
        let node = &request.connection.node;
        let requested = request.method.clone().unwrap_or_default();
        let class_text = request.class_name.to_string_lossy();
        let info = catalog::lookup(&class_text);
        let class = info.as_ref().map_or(class_text, |i| i.name.clone());
        let method = match &info {
            Some(info) => info
                .schema
                .methods
                .iter()
                .find(|m| requested.eq_ignore_case(&m.name.to_string_lossy()))
                .cloned()
                .ok_or_else(|| {
                    let mut message = requested.clone();
                    message.0.extend(" - Invalid alias verb.".encode_utf16());
                    input_error(
                        &request.class_name,
                        InputError {
                            code: 0xABFD,
                            message,
                        },
                    )
                })?,
            None => unknown_method(request, &requested),
        };
        let input = bind_method(&method, &request.arguments)
            .map_err(|e| input_error(&request.class_name, e))?;
        let targets: Vec<(WideText, Vec<(String, String)>)> =
            if method.is_static || request.class_mode {
                vec![(request.class_name.clone(), vec![])]
            } else {
                let mut selecting = request.clone();
                selecting.verb = Verb::Get;
                selecting.properties.clear();
                self.select_found(target, &selecting)?
                    .into_iter()
                    .filter_map(|f| f.record.path.map(|p| (p, f.selectors)))
                    .collect()
            };
        let had_targets = !targets.is_empty();
        let streamed = confirmation.as_ref().is_some_and(|c| c.streams_methods());
        let interactive = crate::backend::write::interactive(request);
        let name = method.name.to_string_lossy();
        let mut results: Vec<MethodResult> = vec![];
        for item in cancellable_targets(targets, interrupted) {
            let (path, selectors) = item
                .map_err(|_| cancelled(node, Some(ManagementResult::Methods(results.clone()))))?;
            if interactive {
                let mut prompt: WideText = "Execute (".into();
                prompt.0.extend(&path.0);
                prompt.0.extend(")->".encode_utf16());
                prompt.0.extend(&method.name.0);
                prompt.0.extend("() (Y/N/?)? ".encode_utf16());
                if !confirm(request, &mut confirmation, &prompt)? {
                    continue;
                }
            }
            if interrupted() {
                return Err(cancelled(node, Some(ManagementResult::Methods(results))));
            }
            let outcome =
                match self
                    .client
                    .invoke(target, &class, &name, &selectors, Some(&method), &input)
                {
                    Ok(outcome) => outcome,
                    Err(mut e) => {
                        e.target = path;
                        if !results.is_empty() {
                            e.partial = Some(Box::new(ManagementResult::Methods(results)));
                        }
                        return Err(e);
                    }
                };
            let result = MethodResult {
                path,
                return_value: outcome.return_value,
                out_parameters: outcome.out_parameters,
                text: None,
            };
            if streamed {
                if let Some(c) = confirmation.as_deref_mut() {
                    c.method_completed(request, &result)?;
                }
            } else {
                results.push(result);
            }
        }
        if results.is_empty() && had_targets {
            return Ok(ManagementResult::Operation(OperationResult {
                action: Verb::Call,
                paths: vec![],
                instances: vec![Record::default()],
            }));
        }
        Ok(ManagementResult::Methods(results))
    }
}

impl Management for WsmanManagement {
    fn reachable(&mut self, node: &WideText, timeout_ms: u32) -> bool {
        (self.port)().is_ok_and(|port| listener_reachable(&host_of(node), port, timeout_ms))
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
