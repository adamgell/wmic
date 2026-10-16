mod support;
use support::wide;
use wmic_compat::{
    backend::{ConnectionOptions, Management, ResolvedRequest, SecretWide},
    model::*,
    parse::parse_command,
    render::OutputError,
    session::{OutputSink, OutputStream, Session, SessionSettings, load_nodes},
};
fn settings(text: &str) -> SessionSettings {
    SessionSettings::default()
        .for_command(&parse_command(&wide(text), EntryMode::Interactive).unwrap())
        .unwrap()
}
#[test]
fn switches_reset_to_reference_security_defaults() {
    let s = settings("/authlevel:connect /implevel:identify /privileges:disable");
    assert_eq!(s.authentication, 2);
    assert_eq!(s.impersonation, 2);
    assert!(!s.privileges);
    let reset = s
        .for_command(
            &parse_command(
                &wide("/authlevel: /implevel: /privileges:"),
                EntryMode::Interactive,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(reset.authentication, 6);
    assert_eq!(reset.impersonation, 3);
    assert!(reset.privileges);
}
#[test]
fn invalid_security_switches_have_measured_status() {
    for (arg, code) in [
        ("/authlevel:bogus", 0xAC49),
        ("/implevel:bogus", 0xAC48),
        ("/privileges:bogus", 0xAC5E),
    ] {
        let p = parse_command(&wide(arg), EntryMode::Interactive).unwrap();
        assert_eq!(
            SessionSettings::default().for_command(&p).unwrap_err().code,
            code
        );
    }
}
#[test]
fn node_list_files_and_duplicates_preserve_first_order() {
    let p = std::env::temp_dir().join(format!("wmic-nodes-{}.txt", std::process::id()));
    std::fs::write(&p, b"node-a\r\nnode-b\r\nNODE-A\r\n").unwrap();
    let actual = load_nodes(&wide(&format!("@{},node-c", p.display()))).unwrap();
    std::fs::remove_file(p).unwrap();
    assert_eq!(actual, vec![wide("node-a"), wide("node-b"), wide("node-c")]);
}
#[test]
fn connection_diagnostics_never_serialize_password() {
    let c = ConnectionOptions {
        user: Some(wide("domain\\user")),
        password: Some(SecretWide::new(wide("test-only-secret"))),
        ..Default::default()
    };
    assert!(
        !serde_json::to_string(&c)
            .unwrap()
            .contains("test-only-secret")
    );
    assert!(!format!("{c:?}").contains("test-only-secret"));
}
#[derive(Default)]
struct Ordered {
    visited: Vec<WideText>,
}
impl Management for Ordered {
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        self.visited.push(r.connection.node.clone());
        if r.connection.node == wide("node-b") {
            return Err(ManagementError {
                code: 0x80041003,
                target: r.connection.node.clone(),
                message: wide("Access denied"),
                operation: wide("enumerate"),
                extended: vec![],
                partial: None,
            });
        }
        Ok(ManagementResult::Objects(ObjectSet {
            objects: vec![Record {
                class: wide("Win32_OperatingSystem"),
                properties: vec![Property {
                    name: wide("Caption"),
                    value: CimValue::String(wide("First node")),
                    cim_type: 8,
                    system: false,
                    qualifiers: vec![],
                }],
                ..Default::default()
            }],
            schema: None,
        }))
    }
}
#[derive(Default)]
struct Sink {
    stdout: WideText,
    stderr: WideText,
}
impl OutputSink for Sink {
    fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
        let out = if s == OutputStream::Stdout {
            &mut self.stdout
        } else {
            &mut self.stderr
        };
        out.0.extend(&t.0);
        Ok(())
    }
}
#[test]
fn ordered_nodes_preserve_success_before_enumeration_failure() {
    let (mut b, mut sink) = (Ordered::default(), Sink::default());
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("/node:node-a,node-b os get Caption"),
        EntryMode::Interactive,
        &mut b,
        &mut sink,
    );
    assert_eq!(b.visited, vec![wide("node-a"), wide("node-b")]);
    assert_eq!(out.nodes[1].node, wide("node-b"));
    assert_eq!(out.nodes[1].exit_status, 0x80041003);
    assert!(sink.stdout.to_string_lossy().contains("First node"));
    assert!(sink.stderr.to_string_lossy().contains("Node - node-b"));
}
#[test]
fn conflicting_authority_and_user_domain_is_rejected_by_connection_validation() {
    let c = ConnectionOptions {
        user: Some(wide("DOMAIN\\user")),
        authority: Some(wide("NTLMDOMAIN:DOMAIN")),
        ..Default::default()
    };
    assert!(c.validate().is_err());
}
#[test]
fn trace_never_echoes_raw_credential_command() {
    let (mut b, mut sink) = (Ordered::default(), Sink::default());
    Session::new(wide("LOCAL")).unwrap().execute(
        &wide("/trace:on /user:domain\\user /password:test-only-secret os get Caption"),
        EntryMode::Interactive,
        &mut b,
        &mut sink,
    );
    assert!(!sink.stderr.to_string_lossy().contains("test-only-secret"));
    assert!(!sink.stderr.0.is_empty());
}
#[test]
fn numeric_failfast_keeps_the_requested_timeout() {
    let s = settings("/failfast:125");
    assert!(s.failfast);
    assert_eq!(s.failfast_timeout_ms, 125);
    assert_eq!(SessionSettings::default().failfast_timeout_ms, 5000);
}

