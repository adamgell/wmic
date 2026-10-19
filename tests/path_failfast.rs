mod support;
use support::wide;
use wmic_compat::{
    backend::{
        Management, PathPreflightError, PathPreflightOutcome, PathPreflightRequest,
        PathPreflightStage, ResolvedRequest,
    },
    model::*,
    render::OutputError,
    session::{OutputSink, OutputStream, Session},
};

struct Unavailable;
impl Management for Unavailable {
    fn preflight_path(
        &mut self,
        request: PathPreflightRequest<'_>,
    ) -> Result<PathPreflightOutcome, PathPreflightError> {
        Err(PathPreflightError {
            stage: PathPreflightStage::Connect,
            completed_method_signatures: 0,
            error: ManagementError {
                code: 0x800706ba,
                target: request.connection.node.clone(),
                message: wide("The RPC server is unavailable."),
                operation: wide("connect"),
                extended: vec![],
                partial: None,
            },
        })
    }
    fn reachable(&mut self, _: &WideText, _: u32) -> bool {
        false
    }
    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        Err(ManagementError {
            code: 0x800706ba,
            target: request.connection.node.clone(),
            message: wide("The RPC server is unavailable."),
            operation: wide("query"),
            extended: vec![],
            partial: None,
        })
    }
}
#[derive(Default)]
struct Sink {
    stdout: WideText,
    stderr: WideText,
}
impl OutputSink for Sink {
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        let destination = if stream == OutputStream::Stderr {
            &mut self.stderr
        } else {
            &mut self.stdout
        };
        destination.0.extend(&text.0);
        Ok(())
    }
}

#[test]
fn explicit_path_connection_error_precedes_failfast_rejection() {
    for trace in [false, true] {
        let mut sink = Sink::default();
        let result = Session::new(wide("DESKTOP-N4H9E7R")).unwrap().execute(
            &wide(&format!(
                "/trace:{} /failfast:on /node:0.0.0.0 path Win32_OperatingSystem get Caption",
                if trace { "on" } else { "off" }
            )),
            EntryMode::Interactive,
            &mut Unavailable,
            &mut sink,
        );
        assert_eq!(
            result.exit_status, 0x800706ba,
            "parser connection error must win over ping rejection"
        );
        assert!(sink.stdout.0.is_empty());
        let diagnostic = if trace {
            concat!(
                "FAIL: IWbemLocator::ConnectServer(L\"root\\cimv2\", NULL, NULL, L\"ms_409\", 0L, L\"<null>\", NULL, -)\r\n",
                "Line:   4244 File: onecore\\admin\\wmi\\wbem\\tools\\wmic\\parserengine.cpp\r\n\r\n",
                "ERROR:\r\r\nCode = 0x800706ba\r\r\nDescription = The RPC server is unavailable.\r\r\nFacility = Win32\r\r\n"
            )
        } else {
            "ERROR:\r\r\nDescription = The RPC server is unavailable.\r\r\n"
        };
        assert_eq!(sink.stderr, wide(diagnostic));
    }
}

