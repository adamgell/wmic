use crate::{
    backend::dispatch::Dispatcher,
    model::{EntryMode, WideText},
    platform::unix::{self, cmdline, console, output},
    session::Session,
};
pub fn run() -> ! {
    let raw = cmdline::command_line(&cmdline::argument_units(std::env::args_os()));
    let node: WideText = unix::local_node();
    let status = {
        match (Session::new(node), Dispatcher::new()) {
            (Ok(mut session), Ok(mut backend)) => {
                let mut sink = crate::session::RoutedOutput::new(output::NativeOutput);
                let _interrupt = console::InterruptGuard::install();
                let shell = crate::parse::parse_command(&raw, EntryMode::Process).is_ok_and(|p| {
                    p.verb == crate::model::Verb::Interactive && p.modifiers.is_empty()
                });
                if shell {
                    crate::session::run_interactive(
                        &mut session,
                        &mut backend,
                        &mut output::NativeInput,
                        &mut sink,
                    )
                    .exit_status
                } else {
                    session
                        .execute(&raw, EntryMode::Process, &mut backend, &mut sink)
                        .exit_status
                }
            }
            (Err(e), _) => e.code,
            (_, Err(e)) => e.code,
        }
    };
    unix::terminate(status)
}
