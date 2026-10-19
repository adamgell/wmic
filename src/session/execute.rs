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
    fn consume_query_key(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn reset_command(&mut self, _settings: &SessionSettings) -> Result<(), OutputError> {
        Ok(())
    }
    fn clear_wait_prompt(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn console_input(&self) -> bool {
        false
    }
    fn command(&mut self, _command: &ParsedCommand) -> Result<(), OutputError> {
        Ok(())
    }
    fn prompt(&mut self, text: &WideText) -> Result<(), OutputError> {
        self.write(OutputStream::Stdout, text)
    }
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
    fn cancelled(&self) -> bool {
        false
    }
    /// False means cancellation; no WMI timeout is implied by this interval.
    fn wait(&mut self, seconds: u32) -> bool;
}
pub struct ThreadScheduler;
impl RepeatScheduler for ThreadScheduler {
    fn cancelled(&self) -> bool {
        interrupted()
    }
    fn wait(&mut self, seconds: u32) -> bool {
        #[cfg(unix)]
        let _watch = crate::platform::unix::console::KeyWatch::enter();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(seconds.into());
        loop {
            if interrupted() || console_key_pressed() {
                return false;
            }
            let now = std::time::Instant::now();
            if now >= end {
                return true;
            }
            std::thread::sleep((end - now).min(std::time::Duration::from_millis(100)));
        }
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
#[derive(Default)]
struct ExecutionFrame {
    every_count: u32,
    settings: Option<SessionSettings>,
}
pub struct Session {
    sequence: u32,
    trace_state: super::trace::TraceState,
    pub settings: SessionSettings,
    pub aliases: AliasStore,
    pub local_node: WideText,
}
impl Session {
    pub fn new(local_node: WideText) -> Result<Self, ResolveError> {
        Ok(Self {
            sequence: 0,
            trace_state: Default::default(),
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
        if sink.reset_command(&self.settings).is_err() {
            return CommandOutcome {
                exit_status: 0x80070005,
                ..Default::default()
            };
        }
        if parse_command(raw, mode)
            .is_ok_and(|c| c.verb == Verb::Exit && !c.help && c.modifiers.is_empty())
        {
            return CommandOutcome {
                exit_requested: true,
                ..Default::default()
            };
        }
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
            if sink.command(&command).is_err() {
                return CommandOutcome {
                    exit_status: 0x80070005,
                    ..Default::default()
                };
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
        } else {
            let mut command = ParsedCommand::empty(raw.clone());
            command.command_line = crate::parse::diagnostic_command_line(raw, mode);
            let context = super::presentation::context(
                &command,
                &self.settings,
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
        let command = parse_command(raw, mode).ok().filter(|c| !c.help);
        let switch_error = |sink: &mut dyn OutputSink, code, message: &str| {
            emit(
                sink,
                RenderedOutput {
                    stderr: format!("{message}\r\r\n").into(),
                    ..Default::default()
                },
                code,
            )
        };
        let number = |modifier: &Modifier| match &modifier.value {
            _ if modifier
                .raw_value
                .as_ref()
                .is_some_and(|raw| raw.0.first() == Some(&45)) =>
            {
                None
            }
            SwitchValue::Value(value) => super::repeat::number(value),
            SwitchValue::Reset
                if modifier
                    .raw_value
                    .as_ref()
                    .is_some_and(|raw| !raw.0.is_empty()) =>
            {
                super::repeat::number(&WideText::default())
            }
            _ => None,
        };
        let mut interval = None;
        let mut limit = None;
        if let Some(command) = &command {
            for (index, modifier) in command.modifiers.iter().enumerate() {
                if modifier.name.eq_ignore_case("every") {
                    if modifier.value == SwitchValue::Missing {
                        return switch_error(sink, 0xADD9, "Invalid EVERY switch.");
                    }
                    interval = match number(modifier) {
                        Some(number) => Some(number),
                        None => {
                            return switch_error(
                                sink,
                                0xACAE,
                                "Invalid interval value specified with /EVERY switch.",
                            );
                        }
                    };
                    limit = None;
                } else if modifier.name.eq_ignore_case("repeat") {
                    if index == 0 || !command.modifiers[index - 1].name.eq_ignore_case("every") {
                        return switch_error(
                            sink,
                            0xB0A6,
                            "HINT: /REPEAT should always follow /EVERY switch.",
                        );
                    }
                    // EVERY's nested REPEAT parser recognizes only a slash.
                    if modifier.prefix == Some(45) {
                        return switch_error(sink, 0xABE5, "Invalid command.");
                    }
                    if modifier.value == SwitchValue::Missing {
                        return switch_error(sink, 0xABE6, "Invalid REPEAT switch.");
                    }
                    limit = match number(modifier) {
                        Some(number) if number > 0 => Some(number),
                        _ => return switch_error(sink, 0xABE8, "Invalid REPEAT count."),
                    };
                }
            }
        }
        self.sequence = self.sequence.saturating_add(1);
        let mut frame = ExecutionFrame::default();
        loop {
            if scheduler.cancelled() {
                return CommandOutcome {
                    exit_status: 0xC000013A,
                    ..Default::default()
                };
            }
            let outcome = self.execute_once(raw, mode, backend, sink, scheduler, &mut frame);
            frame.every_count += 1;
            if scheduler.cancelled() {
                return CommandOutcome {
                    exit_status: 0xC000013A,
                    ..outcome
                };
            }
            if interval.is_none()
                && sink.console_input()
                && command
                    .as_ref()
                    .is_some_and(|c| matches!(c.verb, Verb::Get | Verb::List | Verb::Assoc))
                && !outcome.nodes.is_empty()
                && sink.consume_query_key().is_err()
            {
                return CommandOutcome {
                    exit_status: 0x80070005,
                    ..outcome
                };
            }
            if outcome.exit_requested
                || outcome.interactive_requested
                || interval.is_none()
                || limit.is_some_and(|n| frame.every_count >= n)
            {
                return outcome;
            }
            let wait_prompt =
                limit.is_none() && interval.is_some_and(|n| n > 0) && sink.console_input();
            if wait_prompt
                && sink
                    .prompt(&"Hit any key to break the cycle....".into())
                    .is_err()
            {
                return CommandOutcome {
                    exit_status: 0x80070005,
                    ..outcome
                };
            }
            if !scheduler.wait(interval.unwrap_or(0)) {
                if wait_prompt && !scheduler.cancelled() && sink.clear_wait_prompt().is_err() {
                    return CommandOutcome {
                        exit_status: 0x80070005,
                        ..outcome
                    };
                }
                return if scheduler.cancelled() {
                    CommandOutcome {
                        exit_status: 0xC000013A,
                        ..outcome
                    }
                } else {
                    outcome
                };
            }
            if wait_prompt && sink.clear_wait_prompt().is_err() {
                return CommandOutcome {
                    exit_status: 0x80070005,
                    ..outcome
                };
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
            let result = backend.execute_with_confirmation(
                &selection,
                &mut SinkConfirmation {
                    sink,
                    preparation: PathPreparation::Legacy,
                    trace: None,
                    method_trace_started: false,
                },
            );
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
                                trace_state: Default::default(),
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
    /// Resolve one command, checking argument errors before help or provider execution.
    fn execute_once(
        &mut self,
        raw: &WideText,
        mode: EntryMode,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
        scheduler: &mut dyn RepeatScheduler,
        frame: &mut ExecutionFrame,
    ) -> CommandOutcome {
        let parsed = match parse_command(raw, mode) {
            Ok(p) => p,
            Err(e) => return emit(sink, render::render_parse_error(&e), e.code),
        };
        let every_count = frame.every_count;
        let mut settings = match frame
            .settings
            .as_ref()
            .map_or_else(|| self.settings.for_command(&parsed), |s| Ok(s.clone()))
        {
            Ok(s) => s,
            Err(e) => return emit(sink, render::render_parse_error(&e), e.code),
        };
        let mut seen = Vec::<WideText>::new();
        settings.nodes.retain(|node| {
            let identity = if node.eq_cli(".") || node.eq_cli("localhost") {
                &self.local_node
            } else {
                node
            };
            if seen.iter().any(|n| n.eq_cli_text(identity)) {
                false
            } else {
                seen.push(identity.clone());
                true
            }
        });
        if !parsed.help && settings.user.is_some() && settings.password.is_none() {
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
        frame.settings = Some(settings.clone());
        if mode == EntryMode::Interactive
            && !settings.role.eq_ordinal_ignore_case(&self.settings.role)
        {
            match backend.load_alias_inventory(&settings.role, &settings.locale) {
                Err(_) | Ok(None) => {
                    return emit(
                        sink,
                        RenderedOutput {
                            stderr: "Invalid Role.\r\r\n".into(),
                            ..Default::default()
                        },
                        0xAC61,
                    );
                }
                Ok(Some(_)) => {}
            }
        }
        if mode == EntryMode::Interactive {
            let routes = (
                self.settings.output.clone(),
                self.settings.append.clone(),
                self.settings.record.clone(),
            );
            self.settings = settings.clone();
            if parsed.verb != Verb::Interactive {
                (
                    self.settings.output,
                    self.settings.append,
                    self.settings.record,
                ) = routes;
            }
        }
        if parsed.verb == Verb::Context && !parsed.help {
            return emit(
                sink,
                RenderedOutput {
                    stdout: super::presentation::context_text(&settings, &self.local_node),
                    ..Default::default()
                },
                0,
            );
        }
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
                    .all(|m| m.name.eq_ignore_case("?") || m.value != SwitchValue::Missing))
        {
            let mut loader = BackendRoleLoader {
                backend,
                selected,
                locale: settings.locale.clone(),
            };
            if let Err(error) = self.aliases.load_role(&settings, &mut loader) {
                return emit(
                    sink,
                    render::render_resolve_error_for(&error, secret_of(&settings)),
                    error.code,
                );
            }
        }
        let trace = self.trace_state.alias(&parsed, &settings, &self.aliases);
        if write_trace(sink, trace, &settings).is_err() {
            return CommandOutcome {
                exit_status: 0x80070005,
                ..Default::default()
            };
        }
        if let Err(error) = self.aliases.validate_method_arguments(&parsed) {
            return emit(
                sink,
                render::render_resolve_error_for(&error, secret_of(&settings)),
                error.code,
            );
        }
        if parsed.help {
            let schema = if (matches!(parsed.target, Some(Target::Path(_)))
                && parsed.verb == Verb::Get
                || matches!(parsed.target, Some(Target::Path(_) | Target::Class(_)))
                    && matches!(parsed.verb, Verb::Set | Verb::Create | Verb::Call))
                && parsed.verb_explicit
            {
                let mut req = match self.aliases.resolve(&parsed, &settings) {
                    Ok(ResolvedCommand::Management(r)) => r,
                    Err(error) => {
                        return emit(
                            sink,
                            render::render_resolve_error_for(&error, secret_of(&settings)),
                            error.code,
                        );
                    }
                    _ => {
                        return CommandOutcome {
                            exit_status: 0x8004100c,
                            ..Default::default()
                        };
                    }
                };
                configure_connection(&mut req, &settings);
                let preparation = match prepare_path(&parsed, &req, &settings, backend, sink) {
                    Ok(value) => value,
                    Err(outcome) => return outcome,
                };
                if settings.trace
                    && matches!(preparation, PathPreparation::Ready { .. })
                    && write_trace(
                        sink,
                        preparation
                            .parser(&req, Ok(&ManagementResult::Objects(ObjectSet::default()))),
                        &settings,
                    )
                    .is_err()
                {
                    return CommandOutcome {
                        exit_status: 0x80070005,
                        ..Default::default()
                    };
                }
                req.class_mode = true;
                req.verb = Verb::Get;
                req.method = None;
                req.arguments.clear();
                req.properties.clear();
                configure_connection(&mut req, &settings);
                match backend.execute(&req) {
                    Ok(ManagementResult::Schema(s)) => Some(s),
                    Err(mut error) => {
                        // Schema lookup for CALL help retains method-call diagnostics.
                        if parsed.verb == Verb::Call && error.operation.eq_ignore_case("query") {
                            error.operation = "class".into();
                        }
                        let status = error.code;
                        let node = if req.connection.node.eq_ignore_case(".") {
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
            if parsed.verb == Verb::Call
                && let (Some(schema), Some(method)) = (&schema, &parsed.method)
                && let Some(method) = schema.methods.iter().find(|m| method.eq_cli_text(&m.name))
                && let Err(error) = crate::backend::methods::validate_named_arguments(
                    &parsed.raw_arguments,
                    method
                        .input
                        .iter()
                        .map(|p| (p.name.clone(), true))
                        .chain(method.output.iter().map(|p| (p.name.clone(), false))),
                )
            {
                let error = ResolveError {
                    code: error.code,
                    message: error.message,
                    token: error.token,
                };
                return emit(
                    sink,
                    render::render_resolve_error_for(&error, secret_of(&settings)),
                    error.code,
                );
            }
            if let Some(Target::Alias(name)) = &parsed.target
                && self.aliases.find(name).is_none()
                && let Err(error) = self.aliases.resolve(&parsed, &settings)
            {
                return emit(
                    sink,
                    render::render_resolve_error_for(&error, secret_of(&settings)),
                    error.code,
                );
            }
            return match render::help::render_help(render::help::HelpRequest {
                command: &parsed,
                aliases: &self.aliases,
                schema: schema.as_ref(),
            }) {
                Ok(output) => emit(sink, output, 0),
                Err(error) => {
                    let code = emit_render_error(sink, error);
                    CommandOutcome {
                        exit_status: code,
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
            Err(e) => {
                return emit(
                    sink,
                    render::render_resolve_error_for(&e, secret_of(&settings)),
                    e.code,
                );
            }
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
        configure_connection(&mut request, &settings);
        request.multiple_nodes = settings.nodes.len() > 1;
        let preparation = match prepare_path(&parsed, &request, &settings, backend, sink) {
            Ok(value) => value,
            Err(outcome) => return outcome,
        };
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
        let mut stdout_written = false;
        let mut aggregate_trace_allowed = false;
        let mut blank_output = WideText::default();
        let mut failed_nodes: Vec<(WideText, ManagementError)> = vec![];
        let mut node_order: Vec<WideText> = vec![];
        let nodes = if settings.explicit_nodes {
            settings.nodes.clone()
        } else {
            vec![request.connection.node.clone()]
        };
        let listed = nodes.len() > 1;
        request.multiple_nodes = listed;
        let acts = matches!(
            request.verb,
            Verb::Call | Verb::Set | Verb::Create | Verb::Delete
        );
        let shows_errors = request
            .format_spec
            .as_ref()
            .is_some_and(render::shows_errors);
        let one_document = shows_errors
            && settings.aggregate
            && matches!(request.verb, Verb::Get | Verb::List)
            && !request.class_mode;
        for target in &nodes {
            if scheduler.cancelled() {
                status = 0xC000013A;
                break;
            }
            request.connection.node = target.clone();
            let node = if target.eq_ignore_case(".") || target.eq_ignore_case("localhost") {
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
            node_order.push(node.clone());
            if listed && acts {
                let mut line: WideText = "Node - ".into();
                line.0.extend(&node.0);
                line.0.extend("\r\r\n".encode_utf16());
                if sink.write(OutputStream::Stdout, &line).is_err() {
                    status = 0x80070005;
                    break;
                }
            }
            let mut method_trace_started = false;
            let mut result = if settings.failfast
                && !backend.reachable(&node, settings.failfast_timeout_ms)
            {
                Err(ManagementError {
                    code: 0xABE3,
                    target: node.clone(),
                    message: "RPC Server not available.".into(),
                    operation: "failfast".into(),
                    extended: vec![],
                    partial: None,
                })
            } else {
                match self.aliases.virtual_query(&request, &node) {
                    Some(result) => result,
                    None => {
                        let mut confirmation = SinkConfirmation {
                            sink,
                            preparation,
                            trace: if settings.trace {
                                Some((&mut self.trace_state, &settings, &node))
                            } else {
                                None
                            },
                            method_trace_started: false,
                        };
                        let result = backend.execute_with_confirmation(&request, &mut confirmation);
                        method_trace_started = confirmation.method_trace_started;
                        result
                    }
                }
            };
            match &mut result {
                Ok(value) => mark_node(value, &node),
                Err(error) => {
                    if let Some(value) = &mut error.partial {
                        mark_node(value, &node)
                    }
                }
            }
            let failfast_rejected = result
                .as_ref()
                .err()
                .is_some_and(|e| e.operation.eq_ignore_case("failfast"));
            aggregate_trace_allowed |=
                request.from_alias || preparation.ready() || !failfast_rejected;
            let execution_trace = self.trace_state.execution(
                &request,
                result.as_ref(),
                &node,
                settings.trace,
                preparation.ready(),
            );
            if settings.trace {
                let mut trace = if method_trace_started {
                    WideText::default()
                } else {
                    preparation.parser(&request, result.as_ref())
                };
                trace.0.extend(execution_trace.0);
                if write_trace(sink, trace, &settings).is_err() {
                    status = 0x80070005;
                    break;
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
                && matches!(
                    partial.as_ref(),
                    ManagementResult::Objects(_) | ManagementResult::Associations(_)
                )
            {
                if settings.aggregate
                    && let ManagementResult::Objects(set) = partial.as_ref()
                {
                    combined.extend(set.objects.clone());
                } else {
                    match render::render(render::RenderRequest {
                        request: &request,
                        result: Ok(partial.as_ref()),
                        node: &node,
                    }) {
                        Ok(output) => {
                            stdout_written |= !output.stdout.0.is_empty();
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
                Ok(mut output) => {
                    let parser_failure = !request.from_alias
                        && !preparation.ready()
                        && !request.multiple_nodes
                        && result.as_ref().err().is_some_and(|e| {
                            matches!(e.code, 0x80041002 | 0x8004100e | 0x80041010)
                        });
                    if settings.trace
                        && !failfast_rejected
                        && let Err(error) = &result
                    {
                        output.stderr = super::trace::error_diagnostic(
                            error,
                            if parser_failure { None } else { Some(&node) },
                        );
                        if parser_failure {
                            output.stdout = WideText::default();
                        }
                        if let Some(secret) = secret_of(&settings).filter(|s| !s.0.is_empty()) {
                            output.stderr = redact_trace(output.stderr, secret);
                        }
                    }
                    if let Err(error) = &result
                        && !acts
                        && error.partial.is_none()
                        && render::is_node_failure(error)
                    {
                        if one_document {
                            failed_nodes.push((node.clone(), error.clone()));
                            output.stdout = WideText::default();
                        } else if settings.aggregate && !shows_errors {
                            let blank = std::mem::take(&mut output.stdout);
                            if blank_output.0.is_empty() {
                                blank_output = blank;
                            }
                        }
                    }
                    stdout_written |= !output.stdout.0.is_empty();
                    let out = emit(sink, output, code);
                    if out.exit_status != code {
                        status = out.exit_status;
                    }
                }
                Err(error) => {
                    status = emit_render_error(sink, error);
                }
            }
            if settings.trace
                && (request.from_alias || preparation.ready() || !failfast_rejected)
                && !(settings.aggregate && listed && preparation.ready() && !request.from_alias)
                && result.as_ref().err().is_none_or(|e| {
                    request.from_alias
                        || preparation.ready()
                        || request.multiple_nodes
                        || !matches!(e.code, 0x80041002 | 0x8004100e | 0x80041010)
                })
            {
                let has_rows =
                    matches!(&result,Ok(ManagementResult::Objects(set)) if !set.objects.is_empty());
                if write_trace(
                    sink,
                    super::trace::formatting(&request, has_rows),
                    &settings,
                )
                .is_err()
                {
                    status = 0x80070005;
                    break;
                }
            }
        }
        let aggregate_result =
            settings.aggregate && (had_objects || !combined.is_empty() || !failed_nodes.is_empty());
        if aggregate_result {
            let has_rows = !combined.is_empty();
            let result = ManagementResult::Objects(ObjectSet {
                objects: combined,
                schema: None,
            });
            match render::render_with_failures(
                render::RenderRequest {
                    request: &request,
                    result: Ok(&result),
                    node: &self.local_node,
                },
                &node_order,
                &failed_nodes,
            ) {
                Ok(output) => {
                    stdout_written |= !output.stdout.0.is_empty();
                    status = emit(sink, output, status).exit_status
                }
                Err(error) => status = emit_render_error(sink, error),
            }
            if settings.trace
                && aggregate_trace_allowed
                && write_trace(
                    sink,
                    super::trace::formatting(&request, has_rows),
                    &settings,
                )
                .is_err()
            {
                status = 0x80070005;
            }
        }
        if settings.trace
            && settings.aggregate
            && listed
            && preparation.ready()
            && !request.from_alias
            && aggregate_trace_allowed
            && !aggregate_result
            && write_trace(sink, super::trace::formatting(&request, false), &settings).is_err()
        {
            status = 0x80070005;
        }
        if !stdout_written
            && !blank_output.0.is_empty()
            && sink.write(OutputStream::Stdout, &blank_output).is_err()
        {
            status = 0x80070005;
        }
        CommandOutcome {
            exit_status: status,
            nodes: outcomes,
            ..Default::default()
        }
    }
}
fn secret_of(settings: &SessionSettings) -> Option<&WideText> {
    settings.password.as_ref().map(|p| p.expose())
}
fn write_trace(
    sink: &mut dyn OutputSink,
    mut text: WideText,
    settings: &SessionSettings,
) -> Result<(), OutputError> {
    if text.0.is_empty() {
        return Ok(());
    }
    if let Some(secret) = secret_of(settings).filter(|s| !s.0.is_empty()) {
        text = redact_trace(text, secret);
    }
    sink.write(OutputStream::Stderr, &text)
}
fn redact_trace(mut text: WideText, secret: &WideText) -> WideText {
    let replacement: Vec<_> = "<redacted>".encode_utf16().collect();
    // Diagnostic interpolation replaces unpaired UTF-16 units. Mask that
    // representation as well as the original credential before encoding.
    let rendered: WideText = secret.to_string_lossy().into();
    let mut needles = vec![secret];
    if rendered != *secret {
        needles.push(&rendered);
    }
    for secret in needles {
        let mut start = 0;
        while let Some(offset) = text.0[start..]
            .windows(secret.0.len())
            .position(|w| w == secret.0)
        {
            let at = start + offset;
            text.0
                .splice(at..at + secret.0.len(), replacement.iter().copied());
            start = at + replacement.len();
        }
    }
    text
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
        if self.selected.is_none() {
            self.backend.load_alias_inventory(role, &self.locale)
        } else {
            self.backend
                .load_aliases(role, self.selected.as_ref(), &self.locale)
        }
    }
}

fn configure_connection(request: &mut crate::backend::ResolvedRequest, settings: &SessionSettings) {
    if settings.interactive {
        request.modifiers.insert(
            0,
            Modifier {
                prefix: None,
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

#[derive(Clone, Copy)]
enum PathPreparation {
    Legacy,
    Skipped,
    Ready { method_signatures: u8 },
}
impl PathPreparation {
    fn ready(self) -> bool {
        !matches!(self, Self::Legacy)
    }
    fn parser(
        self,
        request: &crate::backend::ResolvedRequest,
        result: Result<&ManagementResult, &ManagementError>,
    ) -> WideText {
        match self {
            Self::Legacy => super::trace::parser(request, result),
            Self::Skipped => WideText::default(),
            Self::Ready { method_signatures } => {
                super::trace::path_preflight(request, None, method_signatures)
            }
        }
    }
}

fn prepare_path(
    parsed: &ParsedCommand,
    request: &crate::backend::ResolvedRequest,
    settings: &SessionSettings,
    backend: &mut dyn Management,
    sink: &mut dyn OutputSink,
) -> Result<PathPreparation, CommandOutcome> {
    use crate::backend::{PathPreflightOutcome, PathPreflightRequest};
    if !matches!(parsed.target, Some(Target::Path(_))) {
        return Ok(PathPreparation::Legacy);
    }
    let metadata = match parsed.verb {
        Verb::Get => {
            parsed.verb_explicit
                && (!parsed.properties.is_empty()
                    || parsed.help
                    || parsed
                        .modifiers
                        .iter()
                        .any(|m| m.scope == ModifierScope::Verb))
        }
        Verb::Call => true,
        Verb::Set | Verb::Create => parsed.help,
        _ => false,
    };
    // Native multi-node requests and bare/final-token GET bypass parser metadata.
    if settings.nodes.len() > 1 || !metadata {
        return Ok(PathPreparation::Skipped);
    }
    if !settings.failfast {
        return Ok(PathPreparation::Legacy);
    }
    match backend.preflight_path(PathPreflightRequest {
        connection: &request.connection,
        class_name: &request.class_name,
        method: request.method.as_ref(),
    }) {
        Ok(PathPreflightOutcome::Unsupported) => Ok(PathPreparation::Legacy),
        Ok(PathPreflightOutcome::Ready { method_signatures }) => {
            Ok(PathPreparation::Ready { method_signatures })
        }
        Err(failure) => {
            let mut stderr = if settings.trace && failure.error.code != 0xC000013A {
                super::trace::path_preflight(
                    request,
                    Some(failure.stage),
                    failure.completed_method_signatures,
                )
            } else {
                WideText::default()
            };
            let error = failure.error;
            let diagnostic = if error.code == 0xC000013A {
                WideText::default()
            } else if error.code < 0x10000 {
                let mut text = error.message.clone();
                text.0.extend("\r\r\n".encode_utf16());
                text
            } else if settings.trace {
                super::trace::error_diagnostic(&error, None)
            } else {
                let mut text: WideText = "ERROR:\r\r\nDescription = ".into();
                text.0.extend(&error.message.0);
                text.0.extend("\r\r\n".encode_utf16());
                text
            };
            stderr.0.extend(diagnostic.0);
            let status = if write_trace(sink, stderr, settings).is_err() {
                0x80070005
            } else {
                error.code
            };
            Err(CommandOutcome {
                exit_status: status,
                ..Default::default()
            })
        }
    }
}

struct SinkConfirmation<'a> {
    sink: &'a mut dyn OutputSink,
    preparation: PathPreparation,
    trace: Option<(
        &'a mut super::trace::TraceState,
        &'a SessionSettings,
        &'a WideText,
    )>,
    method_trace_started: bool,
}
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
        if request.arguments.is_empty()
            && let Some((state, settings, node)) = &mut self.trace
        {
            let trace = if self.method_trace_started {
                super::trace::method_record(request, result)
            } else {
                let mut trace = self.preparation.parser(request, Ok(&value));
                trace.0.extend(
                    state
                        .execution(request, Ok(&value), node, true, self.preparation.ready())
                        .0,
                );
                trace
            };
            self.method_trace_started = true;
            write_trace(self.sink, trace, settings).map_err(|_| ManagementError {
                code: 0x80070005,
                target: request.class_name.clone(),
                message: "Output failed".into(),
                operation: "input".into(),
                extended: vec![],
                partial: None,
            })?;
        }
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
        let outcome = emit(self.sink, output, 0);
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
                self.sink.write(OutputStream::Stderr, prompt)?;
                let answer = self.sink.read_line()?;
                if answer
                    .as_ref()
                    .is_some_and(|a| a.trim_ascii().eq_ignore_case("?"))
                {
                    self.sink.write(
                        OutputStream::Stderr,
                        &"Invalid XSL format (or) file name.\r\r\n".into(),
                    )?;
                } else {
                    break Ok(answer);
                }
            }
        })();
        result
            .map(|answer| answer.is_some_and(|a| a.trim_ascii().eq_ignore_case("y")))
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

fn interrupted() -> bool {
    #[cfg(windows)]
    {
        crate::platform::windows::console::interrupted()
    }
    #[cfg(unix)]
    {
        crate::platform::unix::console::interrupted()
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

fn console_key_pressed() -> bool {
    #[cfg(windows)]
    {
        crate::platform::windows::console::key_pressed()
    }
    #[cfg(unix)]
    {
        crate::platform::unix::console::key_pressed()
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}
