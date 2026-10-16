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
    pub settings: SessionSettings,
    pub aliases: AliasStore,
    pub local_node: WideText,
}
impl Session {
    pub fn new(local_node: WideText) -> Result<Self, ResolveError> {
        Ok(Self {
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
        loop {
            let outcome = self.execute_once(raw, mode, backend, sink);
            if outcome.exit_requested
                || outcome.interactive_requested
                || interval.is_none()
                || !scheduler.wait(interval.unwrap_or(0))
            {
                return outcome;
            }
        }
    }
    fn execute_once(
        &mut self,
        raw: &WideText,
        mode: EntryMode,
        backend: &mut dyn Management,
        sink: &mut dyn OutputSink,
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
            let result =
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
                    backend.execute(&request)
                };
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
    let mut stderr = error.0;
    stderr.0.extend("\r\r\n".encode_utf16());
    emit(
        sink,
        RenderedOutput {
            stderr,
            ..Default::default()
        },
        0xACB2,
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