#[derive(Default)]
struct MetadataProvider {
    calls: Vec<String>,
    reject_ping: bool,
    fail_stage: Option<PathPreflightStage>,
}
fn error(code: u32, message: &str, operation: &str, target: &WideText) -> ManagementError {
    ManagementError {
        code,
        target: target.clone(),
        message: wide(message),
        operation: wide(operation),
        extended: vec![],
        partial: None,
    }
}
impl Management for MetadataProvider {
    fn preflight_path(
        &mut self,
        r: PathPreflightRequest<'_>,
    ) -> Result<PathPreflightOutcome, PathPreflightError> {
        self.calls
            .push(format!("metadata:{}:{}", r.connection.node, r.class_name));
        let failure = if let Some(stage) = self.fail_stage {
            Some((stage, 0x80070005, "Access is denied."))
        } else if r.connection.node.eq_cli("0.0.0.0") {
            Some((
                PathPreflightStage::Connect,
                0x800706ba,
                "The RPC server is unavailable.",
            ))
        } else if r
            .connection
            .namespace
            .to_string_lossy()
            .contains("WmicTraceMissing")
        {
            Some((PathPreflightStage::Connect, 0x8004100e, "Invalid namespace"))
        } else if r.class_name.eq_cli("WmicTraceMissingClass") {
            Some((PathPreflightStage::Class, 0x80041002, "Not found"))
        } else if r
            .method
            .is_some_and(|m| m.eq_cli("WmicCompatMissingMethod"))
        {
            Some((
                PathPreflightStage::Method,
                0xABFC,
                "WmicCompatMissingMethod - Invalid class method.",
            ))
        } else {
            None
        };
        if let Some((stage, code, message)) = failure {
            return Err(PathPreflightError {
                stage,
                completed_method_signatures: 0,
                error: error(code, message, "class", &r.connection.node),
            });
        }
        Ok(PathPreflightOutcome::Ready {
            method_signatures: u8::from(r.method.is_some()),
        })
    }
    fn reachable(&mut self, node: &WideText, _: u32) -> bool {
        self.calls.push(format!("ping:{node}"));
        !self.reject_ping && !node.eq_cli("0.0.0.0")
    }
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        self.calls.push(format!("execute:{}", r.connection.node));
        assert!(
            !self.reject_ping && !r.connection.node.eq_cli("0.0.0.0"),
            "rejected requests must never execute"
        );
        if r.class_name.eq_cli("WmicTraceMissingClass") {
            return Err(error(
                0x80041010,
                "Invalid class",
                "enumerate",
                &r.connection.node,
            ));
        }
        if r.properties
            .iter()
            .any(|p| p.eq_cli("WmicTraceMissingProperty"))
        {
            return Err(error(
                0x80041017,
                "Invalid query",
                "enumerate",
                &r.connection.node,
            ));
        }
        Ok(ManagementResult::Objects(ObjectSet {
            objects: if r
                .instance_path
                .as_ref()
                .is_some_and(|p| p.to_string_lossy().contains("4294967295"))
            {
                vec![]
            } else {
                vec![Record::default()]
            },
            schema: None,
        }))
    }
}
fn run(command: &str, backend: &mut dyn Management) -> (u32, Sink) {
    let mut sink = Sink::default();
    let outcome = Session::new(wide("DESKTOP-N4H9E7R")).unwrap().execute(
        &wide(command),
        EntryMode::Interactive,
        backend,
        &mut sink,
    );
    (outcome.exit_status, sink)
}
fn fixture(id: &str, stream: &str) -> WideText {
    let root = std::env::var_os("WMIC_PATH_FAILFAST_FIXTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/path-failfast-reference")
        });
    let data = std::fs::read(root.join(format!("{id}.{stream}.bin"))).unwrap();
    wide(&String::from_utf8(data).unwrap().replace(
        "C:\\WINDOWS",
        &std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\WINDOWS".into()),
    ))
}

