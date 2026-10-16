use super::SessionSettings;
use crate::{
    alias::{AliasStore, ResolveError, ResolvedCommand},
    backend::Management,
    model::*,
    parse::parse_command,
    render::{self, OutputError, RenderedOutput},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}
pub trait OutputSink {
    fn configure(&mut self, _settings: &SessionSettings) -> Result<(), OutputError> {
        Ok(())
    }
    fn command_context(&mut self, _context: &RenderContext) -> Result<(), OutputError> {
        Ok(())
    }
    fn context(&mut self, _context: &RenderContext, _node: &WideText) -> Result<(), OutputError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn clipboard(&mut self, _text: &WideText) -> Result<(), OutputError> {
        Err(OutputError("Clipboard unavailable".into()))
    }
    fn record_user(&self) -> WideText {
        format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").unwrap_or_default()
        )
        .into()
    }

    fn read_line(&mut self) -> Result<Option<WideText>, OutputError> {
        Ok(None)
    }
    fn read_secret(&mut self) -> Result<Option<WideText>, OutputError> {
        Ok(None)
    }
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError>;
}
pub trait RepeatScheduler {
    /// False means cancellation; no WMI timeout is implied by this interval.
    fn wait(&mut self, seconds: u32) -> bool;
}
pub struct ThreadScheduler;
impl RepeatScheduler for ThreadScheduler {
    fn wait(&mut self, seconds: u32) -> bool {
        std::thread::sleep(std::time::Duration::from_secs(seconds.into()));
        true
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutcome {
    pub exit_status: u32,
    pub exit_requested: bool,
    pub interactive_requested: bool,
    pub nodes: Vec<NodeOutcome>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeOutcome {
    pub node: WideText,
    pub exit_status: u32,
}
pub struct Session {
    sequence: u32,
    pub settings: SessionSettings,
    pub aliases: AliasStore,
    pub local_node: WideText,
}
impl Session {
    pub fn new(local_node: WideText) -> Result<Self, ResolveError> {
        Ok(Self {
            sequence: 0,
            settings: SessionSettings::default(),
            aliases: AliasStore::stock()?,
            local_node,
        })
    }
    pub fn execute(
        &mut self,
        raw: &WideText,
        mode: EntryMode,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
    ) -> CommandOutcome {
        self.execute_with_scheduler(raw, mode, backend, sink, &mut ThreadScheduler)
    }
    pub fn execute_with_scheduler(
        &mut self,
        raw: &WideText,
        mode: EntryMode,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
        scheduler: &mut dyn RepeatScheduler,
    ) -> CommandOutcome {
        if let Ok(command) = parse_command(raw, mode)
            && let Ok(settings) = self.settings.for_command(&command)
        {
            if sink.configure(&settings).is_err() {
                return emit(
                    sink,
                    RenderedOutput {
                        stderr: "Invalid file name.\r\r\n".into(),
                        ..Default::default()
                    },
                    0xABF7,
                );
            }
            let context = super::presentation::context(
                &command,
                &settings,
                &self.local_node,
                self.sequence + 1,
                0,
            );
            if sink.command_context(&context).is_err() {
                return CommandOutcome {
                    exit_status: 0x80070005,
                    ..Default::default()
                };
            }
        }
        let outcome = self.execute_repeated(raw, mode, backend, sink, scheduler);
        if sink.finish().is_err() {
            return CommandOutcome {
                exit_status: 0x80070005,
                ..outcome
            };
        }
        outcome
    }
    fn execute_repeated(
        &mut self,
        raw: &WideText,
        mode: EntryMode,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
        scheduler: &mut dyn RepeatScheduler,
    ) -> CommandOutcome {
        let interval = parse_command(raw, mode)
            .ok()
            .filter(|c| !c.help)
            .and_then(|command| {
                command
                    .modifiers
                    .into_iter()
                    .rev()
                    .find(|m| m.name.eq_ascii_ignore_case("every"))
            });
        let interval = match interval {
            None => None,
            Some(m) => match m.value {
                SwitchValue::Value(v) => match v.to_string_lossy().parse::<u32>() {
                    Ok(n) if n > 0 => Some(n),
                    _ => {
                        return emit(
                            sink,
                            RenderedOutput {
                                stderr: "Invalid GET switch.\r\r\n".into(),
                                ..Default::default()
                            },
                            0xACB4,
                        );
                    }
                },
                _ => {
                    return emit(
                        sink,
                        RenderedOutput {
                            stderr: "Invalid GET switch.\r\r\n".into(),
                            ..Default::default()
                        },
                        0xACB4,
                    );
                }
            },
        };
        self.sequence = self.sequence.saturating_add(1);
        let mut every_count = 0;
        loop {
            let outcome = self.execute_once(raw, mode, backend, sink, every_count);
            every_count += 1;
            if outcome.exit_requested
                || outcome.interactive_requested
                || interval.is_none()
                || !scheduler.wait(interval.unwrap_or(0))
            {
                return outcome;
            }
        }
    }
    fn execute_derived(
        &mut self,
        command: crate::alias::DerivedCommand,
        settings: &SessionSettings,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
        external: bool,
    ) -> CommandOutcome {
        let mut selection = *command.selection.clone();
        selection.verb = Verb::Get;
        selection.method = None;
        selection.properties.clear();
        configure_connection(&mut selection, settings);
        let nodes = if settings.explicit_nodes {
            settings.nodes.clone()
        } else {
            vec![selection.connection.node.clone()]
        };
        let mut status = 0;
        for node in nodes {
            selection.connection.node = node.clone();
            let result = backend.execute_with_confirmation(&selection, &mut SinkConfirmation(sink));
            match result {
                Ok(ManagementResult::Objects(set)) => {
                    for object in set.objects {
                        let expanded = match command.expand(&object) {
                            Ok(s) => s,
                            Err(e) => return emit(sink, render::render_resolve_error(&e), e.code),
                        };
                        if external {
                            match backend.launch_external(&expanded) {
                                Ok(code) => status = code,
                                Err(e) => {
                                    return emit(
                                        sink,
                                        render::render(render::RenderRequest {
                                            request: &selection,
                                            result: Err(&e),
                                            node: &node,
                                        })
                                        .unwrap_or_default(),
                                        e.code,
                                    );
                                }
                            }
                        } else {
                            if sink.write(OutputStream::Stdout, &"\r\n".into()).is_err() {
                                return CommandOutcome {
                                    exit_status: 0x80070005,
                                    ..Default::default()
                                };
                            }
                            let mut child = Session {
                                sequence: 0,
                                settings: SessionSettings::default(),
                                aliases: self.aliases.clone(),
                                local_node: self.local_node.clone(),
                            };
                            let _ = child.execute(&expanded, EntryMode::Process, backend, sink);
                            if sink.write(OutputStream::Stdout, &"\r\n".into()).is_err() {
                                return CommandOutcome {
                                    exit_status: 0x80070005,
                                    ..Default::default()
                                };
                            }
                        }
                    }
                }
                Err(e) => {
                    return emit(
                        sink,
                        render::render(render::RenderRequest {
                            request: &selection,
                            result: Err(&e),
                            node: &node,
                        })
                        .unwrap_or_default(),
                        e.code,
                    );
                }
                _ => {
                    return CommandOutcome {
                        exit_status: 0x8004100c,
                        ..Default::default()
                    };
                }
            }
        }
        CommandOutcome {
            exit_status: status,
            ..Default::default()
        }
    }
    fn execute_once(
        &mut self,
        raw: &WideText,
        mode: EntryMode,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
        every_count: u32,
    ) -> CommandOutcome {
        let parsed = match parse_command(raw, mode) {
            Ok(p) => p,
            Err(e) => return emit(sink, render::render_parse_error(&e), e.code),
        };
        let mut settings = match self.settings.for_command(&parsed) {
            Ok(s) => s,
            Err(e) => return emit(sink, render::render_parse_error(&e), e.code),
        };
        let selected = match &parsed.target {
            Some(Target::Alias(name)) => Some(name.clone()),
            _ => None,
        };
        if selected.is_some()
            || (parsed.help
                && parsed.target.is_none()
                && parsed.verb != Verb::Context
                && parsed
                    .modifiers
                    .iter()
                    .all(|m| m.name.eq_ascii_ignore_case("?") || m.value != SwitchValue::Missing))
        {
            let mut loader = BackendRoleLoader {
                backend,
                selected,
                locale: settings.locale.clone(),
            };
            if let Err(error) = self.aliases.load_role(&settings, &mut loader) {
                return emit(sink, render::render_resolve_error(&error), error.code);
            }
        }
        if parsed.help {
            let schema = if matches!(parsed.target, Some(Target::Path(_)))
                && parsed.verb == Verb::Get
                && parsed.verb_explicit
            {
                let mut req = match self.aliases.resolve(&parsed, &settings) {
                    Ok(ResolvedCommand::Management(r)) => r,
                    Err(error) => {
                        return emit(sink, render::render_resolve_error(&error), error.code);
                    }
                    _ => {
                        return CommandOutcome {
                            exit_status: 0x8004100c,
                            ..Default::default()
                        };
                    }
                };
                req.class_mode = true;
                configure_connection(&mut req, &settings);
                match backend.execute(&req) {
                    Ok(ManagementResult::Schema(s)) => Some(s),
                    Err(error) => {
                        let status = error.code;
                        let node = if req.connection.node.eq_ascii_ignore_case(".") {
                            self.local_node.clone()
                        } else {
                            req.connection.node.clone()
                        };
                        let output = render::render(render::RenderRequest {
                            request: &req,
                            result: Err(&error),
                            node: &node,
                        })
                        .unwrap_or_default();
                        return emit(sink, output, status);
                    }
                    _ => {
                        return CommandOutcome {
                            exit_status: 0x8004100c,
                            ..Default::default()
                        };
                    }
                }
            } else {
                None
            };
            if let Some(Target::Alias(name)) = &parsed.target
                && self.aliases.find(name).is_none()
                && let Err(error) = self.aliases.resolve(&parsed, &settings)
            {
                return emit(sink, render::render_resolve_error(&error), error.code);
            }
            return match render::help::render_help(render::help::HelpRequest {
                command: &parsed,
                aliases: &self.aliases,
                schema: schema.as_ref(),
            }) {
                Ok(output) => emit(sink, output, 0),
                Err(error) => {
                    emit_render_error(sink, error);
                    CommandOutcome {
                        exit_status: 0xACB2,
                        ..Default::default()
                    }
                }
            };
        }
        if parsed.verb == Verb::Exit {
            return CommandOutcome {
                exit_requested: true,
                ..Default::default()
            };
        }
        if parsed.verb == Verb::Interactive {
            self.settings = settings;
            return CommandOutcome {
                interactive_requested: true,
                ..Default::default()
            };
        }
        let resolved = match self.aliases.resolve(&parsed, &settings) {
            Ok(r) => r,
            Err(e) => return emit(sink, render::render_resolve_error(&e), e.code),
        };
        let resolved = match resolved {
            ResolvedCommand::Derived(command) => {
                return self.execute_derived(command, &settings, backend, sink, false);
            }
            ResolvedCommand::External(command) => {
                return self.execute_derived(command, &settings, backend, sink, true);
            }
            other => other,
        };
        let ResolvedCommand::Management(mut request) = resolved else {
            return emit(
                sink,
                RenderedOutput {
                    stderr: "Not supported\r\r\n".into(),
                    ..Default::default()
                },
                0x8004100c,
            );
        };
        if settings.user.is_some() && settings.password.is_none() {
            if sink
                .write(OutputStream::Stderr, &"Enter the password :".into())
                .is_err()
            {
                return CommandOutcome {
                    exit_status: 0x80070005,
                    ..Default::default()
                };
            }
            match sink.read_secret() {
                Ok(value) => {
                    settings.password =
                        Some(crate::backend::SecretWide::new(value.unwrap_or_default()))
                }
                Err(_) => {
                    return CommandOutcome {
                        exit_status: 0x80070005,
                        ..Default::default()
                    };
                }
            }
        }
        configure_connection(&mut request, &settings);
        request.render_context = Some(super::presentation::context(
            &parsed,
            &settings,
            &self.local_node,
            self.sequence,
            every_count,
        ));
        if matches!(request.verb, Verb::Get | Verb::List | Verb::Assoc)
            && let Err(error) = super::presentation::prepare(&mut request, &settings, backend)
        {
            return CommandOutcome {
                exit_status: emit_render_error(sink, error),
                ..Default::default()
            };
        }
        let components = super::presentation::components(&parsed, &request, &self.aliases);
        if let Some(context) = &mut request.render_context {
            context.components_xml = components;
        }
        let mut outcomes = vec![];
        let mut status = 0;
        let mut combined = vec![];
        let mut had_objects = false;
        let nodes = if settings.explicit_nodes {
            settings.nodes.clone()
        } else {
            vec![request.connection.node.clone()]
        };
        for target in &nodes {
            request.connection.node = target.clone();
            let node = if target.eq_ascii_ignore_case(".") {
                self.local_node.clone()
            } else {
                target.clone()
            };
            if let Some(context) = &request.render_context
                && sink.context(context, &node).is_err()
            {
                status = 0x80070005;
                break;
            }
            if settings.trace {
                let trace = format!(
                    "IWbemServices request: node={} namespace={} authlevel={} implevel={}\r\n",
                    node,
                    request.connection.namespace,
                    request.connection.authentication,
                    request.connection.impersonation
                );
                if sink.write(OutputStream::Stderr, &trace.into()).is_err() {
                    status = 0x80070005;
                    break;
                }
            }
            let mut result =
                if settings.failfast && !backend.reachable(&node, settings.failfast_timeout_ms) {
                    Err(ManagementError {
                        code: 0xABE3,
                        target: node.clone(),
                        message: "RPC Server not available.".into(),
                        operation: "query".into(),
                        extended: vec![],
                        partial: None,
                    })
                } else {
                    backend.execute_with_confirmation(&request, &mut SinkConfirmation(sink))
                };
            match &mut result {
                Ok(value) => mark_node(value, &node),
                Err(error) => {
                    if let Some(value) = &mut error.partial {
                        mark_node(value, &node)
                    }
                }
            }
            had_objects |= matches!(result, Ok(ManagementResult::Objects(_)));
            let code = result.as_ref().err().map_or(0, |e| e.code);
            outcomes.push(NodeOutcome {
                node: node.clone(),
                exit_status: code,
            });
            if code != 0 {
                status = code;
            }
            if settings.aggregate
                && let Ok(ManagementResult::Objects(set)) = &result
            {
                combined.extend(set.objects.clone());
                continue;
            }
            if let Err(error) = &result
                && let Some(partial) = &error.partial
                && let ManagementResult::Objects(set) = partial.as_ref()
            {
                if settings.aggregate {
                    combined.extend(set.objects.clone());
                } else {
                    match render::render(render::RenderRequest {
                        request: &request,
                        result: Ok(partial.as_ref()),
                        node: &node,
                    }) {
                        Ok(output) => {
                            let outcome = emit(sink, output, code);
                            if outcome.exit_status != code {
                                status = outcome.exit_status;
                            }
                        }
                        Err(error) => status = emit_render_error(sink, error),
                    }
                }
            }
            match render::render(render::RenderRequest {
                request: &request,
                result: result.as_ref(),
                node: &node,
            }) {
                Ok(output) => {
                    let out = emit(sink, output, code);
                    if out.exit_status != code {
                        status = out.exit_status;
                    }
                }
                Err(error) => {
                    status = emit_render_error(sink, error);
                }
            }
        }
        if settings.aggregate && (had_objects || !combined.is_empty()) {
            let result = ManagementResult::Objects(ObjectSet {
                objects: combined,
                schema: None,
            });
            match render::render(render::RenderRequest {
                request: &request,
                result: Ok(&result),
                node: &self.local_node,
            }) {
                Ok(output) => status = emit(sink, output, status).exit_status,
                Err(error) => status = emit_render_error(sink, error),
            }
        }
        CommandOutcome {
            exit_status: status,
            nodes: outcomes,
            ..Default::default()
        }
    }
}
fn emit_render_error(sink: &mut dyn OutputSink, error: render::RenderError) -> u32 {
    let code = error.code;
    let mut stderr = error.message;
    stderr.0.extend("\r\r\n".encode_utf16());
    emit(
        sink,
        RenderedOutput {
            stderr,
            ..Default::default()
        },
        code,
    )
    .exit_status
}
pub(super) fn emit(
    sink: &mut dyn OutputSink,
    output: RenderedOutput,
    status: u32,
) -> CommandOutcome {
    let stdout = sink.write(OutputStream::Stdout, &output.stdout);
    let stderr = sink.write(OutputStream::Stderr, &output.stderr);
    CommandOutcome {
        exit_status: if stdout.is_err() || stderr.is_err() {
            0x80070005
        } else {
            status
        },
        ..Default::default()
    }
}

struct BackendRoleLoader<'a> {
    backend: &'a mut dyn Management,
    selected: Option<WideText>,
    locale: WideText,
}
impl crate::alias::RoleLoader for BackendRoleLoader<'_> {
    fn load(
        &mut self,
        role: &WideText,
    ) -> Result<Option<Vec<crate::alias::AliasDefinition>>, ResolveError> {
        self.backend
            .load_aliases(role, self.selected.as_ref(), &self.locale)
    }
}

fn configure_connection(request: &mut crate::backend::ResolvedRequest, settings: &SessionSettings) {
    if settings.interactive {
        request.modifiers.insert(
            0,
            Modifier {
                raw_value: None,
                name: "interactive".into(),
                value: SwitchValue::Value("on".into()),
                scope: ModifierScope::Global,
            },
        );
    }
    request.connection.authentication = settings.authentication;
    request.connection.impersonation = settings.impersonation;
    request.connection.privileges = settings.privileges;
    request.connection.locale = Some(settings.locale.clone());
    if settings.user.is_some() {
        request.connection.user = settings.user.clone();
        request.connection.password = settings.password.clone();
    }
    if settings.authority.is_some() {
        request.connection.authority = settings.authority.clone();
    }
}

struct SinkConfirmation<'a>(&'a mut dyn OutputSink);
impl crate::backend::Confirmation for SinkConfirmation<'_> {
    fn streams_methods(&self) -> bool {
        true
    }
    fn method_completed(
        &mut self,
        request: &crate::backend::ResolvedRequest,
        result: &MethodResult,
    ) -> Result<(), ManagementError> {
        let value = ManagementResult::Methods(vec![result.clone()]);
        let output = render::render(render::RenderRequest {
            request,
            result: Ok(&value),
            node: &request.connection.node,
        })
        .map_err(|e| ManagementError {
            code: e.code,
            target: request.class_name.clone(),
            message: e.message,
            operation: "input".into(),
            extended: vec![],
            partial: None,
        })?;
        let outcome = emit(self.0, output, 0);
        if outcome.exit_status != 0 {
            return Err(ManagementError {
                code: outcome.exit_status,
                target: request.class_name.clone(),
                message: "Output failed".into(),
                operation: "input".into(),
                extended: vec![],
                partial: None,
            });
        }
        Ok(())
    }
    fn confirm(&mut self, prompt: &WideText) -> Result<bool, ManagementError> {
        let result: Result<Option<WideText>, OutputError> = (|| {
            loop {
                self.0.write(OutputStream::Stderr, prompt)?;
                let answer = self.0.read_line()?;
                if answer
                    .as_ref()
                    .is_some_and(|a| a.trim_ascii().eq_ascii_ignore_case("?"))
                {
                    self.0.write(
                        OutputStream::Stderr,
                        &"Invalid XSL format (or) file name.\r\r\n".into(),
                    )?;
                } else {
                    break Ok(answer);
                }
            }
        })();
        result
            .map(|answer| answer.is_some_and(|a| a.trim_ascii().eq_ascii_ignore_case("y")))
            .map_err(|e| ManagementError {
                code: 0x80070005,
                target: WideText::default(),
                message: e.to_string().into(),
                operation: "input".into(),
                extended: vec![],
                partial: None,
            })
    }
}

fn mark_node(result: &mut ManagementResult, node: &WideText) {
    match result {
        ManagementResult::Objects(set) => {
            for object in &mut set.objects {
                object.result_node = Some(node.clone())
            }
        }
        ManagementResult::Associations(groups) => {
            for group in groups {
                group.source.result_node = Some(node.clone());
                for object in &mut group.associated {
                    object.result_node = Some(node.clone())
                }
            }
        }
        _ => {}
    }
}
