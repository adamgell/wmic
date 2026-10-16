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
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError>;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandOutcome {
    pub exit_status: u32,
    pub exit_requested: bool,
    pub interactive_requested: bool,
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
        let parsed = match parse_command(raw, mode) {
            Ok(p) => p,
            Err(e) => return emit(sink, render::render_parse_error(&e), e.code),
        };
        let settings = match self.settings.for_command(&parsed) {
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
        request.connection.node = settings
            .nodes
            .first()
            .cloned()
            .unwrap_or_else(|| ".".into());
        let node = if request.connection.node.eq_ascii_ignore_case(".") {
            self.local_node.clone()
        } else {
            request.connection.node.clone()
        };
        let result = backend.execute(&request);
        let status = result.as_ref().err().map_or(0, |e| e.code);
        match render::render(render::RenderRequest {
            request: &request,
            result: result.as_ref(),
            node: &node,
        }) {
            Ok(output) => emit(sink, output, status),
            Err(error) => emit(
                sink,
                RenderedOutput {
                    stderr: {
                        let mut w = error.0;
                        w.0.extend("\r\r\n".encode_utf16());
                        w
                    },
                    ..Default::default()
                },
                0xACB2,
            ),
        }
    }
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