#[test]
fn path_failfast_diagnostics_match_original_captures() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-failfast-cases.json")).unwrap();
    let captured: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/path-failfast-reference/capture.json"
    ))
    .unwrap();
    let mut differences = vec![];
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if matches!(
            id,
            "alias-zero-control" | "path-readonly-method" | "interactive-class-failure-recovery"
        ) {
            continue;
        }
        let expected = captured["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let (status, sink) = run(
            case["arguments"].as_str().unwrap(),
            &mut MetadataProvider::default(),
        );
        if status != expected["exit_status"].as_u64().unwrap() as u32
            || sink.stderr != fixture(id, "stderr")
        {
            differences.push(format!(
                "{id}: status {status:#x}, stderr {} vs {} units",
                sink.stderr.0.len(),
                fixture(id, "stderr").0.len()
            ));
        }
        if expected["stdout"]["bytes"] == 0 {
            assert!(
                sink.stdout.0.is_empty(),
                "{id}: parser errors must not render query output"
            );
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn metadata_errors_stop_before_ping_execution_or_confirmation() {
    for stage in [
        PathPreflightStage::Connect,
        PathPreflightStage::Security,
        PathPreflightStage::Class,
        PathPreflightStage::Names,
    ] {
        let mut provider = MetadataProvider {
            fail_stage: Some(stage),
            ..Default::default()
        };
        let (status, sink) = run(
            "/trace:on /failfast:on path Win32_OperatingSystem get Caption",
            &mut provider,
        );
        assert_eq!(status, 0x80070005);
        assert_eq!(provider.calls, ["metadata:.:Win32_OperatingSystem"]);
        assert!(sink.stdout.0.is_empty());
        let stderr = sink.stderr.to_string_lossy();
        let failed = match stage {
            PathPreflightStage::Connect => "4244",
            PathPreflightStage::Security => "4281",
            PathPreflightStage::Class => "3865",
            _ => "3877",
        };
        assert!(stderr.contains(&format!("Line:   {failed}")));
        assert!(!stderr.contains("execengine.cpp") && !stderr.contains("formatengine.cpp"));
    }
}

#[test]
fn successful_metadata_then_failed_ping_never_executes_and_keeps_context_cold() {
    let mut provider = MetadataProvider {
        reject_ping: true,
        ..Default::default()
    };
    let mut session = Session::new(wide("DESKTOP-N4H9E7R")).unwrap();
    for trace in [false, true] {
        let mut sink = Sink::default();
        let result = session.execute(
            &wide(&format!(
                "/trace:{} /failfast:on path Win32_OperatingSystem get Caption",
                if trace { "on" } else { "off" }
            )),
            EntryMode::Interactive,
            &mut provider,
            &mut sink,
        );
        assert_eq!(result.exit_status, 0xABE3);
        assert_eq!(
            &provider.calls[provider.calls.len() - 2..],
            ["metadata:.:Win32_OperatingSystem", "ping:DESKTOP-N4H9E7R"]
        );
        assert!(!sink.stderr.to_string_lossy().contains("Line:   2838"));
    }
    provider.reject_ping = false;
    let mut sink = Sink::default();
    assert_eq!(
        session
            .execute(
                &wide("/trace:on /failfast:off path Win32_OperatingSystem get Caption"),
                EntryMode::Interactive,
                &mut provider,
                &mut sink
            )
            .exit_status,
        0
    );
    let recovered = sink.stderr.to_string_lossy();
    assert!(recovered.contains("Line:   2838"));
    assert!(!recovered.contains("Line:   2787"));
}

#[test]
fn metadata_ping_and_query_run_in_that_order() {
    let mut provider = MetadataProvider::default();
    assert_eq!(
        run(
            "/failfast:on path Win32_OperatingSystem get Caption",
            &mut provider
        )
        .0,
        0
    );
    assert_eq!(
        provider.calls,
        [
            "metadata:.:Win32_OperatingSystem",
            "ping:DESKTOP-N4H9E7R",
            "execute:."
        ]
    );
}

#[test]
fn method_and_write_requests_cannot_execute_or_confirm_before_reachability() {
    for (verb, metadata) in [
        ("call GetOwner", true),
        ("set Priority=8", false),
        ("create Name='WmicNever'", false),
        ("delete", false),
    ] {
        let mut provider = MetadataProvider {
            reject_ping: true,
            ..Default::default()
        };
        let (status, _) = run(
            &format!("/failfast:on path Win32_Process {verb}"),
            &mut provider,
        );
        assert_eq!(status, 0xABE3, "{verb}");
        let expected = if metadata {
            vec!["metadata:.:Win32_Process", "ping:DESKTOP-N4H9E7R"]
        } else {
            vec!["ping:DESKTOP-N4H9E7R"]
        };
        assert_eq!(provider.calls, expected, "{verb}");
    }
}

#[test]
fn keyed_path_preflight_receives_class_metadata_only() {
    let mut provider = MetadataProvider {
        reject_ping: true,
        ..Default::default()
    };
    assert_eq!(
        run(
            "/failfast:on path Win32_Process.Handle='4294967295' get Caption",
            &mut provider
        )
        .0,
        0xABE3
    );
    assert_eq!(
        provider.calls,
        ["metadata:.:Win32_Process", "ping:DESKTOP-N4H9E7R"]
    );
}

#[test]
fn preflight_failure_keeps_later_text_source_and_context_cold() {
    for traced in [false, true] {
        let mut provider = MetadataProvider::default();
        let mut session = Session::new(wide("DESKTOP-N4H9E7R")).unwrap();
        let mut sink = Sink::default();
        assert_eq!(session.execute(&wide(&format!("/trace:{} /failfast:on /node:0.0.0.0 path Win32_OperatingSystem get Caption",if traced {"on"}else{"off"})),EntryMode::Interactive,&mut provider,&mut sink).exit_status,0x800706ba);
        sink = Sink::default();
        assert_eq!(
            session
                .execute(
                    &wide("/trace:on /node:. path Win32_OperatingSystem get Caption"),
                    EntryMode::Interactive,
                    &mut provider,
                    &mut sink
                )
                .exit_status,
            0
        );
        let recovered = sink.stderr.to_string_lossy();
        assert!(recovered.contains("Line:   2787") && recovered.contains("Line:   2838"));
    }
}

#[test]
fn cancelled_preflight_is_quiet_and_never_reaches_ping_or_execution() {
    struct Cancelled;
    impl Management for Cancelled {
        fn preflight_path(
            &mut self,
            r: PathPreflightRequest<'_>,
        ) -> Result<PathPreflightOutcome, PathPreflightError> {
            Err(PathPreflightError {
                stage: PathPreflightStage::Class,
                completed_method_signatures: 0,
                error: error(
                    0xC000013A,
                    "Operation cancelled",
                    "cancel",
                    &r.connection.node,
                ),
            })
        }
        fn reachable(&mut self, _: &WideText, _: u32) -> bool {
            panic!("cancelled preflight must not ping")
        }
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("cancelled preflight must not execute")
        }
    }
    for trace in ["on", "off"] {
        let (status, sink) = run(
            &format!("/trace:{trace} /failfast:on path Win32_Process get Name"),
            &mut Cancelled,
        );
        assert_eq!(status, 0xC000013A);
        assert!(sink.stdout.0.is_empty() && sink.stderr.0.is_empty());
    }
}