#[test]
fn node_file_reference_can_quote_its_path() {
    let p = std::env::temp_dir().join(format!("wmic nodes {}.txt", std::process::id()));
    std::fs::write(&p, b"node-a\r\n").unwrap();
    let value = wide(&format!("@\"{}\"", p.display()));
    let actual = load_nodes(&value);
    std::fs::remove_file(p).unwrap();
    assert_eq!(actual.unwrap(), vec![wide("node-a")]);
}
#[test]
fn ntlm_authority_supplies_proxy_domain_for_unqualified_username() {
    let c = ConnectionOptions {
        user: Some(wide("user")),
        authority: Some(wide("NTLMDOMAIN:DOMAIN")),
        ..Default::default()
    };
    assert_eq!(c.identity_parts(), (wide("user"), wide("DOMAIN")));
}
#[test]
fn aggregate_format_failure_keeps_its_diagnostic() {
    let (mut b, mut sink) = (Ordered::default(), Sink::default());
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("os get Caption /format:WmicResearchMissingFormat"),
        EntryMode::Interactive,
        &mut b,
        &mut sink,
    );
    assert_ne!(out.exit_status, 0);
    assert!(sink.stderr.to_string_lossy().contains("Invalid XSL format"));
}
#[test]
fn repeated_queries_keep_their_connection_settings() {
    use wmic_compat::session::RepeatScheduler;
    struct Once(bool);
    impl RepeatScheduler for Once {
        fn wait(&mut self, seconds: u32) -> bool {
            assert_eq!(seconds, 2);
            let again = self.0;
            self.0 = false;
            again
        }
    }
    let (mut b, mut sink) = (Ordered::default(), Sink::default());
    let out = Session::new(wide("LOCAL")).unwrap().execute_with_scheduler(
        &wide("/node:node-a /authlevel:pktprivacy os get Caption /every:2"),
        EntryMode::Interactive,
        &mut b,
        &mut sink,
        &mut Once(true),
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(b.visited, vec![wide("node-a"), wide("node-a")]);
}
#[test]
fn help_does_not_start_a_repetition_loop() {
    use wmic_compat::session::RepeatScheduler;
    struct Never;
    impl RepeatScheduler for Never {
        fn wait(&mut self, _: u32) -> bool {
            panic!("help must finish without a repetition wait")
        }
    }
    let (mut b, mut sink) = (Ordered::default(), Sink::default());
    Session::new(wide("LOCAL")).unwrap().execute_with_scheduler(
        &wide("os get /every:2 /?"),
        EntryMode::Interactive,
        &mut b,
        &mut sink,
        &mut Never,
    );
    assert!(b.visited.is_empty());
}

#[test]
fn non_aggregate_partial_enumeration_preserves_rows_and_failure() {
    struct Partial;
    impl Management for Partial {
        fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Err(ManagementError {
                code: 0x80041001,
                target: r.class_name.clone(),
                message: wide("Generic failure"),
                operation: wide("enumerate"),
                extended: vec![],
                partial: Some(Box::new(ManagementResult::Objects(ObjectSet {
                    objects: vec![Record {
                        properties: vec![Property {
                            name: wide("Caption"),
                            value: CimValue::String(wide("Before failure")),
                            cim_type: 8,
                            system: false,
                            qualifiers: vec![],
                        }],
                        ..Default::default()
                    }],
                    schema: None,
                }))),
            })
        }
    }
    let mut sink = Sink::default();
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("/aggregate:off os get Caption"),
        EntryMode::Interactive,
        &mut Partial,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0x80041001);
    assert!(sink.stdout.to_string_lossy().contains("Before failure"));
    assert!(sink.stderr.to_string_lossy().contains("Generic failure"));
}
