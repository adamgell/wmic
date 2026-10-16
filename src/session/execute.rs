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
        if parsed.help {
            return emit(
                sink,
                RenderedOutput {
                    stderr: "Not supported\r\r\n".into(),
                    ..Default::default()
                },
                0x8004100c,
            );
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
        let mut outcomes = vec![];
        let mut status = 0;
        let mut combined = vec![];
        for target in &settings.nodes {
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
                combined.extend(set.objects.clone());
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
        if settings.aggregate && (!combined.is_empty() || status == 0) {
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