#[test]
fn preflight_redacts_raw_and_lossy_passwords_in_metadata_errors() {
    struct Echo;
    impl Management for Echo {
        fn preflight_path(
            &mut self,
            r: PathPreflightRequest<'_>,
        ) -> Result<PathPreflightOutcome, PathPreflightError> {
            let mut message = r.class_name.clone();
            message.0.extend(" / ".encode_utf16());
            message
                .0
                .extend(r.class_name.to_string_lossy().encode_utf16());
            Err(PathPreflightError {
                stage: PathPreflightStage::Class,
                completed_method_signatures: 0,
                error: ManagementError {
                    message,
                    ..error(0x80041002, "", "class", &r.connection.node)
                },
            })
        }
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("metadata failure must not execute")
        }
    }
    for trace in ["on", "off"] {
        let mut secret = wide("secret-");
        secret.0.push(0xd800);
        secret.0.extend("-suffix".encode_utf16());
        let mut command = wide(&format!(
            "/trace:{trace} /failfast:on /user:tester /password:"
        ));
        command.0.extend(&secret.0);
        command.0.extend(" path ".encode_utf16());
        command.0.extend(&secret.0);
        command.0.extend(" get Caption".encode_utf16());
        let mut sink = Sink::default();
        let outcome = Session::new(wide("LOCAL")).unwrap().execute(
            &command,
            EntryMode::Interactive,
            &mut Echo,
            &mut sink,
        );
        assert_eq!(outcome.exit_status, 0x80041002);
        assert!(sink.stdout.0.is_empty());
        assert!(
            !sink
                .stderr
                .0
                .windows(secret.0.len())
                .any(|units| units == secret.0)
        );
        let text = sink.stderr.to_string_lossy();
        assert!(!text.contains("secret-") && text.contains("<redacted>"));
    }
}

#[cfg(not(windows))]
#[test]
fn wsman_preflight_does_not_connect_or_query_instances() {
    use wmic_compat::backend::{ConnectionOptions, wsman::manage::WsmanManagement};
    let mut backend = WsmanManagement::new(Box::new(|_| {
        panic!("metadata-only preflight must not connect")
    }));
    for name in ["Win32_OperatingSystem", "WmicTraceMissingClass"] {
        assert_eq!(
            backend
                .preflight_path(PathPreflightRequest {
                    connection: &ConnectionOptions::default(),
                    class_name: &wide(name),
                    method: None,
                })
                .unwrap(),
            PathPreflightOutcome::Unsupported
        );
    }
}

