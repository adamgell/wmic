use super::*;
use crate::{backend::Management, model::WideText, render::OutputError};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Line { line: WideText, console: bool },
    Eof,
    Interrupted,
}
#[derive(Debug)]
pub struct InputError(pub String);
pub trait InputSource {
    fn next_input(&mut self) -> Result<InputEvent, InputError>;
}
pub fn run_interactive(
    session: &mut Session,
    backend: &mut dyn Management,
    input: &mut dyn InputSource,
    sink: &mut dyn OutputSink,
) -> CommandOutcome {
    session.settings.interactive = true;
    let mut outcome = CommandOutcome::default();
    loop {
        let mut prompt: WideText = "wmic:".into();
        prompt.0.extend(&session.settings.role.0);
        prompt.0.push(62);
        if sink.prompt(&prompt).is_err() {
            outcome.exit_status = 0x80070005;
            break;
        }
        match input.next_input() {
            Ok(InputEvent::Eof) => break,
            Ok(InputEvent::Interrupted) => {
                outcome.exit_status = 0xC000013A;
                break;
            }
            Err(_) => {
                outcome.exit_status = 0x80070005;
                break;
            }
            Ok(InputEvent::Line { line, .. }) => {
                if line.trim_ascii().0.is_empty() {
                    if sink
                        .write(
                            OutputStream::Stderr,
                            &"\"/?\" for help, QUIT to Exit.\r\r\n".into(),
                        )
                        .is_err()
                    {
                        outcome.exit_status = 0x80070005;
                        break;
                    }
                    continue;
                }
                outcome =
                    session.execute(&line, crate::model::EntryMode::Interactive, backend, sink);
                if outcome.exit_requested || outcome.exit_status == 0xC000013A {
                    break;
                }
            }
        }
    }
    outcome
}
impl From<OutputError> for InputError {
    fn from(e: OutputError) -> Self {
        Self(e.0)
    }
}
