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

#[test]
fn session_supplies_a_sanitized_xml_presentation_context() {
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            let context = r
                .render_context
                .as_ref()
                .expect("presentation context missing");
            assert_eq!(
                context.command_line,
                wide(" /password:<redacted> os get Caption")
            );
            assert_eq!(context.issued_from, wide("HOST"));
            assert_eq!(context.nodes, vec![wide("HOST")]);
            assert_eq!(context.sequence, 1);
            assert_eq!(context.every_count, 0);
            assert!(
                !context
                    .settings
                    .iter()
                    .any(|(key, _)| key.eq_ignore_case("password"))
            );
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    let mut sink = Sink::default();
    let out = Session::new(wide("HOST")).unwrap().execute(
        &wide("wmic.exe /password:test-only-secret os get Caption"),
        EntryMode::Process,
        &mut Provider,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
}