/// These cases need no provider rows, remote credentials, ping success or methods.
/// Launch the shipped CLI so dispatch/COM/preflight/output bugs cannot hide in mocks.
#[cfg(windows)]
#[test]
fn windows_cli_matches_path_failfast_diagnostics() {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-failfast-cases.json")).unwrap();
    let capture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/path-failfast-reference/capture.json"
    ))
    .unwrap();
    let binary =
        std::env::var_os("WMIC_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_wmic").into());
    let ids = [
        "path-zero",
        "path-zero-trace",
        "path-zero-list",
        "path-zero-rawxml",
        "path-zero-call",
        "path-zero-help",
        "path-missing-class",
        "path-missing-class-trace",
        "path-missing-namespace",
        "path-zero-missing-property",
        "path-zero-instance",
        "path-bare-zero",
        "path-get-all-zero",
        "path-get-all-format-zero",
        "path-missing-class-call",
        "path-unknown-method",
        "path-bare-missing-class",
        "path-get-all-missing-class",
        "path-bare-missing-class-off",
        "path-get-all-missing-class-off",
    ];
    for id in ids {
        let case = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
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
            "{id}: DWORD status"
        );
        for (stream, actual) in [("stdout", stdout), ("stderr", stderr)] {
            let expected = fixture(id, stream).to_string_lossy().into_bytes();
            let at = actual.iter().zip(&expected).position(|(a, b)| a != b);
            assert!(
                actual == expected,
                "{id}: {stream} differs at {at:?}, {} vs {} bytes",
                actual.len(),
                expected.len()
            );
        }
    }
}

#[test]
fn method_metadata_failure_keeps_completed_signature_trace() {
    struct SignatureFailure(u8, PathPreflightStage);
    impl Management for SignatureFailure {
        fn preflight_path(
            &mut self,
            r: PathPreflightRequest<'_>,
        ) -> Result<PathPreflightOutcome, PathPreflightError> {
            Err(PathPreflightError {
                stage: self.1,
                completed_method_signatures: self.0,
                error: error(0x80070005, "Access is denied.", "class", &r.connection.node),
            })
        }
        fn reachable(&mut self, _: &WideText, _: u32) -> bool {
            panic!("failed metadata must not ping")
        }
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("failed metadata must not invoke methods")
        }
    }
    for (completed, stage) in [
        (0, PathPreflightStage::Names),
        (1, PathPreflightStage::Names),
        (1, PathPreflightStage::Method),
    ] {
        let (status, sink) = run(
            "/trace:on /failfast:on path Win32_Process call GetOwner",
            &mut SignatureFailure(completed, stage),
        );
        assert_eq!(status, 0x80070005);
        assert!(sink.stdout.0.is_empty());
        let text = sink.stderr.to_string_lossy();
        let names = "IWbemClassObject::GetNames(NULL, WBEM_FLAG_ALWAYS | WBEM_FLAG_NONSYSTEM_ONLY, NULL, -)";
        assert_eq!(
            text.matches(&format!("SUCCESS: {names}")).count(),
            completed as usize
        );
        assert_eq!(
            text.matches(&format!("FAIL: {names}")).count(),
            usize::from(stage == PathPreflightStage::Names)
        );
        assert!(!text.contains("execengine.cpp") && !text.contains("formatengine.cpp"));
    }
}

#[test]
fn interactive_metadata_failure_then_recovery_matches_original_trace() {
    let mut session = Session::new(wide("DESKTOP-N4H9E7R")).unwrap();
    let mut provider = MetadataProvider::default();
    let mut sink = Sink::default();
    for (command, status) in [
        (
            "/trace:on /failfast:on path WmicTraceMissingClass get Caption",
            0x80041002,
        ),
        (
            "/trace:on /failfast:on path Win32_OperatingSystem get Caption",
            0,
        ),
    ] {
        assert_eq!(
            session
                .execute(
                    &wide(command),
                    EntryMode::Interactive,
                    &mut provider,
                    &mut sink
                )
                .exit_status,
            status
        );
    }
    assert_eq!(
        sink.stderr,
        fixture("interactive-class-failure-recovery", "stderr")
    );
    assert_eq!(
        provider.calls,
        [
            "metadata:.:WmicTraceMissingClass",
            "metadata:.:Win32_OperatingSystem",
            "ping:DESKTOP-N4H9E7R",
            "execute:."
        ]
    );
}
