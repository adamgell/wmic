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
fn explicit_defaults_restore_reference_security_levels() {
    let s = settings("/authlevel:connect /implevel:identify /privileges:disable");
    assert_eq!(s.authentication, 2);
    assert_eq!(s.impersonation, 2);
    assert!(!s.privileges);
    let reset = s
        .for_command(
            &parse_command(
                &wide("/authlevel:pktprivacy /implevel:impersonate /privileges:enable"),
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
        &wide(r#"/node:"node-a","node-b" os get Caption"#),
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
fn aggregated_success_has_no_extra_empty_table_from_a_failed_node() {
    for nodes in [r#""node-a","node-b""#, r#""node-b","node-a""#] {
        let (mut backend, mut sink) = (Ordered::default(), Sink::default());
        let outcome = Session::new(wide("LOCAL")).unwrap().execute(
            &wide(&format!("/node:{nodes} os get Caption")),
            EntryMode::Interactive,
            &mut backend,
            &mut sink,
        );
        assert_eq!(outcome.exit_status, 0x80041003);
        assert!(
            sink.stdout.to_string_lossy().starts_with("Caption"),
            "{:?}",
            sink.stdout
        );
        assert!(sink.stderr.to_string_lossy().contains("Access denied"));
    }
    let (mut backend, mut sink) = (Ordered::default(), Sink::default());
    let outcome = Session::new(wide("LOCAL")).unwrap().execute(
        &wide(r#"/node:"node-b" os get Caption"#),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0x80041003);
    assert_eq!(sink.stdout, wide("\r\r\n\r\r\n"));
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
fn numeric_failfast_matches_captured_windows_crt_conversion() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/legacy-wtol.json")).unwrap();
    for case in fixture["values"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        if input.eq_ignore_ascii_case("on") || input.eq_ignore_ascii_case("off") {
            continue;
        }
        let expected = case["value"].as_i64().unwrap();
        let parsed = parse_command(
            &wide(&format!("/failfast:\"{input}\" context")),
            EntryMode::Interactive,
        )
        .unwrap();
        let actual = SessionSettings::default().for_command(&parsed);
        if expected > 0 {
            let actual = actual.unwrap_or_else(|e| panic!("{input:?}: {e:?}"));
            assert!(actual.failfast, "{input:?}");
            assert_eq!(actual.failfast_timeout_ms, expected as u32, "{input:?}");
        } else {
            assert_eq!(actual.unwrap_err().code, 0xAC68, "{input:?}");
        }
    }
}

#[test]
fn numeric_failfast_accepts_captured_crt_digit_and_space_classes() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/legacy-wtol.json")).unwrap();
    for zero in fixture["digit_zero_units"].as_array().unwrap() {
        let zero = zero.as_u64().unwrap() as u32;
        for digit in 0..10 {
            let input = format!("{}1", char::from_u32(zero + digit).unwrap());
            let actual = settings(&format!("/failfast:\"{input}\" context"));
            assert_eq!(actual.failfast_timeout_ms, digit * 10 + 1, "{input:?}");
        }
    }
    for unit in fixture["space_units"].as_array().unwrap() {
        let input = format!(
            "{}125",
            char::from_u32(unit.as_u64().unwrap() as u32).unwrap()
        );
        assert_eq!(
            settings(&format!("/failfast:\"{input}\" context")).failfast_timeout_ms,
            125,
            "{input:?}"
        );
    }
}

#[test]
fn locale_keeps_the_spelling_of_an_unchanged_value() {
    let initial = settings("/locale:MS_409 context");
    assert_eq!(initial.locale, wide("ms_409"));
    let changed = initial
        .for_command(
            &parse_command(&wide("/locale:MS_411 context"), EntryMode::Interactive).unwrap(),
        )
        .unwrap();
    assert_eq!(changed.locale, wide("MS_411"));
    let same = changed
        .for_command(
            &parse_command(&wide("/locale:ms_411 context"), EntryMode::Interactive).unwrap(),
        )
        .unwrap();
    assert_eq!(same.locale, wide("MS_411"));
    let default = same
        .for_command(
            &parse_command(&wide("/locale:MS_409 context"), EntryMode::Interactive).unwrap(),
        )
        .unwrap();
    assert_eq!(default.locale, wide("MS_409"));
    let reset = default
        .for_command(&parse_command(&wide("/locale:\"\" context"), EntryMode::Interactive).unwrap())
        .unwrap();
    assert_eq!(reset.locale, wide("MS_409"));
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
        &wide(r#"/node:"node-a" /authlevel:pktprivacy os get Caption /every:2"#),
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

#[test]
fn empty_security_and_boolean_values_have_native_errors() {
    for (name, code) in [
        ("authlevel", 0xAC49),
        ("implevel", 0xAC48),
        ("privileges", 0xAC5E),
        ("trace", 0xAC54),
        ("aggregate", 0xAC6F),
        ("interactive", 0xAC66),
        ("failfast", 0xAC68),
    ] {
        let parsed = parse_command(
            &wide(&format!("/{name}:\"\" CONTEXT")),
            EntryMode::Interactive,
        )
        .unwrap();
        assert_eq!(
            SessionSettings::default()
                .for_command(&parsed)
                .unwrap_err()
                .code,
            code,
            "{name}"
        );
    }
}

#[test]
fn localhost_uses_the_computer_name_for_result_labels_while_connecting_to_localhost() {
    #[derive(Default)]
    struct ContextSink(Vec<WideText>);
    impl OutputSink for ContextSink {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Ok(())
        }
        fn context(&mut self, _: &RenderContext, node: &WideText) -> Result<(), OutputError> {
            self.0.push(node.clone());
            Ok(())
        }
    }
    let mut backend = Ordered::default();
    let mut sink = ContextSink::default();
    let result = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("/node:localhost os get Caption /format:csv"),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    assert_eq!(result.exit_status, 0);
    assert_eq!(backend.visited, vec![wide("localhost")]);
    assert_eq!(sink.0.last(), Some(&wide("LOCAL")));
}
