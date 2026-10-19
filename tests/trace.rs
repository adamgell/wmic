mod support;
use support::wide;
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::OutputError,
    session::{OutputSink, OutputStream, Session},
};

struct OneRow;
impl Management for OneRow {
    fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        Ok(ManagementResult::Objects(ObjectSet {
            objects: vec![Record::default()],
            schema: None,
        }))
    }
}

struct ReferenceResults;
impl Management for ReferenceResults {
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        let failure = if r
            .connection
            .namespace
            .to_string_lossy()
            .contains("WmicTraceMissing")
        {
            Some((0x8004100e, "Invalid namespace", "class"))
        } else if r.class_name.eq_cli("WmicTraceMissingClass") {
            Some((0x80041002, "Not found", "class"))
        } else if r
            .properties
            .iter()
            .any(|p| p.eq_cli("WmicTraceMissingProperty"))
        {
            Some((0x80041017, "Invalid query", "enumerate"))
        } else {
            None
        };
        if let Some((code, message, operation)) = failure {
            return Err(ManagementError {
                code,
                message: wide(message),
                operation: wide(operation),
                target: r.class_name.clone(),
                extended: vec![],
                partial: None,
            });
        }
        if r.verb == Verb::Call {
            return Ok(ManagementResult::Methods(vec![MethodResult {
                path: wide(r#"\\DESKTOP-N4H9E7R\ROOT\CIMV2:Win32_Process.Handle="0""#),
                return_value: Some(CimValue::Uint32(2)),
                out_parameters: vec![],
                text: None,
            }]));
        }
        Ok(ManagementResult::Objects(ObjectSet {
            objects: if r
                .selector
                .as_ref()
                .is_some_and(|s| s.to_string_lossy().contains("4294967295"))
            {
                vec![]
            } else {
                vec![
                    Record::default();
                    if r.class_name.eq_cli("Win32_Processor") {
                        6
                    } else {
                        1
                    }
                ]
            },
            schema: None,
        }))
    }
}

#[derive(Default)]
struct Sink(WideText);
impl OutputSink for Sink {
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        if stream == OutputStream::Stderr {
            self.0.0.extend(&text.0);
        }
        Ok(())
    }
}

fn captured(text: &str) -> WideText {
    wide(&text.replace(
        "C:\\WINDOWS",
        &std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\WINDOWS".into()),
    ))
}
fn fixtures() -> std::path::PathBuf {
    std::env::var_os("WMIC_TRACE_FIXTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/trace-reference")
        })
}

#[test]
fn trace_os_query_matches_microsoft_diagnostic_bytes() {
    let mut sink = Sink::default();
    let outcome = Session::new(wide("DESKTOP-N4H9E7R")).unwrap().execute(
        &wide("/trace:on os get Caption"),
        EntryMode::Interactive,
        &mut OneRow,
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0);
    let expected = include_bytes!("fixtures/connection-reference/trace.stderr.bin");
    assert!(sink.0 == captured(std::str::from_utf8(expected).unwrap()));
}

