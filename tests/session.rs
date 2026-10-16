mod support;
use support::wide;
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream, Session},
};
#[derive(Default)]
struct Backend {
    requests: Vec<ResolvedRequest>,
}
impl Management for Backend {
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        self.requests.push(r.clone());
        Ok(ManagementResult::Objects(ObjectSet::default()))
    }
}
#[derive(Default)]
struct Sink {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
impl OutputSink for Sink {
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        let dest = if stream == OutputStream::Stdout {
            &mut self.stdout
        } else {
            &mut self.stderr
        };
        dest.extend(encode_output(text, &DestinationProfile::pipe(1252))?);
        Ok(())
    }
}
#[test]
fn unknown_switch_is_not_ignored() {
    let (mut backend, mut sink) = (Backend::default(), Sink::default());
    let out = Session::new(wide("TEST")).unwrap().execute(
        &wide("/nonsense os get Caption"),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0xAC5D);
    assert!(backend.requests.is_empty());
    assert_eq!(sink.stderr, b"Invalid Global Switch.\r\r\n");
}
#[test]
fn empty_result_keeps_reference_status() {
    let (mut backend, mut sink) = (Backend::default(), Sink::default());
    let out = Session::new(wide("TEST")).unwrap().execute(
        &wide("os where Caption='missing' get Caption"),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(sink.stdout, b"\r\r\n\r\r\n");
}
#[test]
fn quoted_wql_reaches_backend_unchanged() {
    let (mut backend, mut sink) = (Backend::default(), Sink::default());
    Session::new(wide("TEST")).unwrap().execute(
        &wide(r#"os where "Caption='Microsoft Windows 11 Pro'" get Caption"#),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    assert_eq!(
        backend.requests[0].selector,
        Some(wide("Caption='Microsoft Windows 11 Pro'"))
    );
}
#[test]
fn stderr_is_not_merged_into_stdout() {
    let (mut backend, mut sink) = (Backend::default(), Sink::default());
    Session::new(wide("TEST")).unwrap().execute(
        &wide("os get Caption"),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    assert_eq!(sink.stderr, b"No Instance(s) Available.\r\r\n");
    assert_eq!(sink.stdout, b"\r\r\n\r\r\n");
}