#[test]
fn trace_query_errors_formats_aliases_and_help_match_native_fixtures() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/trace-cases.json")).unwrap();
    let root = fixtures();
    let mut differences = vec![];
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if id.starts_with("interactive-") || id == "call-multiple" {
            continue;
        }
        let mut sink = Sink::default();
        Session::new(wide("DESKTOP-N4H9E7R")).unwrap().execute(
            &wide(case["arguments"].as_str().unwrap()),
            EntryMode::Interactive,
            &mut ReferenceResults,
            &mut sink,
        );
        let expected = std::fs::read(root.join(format!("{id}.stderr.bin"))).unwrap();
        let actual = sink.0.to_string_lossy();
        let expected = captured(std::str::from_utf8(&expected).unwrap()).to_string_lossy();
        let at = actual
            .bytes()
            .zip(expected.bytes())
            .position(|(a, b)| a != b);
        if actual != expected {
            differences.push(format!(
                "{id}: diagnostic differs at {at:?}, actual {} bytes, expected {} bytes",
                actual.len(),
                expected.len()
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn interactive_trace_caches_connections_and_honors_off() {
    for (fixture, commands) in [
        (
            "interactive-toggle",
            vec![
                "/trace:on",
                "os get Caption",
                "os get Caption",
                "/trace:off",
                "os get Caption",
            ],
        ),
        (
            "interactive-warm",
            vec!["os get Caption", "/trace:on", "os get Caption"],
        ),
    ] {
        let mut session = Session::new(wide("DESKTOP-N4H9E7R")).unwrap();
        let mut sink = Sink::default();
        for command in commands {
            assert_eq!(
                session
                    .execute(
                        &wide(command),
                        EntryMode::Interactive,
                        &mut OneRow,
                        &mut sink
                    )
                    .exit_status,
                0
            );
        }
        let expected = std::fs::read(fixtures().join(format!("{fixture}.stderr.bin"))).unwrap();
        let expected = captured(std::str::from_utf8(&expected).unwrap());
        let at = sink.0.0.iter().zip(&expected.0).position(|(a, b)| a != b);
        assert!(
            sink.0 == expected,
            "{fixture}: mismatch at {at:?}, actual {} units expected {}",
            sink.0.0.len(),
            expected.0.len()
        );
    }
}

#[test]
fn streamed_method_completion_keeps_its_trace() {
    struct Streamed;
    impl Management for Streamed {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("confirmation path required")
        }
        fn execute_with_confirmation(
            &mut self,
            r: &ResolvedRequest,
            c: &mut dyn wmic_compat::backend::Confirmation,
        ) -> Result<ManagementResult, ManagementError> {
            assert!(c.streams_methods());
            let ManagementResult::Methods(results) = ReferenceResults.execute(r)? else {
                panic!("method expected")
            };
            c.method_completed(r, &results[0])?;
            if r.selector
                .as_ref()
                .is_some_and(|s| s.to_string_lossy().contains(" OR "))
            {
                let mut second = results[0].clone();
                second.path = wide(
                    &second
                        .path
                        .to_string_lossy()
                        .replace("Handle=\"0\"", "Handle=\"4\""),
                );
                c.method_completed(r, &second)?;
            }
            Ok(ManagementResult::Operation(OperationResult {
                action: Verb::Call,
                paths: vec![],
                instances: vec![Record::default()],
            }))
        }
    }
    for (selector, fixture) in [
        ("ProcessId=0", "call-readonly"),
        ("ProcessId=0 OR ProcessId=4", "call-multiple"),
    ] {
        let mut sink = Sink::default();
        let outcome = Session::new(wide("DESKTOP-N4H9E7R")).unwrap().execute(
            &wide(&format!(
                "/trace:on process where \"{selector}\" call GetOwner"
            )),
            EntryMode::Interactive,
            &mut Streamed,
            &mut sink,
        );
        assert_eq!(outcome.exit_status, 0);
        let expected = std::fs::read(fixtures().join(format!("{fixture}.stderr.bin"))).unwrap();
        assert!(
            sink.0 == captured(std::str::from_utf8(&expected).unwrap()),
            "{fixture}"
        );
    }
}

#[test]
fn trace_redacts_password_even_when_it_occurs_in_the_query() {
    let mut sink = Sink::default();
    Session::new(wide("LOCAL")).unwrap().execute(&wide("/trace:on /user:tester /password:trace-secret process where \"Name='trace-secret'\" get Name"),EntryMode::Interactive,&mut OneRow,&mut sink);
    let text = sink.0.to_string_lossy();
    assert!(!text.contains("trace-secret"));
    assert!(text.contains("Name='<redacted>'"));
}

#[test]
fn trace_output_failure_stops_before_provider_execution() {
    struct Never;
    impl Management for Never {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("provider must not run after diagnostic failure")
        }
    }
    struct Fails;
    impl OutputSink for Fails {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Err(OutputError("closed pipe".into()))
        }
    }
    let outcome = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("/trace:on os get Caption"),
        EntryMode::Interactive,
        &mut Never,
        &mut Fails,
    );
    assert_eq!(outcome.exit_status, 0x80070005);
}

#[cfg(windows)]
#[test]
fn windows_cli_matches_trace_diagnostics() {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/trace-cases.json")).unwrap();
    let capture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/trace-reference/capture.json")).unwrap();
    let binary =
        std::env::var_os("WMIC_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_wmic").into());
    let mut tested = 0;
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        // CPU row counts and method __PATH values belong to the current provider.
        if id == "cpu" || id.starts_with("call-") || id.starts_with("interactive-") {
            continue;
        }
        let expected = capture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let mut child = Command::new(&binary)
            .raw_arg(case["arguments"].as_str().unwrap())
            .creation_flags(0x08000000)
            .env("COMPUTERNAME", "DESKTOP-N4H9E7R")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut b = vec![];
            stdout.read_to_end(&mut b).unwrap();
            b
        });
        let err = std::thread::spawn(move || {
            let mut b = vec![];
            stderr.read_to_end(&mut b).unwrap();
            b
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut timeout = false;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                timeout = true;
                let _ = child.kill();
                break child.wait().unwrap();
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let stdout = out.join().unwrap();
        let stderr = err.join().unwrap();
        assert!(!timeout, "{id}: CLI timed out");
        assert_eq!(
            status.code().unwrap() as u32,
            expected["exit_status"].as_u64().unwrap() as u32,
            "{id}: full status"
        );
        let expected = std::fs::read(fixtures().join(format!("{id}.stderr.bin"))).unwrap();
        let expected = captured(std::str::from_utf8(&expected).unwrap())
            .to_string_lossy()
            .into_bytes();
        let at = stderr.iter().zip(&expected).position(|(a, b)| a != b);
        assert!(
            stderr == expected,
            "{id}: CLI stderr differs at {at:?}, {} vs {} bytes",
            stderr.len(),
            expected.len()
        );
        if status.success() {
            assert!(
                !stdout.is_empty(),
                "{id}: successful query/help must produce output"
            );
        }
        tested += 1;
    }
    assert_eq!(tested, 17);
}
