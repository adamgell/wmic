mod support;
use support::wide;
use wmic_compat::{
    backend::methods::{bind_method, convert_input},
    model::*,
};

#[test]
#[cfg(windows)]
fn fullwidth_wmi_method_is_rejected_by_the_provider() {
    use std::process::Command;
    for args in [
        vec![
            "process",
            "where",
            "ProcessId=0",
            "call",
            "ＧｅｔＯｗｎｅｒ",
        ],
        vec![
            "path",
            "Win32_Process",
            "where",
            "ProcessId=0",
            "call",
            "ＧｅｔＯｗｎｅｒ",
        ],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_wmic"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code().map(|s| s as u32),
            Some(0x80041002),
            "{args:?}: {out:?}"
        );
        assert!(out.stdout.is_empty(), "{args:?}: {out:?}");
    }
}
fn parameter(name: &str, ty: u32, id: u32) -> ParameterSchema {
    ParameterSchema {
        cim_type_name: None,
        name: wide(name),
        cim_type: ty,
        id: Some(id),
        optional: true,
        default_value: Some(CimValue::Null),
    }
}
fn method() -> MethodSchema {
    MethodSchema {
        name: wide("Sample"),
        input: vec![parameter("Number", 19, 1), parameter("Text", 8, 0)],
        output: vec![],
        is_static: true,
        qualifiers: vec![],
    }
}
#[test]
fn positional_arguments_use_parameter_id_not_enumeration_order() {
    let bound = bind_method(&method(), &[wide("\"NULL\""), wide("42")]).unwrap();
    assert_eq!(bound[0].name, wide("Text"));
    assert_eq!(bound[0].value, CimValue::String(wide("NULL")));
    assert_eq!(bound[1].value, CimValue::Uint32(42));
}
#[test]
fn named_parameters_are_case_insensitive_and_omissions_keep_defaults() {
    let bound = bind_method(&method(), &[wide("number=7")]).unwrap();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].name, wide("Number"));
    assert_eq!(bound[0].value, CimValue::Uint32(7));
}
#[test]
fn null_empty_and_quoted_null_are_distinct() {
    assert_eq!(convert_input(&wide("NULL"), 8).unwrap(), CimValue::Null);
    assert_eq!(
        convert_input(&wide("\"NULL\""), 8).unwrap(),
        CimValue::String(wide("NULL"))
    );
    assert_eq!(
        convert_input(&wide("\"\""), 8).unwrap(),
        CimValue::String(wide(""))
    );
}
#[test]
fn typed_arrays_numbers_booleans_and_overflow() {
    assert_eq!(
        convert_input(&wide("{1,0x2,3}"), 0x2013).unwrap(),
        CimValue::Array(vec![
            CimValue::Uint32(1),
            CimValue::Uint32(2),
            CimValue::Uint32(3)
        ])
    );
    assert_eq!(
        convert_input(&wide("{\"a,b\",\"\",NULL}"), 0x2008).unwrap(),
        CimValue::Array(vec![
            CimValue::String(wide("a,b")),
            CimValue::String(wide("")),
            CimValue::Null
        ])
    );
    assert_eq!(
        convert_input(&wide("TRUE"), 11).unwrap(),
        CimValue::Boolean(true)
    );
    assert_eq!(
        convert_input(&wide("18446744073709551615"), 21).unwrap(),
        CimValue::Uint64(u64::MAX)
    );
    assert!(convert_input(&wide("256"), 17).is_err());
}
#[test]
fn duplicate_and_unknown_parameters_fail_before_invocation() {
    for args in [
        vec![wide("Text=a"), wide("TEXT=b")],
        vec![wide("Missing=x")],
        vec![wide("a"), wide("2"), wide("extra")],
    ] {
        assert!(bind_method(&method(), &args).is_err());
    }
}

#[test]
/// Pin the native error codes and the first failing name in UTF-16 order.
fn named_method_errors_distinguish_unknown_names_from_output_parameters() {
    let schema = MethodSchema {
        name: wide("GetOwner"),
        input: vec![],
        output: vec![parameter("User", 8, 0), parameter("Domain", 8, 1)],
        is_static: false,
        qualifiers: vec![],
    };
    for (args, code, message) in [
        (
            vec!["Unknown=1"],
            0xB0AB,
            "Unknown - Not a parameter of the verb or method.",
        ),
        (
            vec!["User=x", "Unknown=1"],
            0xB0AB,
            "Unknown - Not a parameter of the verb or method.",
        ),
        (
            vec!["User=x", "Domain=y"],
            0xADE5,
            "Domain - Not a input parameter.",
        ),
        (
            vec!["alpha=1", "Beta=2"],
            0xB0AB,
            "Beta - Not a parameter of the verb or method.",
        ),
        (vec!["\"User\"=x"], 0xADE5, "User - Not a input parameter."),
        (
            vec!["\"\"=1"],
            0xB0AB,
            " - Not a parameter of the verb or method.",
        ),
        (
            vec!["\"Unknown=1\""],
            0xADE6,
            "Verb Or Method has no input parameters.",
        ),
        (
            vec![r#""Unknown\\"=1"#],
            0xADE6,
            "Verb Or Method has no input parameters.",
        ),
        (
            vec!["Unknown[1]=x"],
            0xB0AB,
            "Unknown[1] - Not a parameter of the verb or method.",
        ),
        (
            vec!["{Unknown=1}"],
            0xB0AB,
            "{Unknown - Not a parameter of the verb or method.",
        ),
        (
            vec!["\"Unknown,Name\"=1"],
            0xADE6,
            "Verb Or Method has no input parameters.",
        ),
    ] {
        let args: Vec<_> = args.into_iter().map(wide).collect();
        let error = bind_method(&schema, &args).unwrap_err();
        assert_eq!(error.code, code, "{args:?}: {error:?}");
        assert_eq!(error.message, wide(message), "{args:?}");
    }
}

#[test]
/// Ensure invalid arguments beat help, empty selections and raw WMI name failures.
fn alias_named_parameter_errors_precede_help_and_provider_execution() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct NoProvider;
    impl Management for NoProvider {
        /// Catch accidental provider execution before the argument diagnostic is returned.
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("invalid alias parameters must never execute a provider")
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        /// Require empty stdout and retain the complete stderr diagnostic.
        fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
            if stream == OutputStream::Stdout {
                assert!(text.0.is_empty());
            } else {
                self.0.0.extend(&text.0);
            }
            Ok(())
        }
    }
    for command in [
        "process where ProcessId=0 call GetOwner Unknown=1",
        "process where ProcessId=0 call ＧｅｔＯｗｎｅｒ Unknown=1",
        "process where ProcessId=0 call GetOwner User=x Unknown=1 /?",
        "process where ProcessId=4294967295 call GetOwner Unknown = 1",
    ] {
        let mut sink = Sink::default();
        let outcome = Session::new(wide("HOST")).unwrap().execute(
            &wide(command),
            EntryMode::Interactive,
            &mut NoProvider,
            &mut sink,
        );
        assert_eq!(outcome.exit_status, 0xB0AB, "{command}");
        assert_eq!(
            sink.0,
            wide("Unknown - Not a parameter of the verb or method.\r\r\n"),
            "{command}"
        );
    }
}

#[test]
/// Separate accepted assignment whitespace from a missing value before another name.
fn named_method_arguments_allow_spaces_around_equals_and_reject_missing_values() {
    use wmic_compat::parse::parse_command;
    for line in ["Unknown=1", "Unknown =1", "Unknown= 1", "Unknown = 1"] {
        let command = parse_command(
            &wide(&format!("process call GetOwner {line}")),
            EntryMode::Interactive,
        )
        .unwrap();
        assert_eq!(command.raw_arguments, vec![wide("Unknown=1")], "{line}");
    }
    for line in [
        "Unknown=",
        "Unknown=1 User=",
        "User= Unknown=1",
        "User= Unknown =1",
        "User= Unknown = 1",
        "Unknown= =1",
    ] {
        assert_eq!(
            parse_command(
                &wide(&format!("process call GetOwner {line}")),
                EntryMode::Interactive
            )
            .unwrap_err()
            .code,
            0xADDC,
            "{line}"
        );
    }
}

#[test]
/// Keep positional-format errors distinct from malformed named-list errors.
fn malformed_named_method_tokens_keep_native_lexical_errors() {
    use wmic_compat::parse::parse_command;
    for (tail, code) in [
        ("Unknown(1)=x", 0xB7CE),
        ("Unknown=a=b", 0xADDC),
        ("Unknown,=1", 0xB7CE),
        ("User,Unknown=x", 0xB7CE),
    ] {
        let line = format!("process call GetOwner {tail}");
        assert_eq!(
            parse_command(&wide(&line), EntryMode::Interactive)
                .unwrap_err()
                .code,
            code,
            "{line}"
        );
    }
}

#[test]
/// Exercise valid arrays and quoted delimiters through both parsing and typed binding.
fn method_assignment_parsing_preserves_arrays_and_quoted_equals_values() {
    let schema = MethodSchema {
        name: wide("Sample"),
        input: vec![parameter("Values", 0x2008, 0), parameter("Text", 8, 1)],
        output: vec![],
        is_static: true,
        qualifiers: vec![],
    };
    for tail in [
        r#"{"a=b","x,y"},"c=d""#,
        r#"Values={"a=b","x,y"} Text="c=d""#,
        r#"Values = {"a=b", "x,y"} Text = "c=d""#,
    ] {
        let parsed = wmic_compat::parse::parse_command(
            &wide(&format!("process call Sample {tail}")),
            EntryMode::Interactive,
        )
        .unwrap();
        let bound = bind_method(&schema, &parsed.raw_arguments).unwrap();
        assert_eq!(bound.len(), 2, "{tail}");
        assert_eq!(
            bound[0].value,
            CimValue::Array(vec![
                CimValue::String(wide("a=b")),
                CimValue::String(wide("x,y")),
            ]),
            "{tail}"
        );
        assert_eq!(bound[1].value, CimValue::String(wide("c=d")), "{tail}");
    }
}

#[test]
fn resolved_method_retains_quote_semantics() {
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        parse::parse_command,
        session::SessionSettings,
    };
    let parsed = parse_command(
        &wide("process call Create \"NULL\",NULL"),
        EntryMode::Interactive,
    )
    .unwrap();
    let ResolvedCommand::Management(request) = AliasStore::stock()
        .unwrap()
        .resolve(&parsed, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(request.arguments, vec![wide("\"NULL\""), wide("NULL")]);
}

#[test]
fn method_return_failure_is_not_com_failure() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::{DestinationProfile, OutputError, encode_output},
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Ok(ManagementResult::Methods(vec![MethodResult {
                path: wide(r#"\\DESKTOP-N4H9E7R\ROOT\CIMV2:Win32_Process.Handle="0""#),
                return_value: Some(CimValue::Uint32(2)),
                out_parameters: vec![],
                text: None,
            }]))
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
    let mut sink = Sink::default();
    let outcome = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("process where ProcessId=0 call GetOwner"),
        EntryMode::Interactive,
        &mut Provider,
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(
        encode_output(&sink.stdout, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/method-reference/getowner-pid0.stdout.bin")
    );
    assert_eq!(
        encode_output(&sink.stderr, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/method-reference/getowner-pid0.stderr.bin")
    );
}

#[test]
fn native_parameter_mof_whitespace_matches_wmic() {
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        parse::parse_command,
        render::{DestinationProfile, RenderRequest, encode_output, render},
        session::SessionSettings,
    };
    let command = parse_command(
        &wide("process where ProcessId=0 call GetOwner"),
        EntryMode::Interactive,
    )
    .unwrap();
    let ResolvedCommand::Management(request) = AliasStore::stock()
        .unwrap()
        .resolve(&command, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    let result = ManagementResult::Methods(vec![MethodResult {
        path: wide(r#"\\DESKTOP-N4H9E7R\ROOT\CIMV2:Win32_Process.Handle="0""#),
        return_value: Some(CimValue::Uint32(2)),
        out_parameters: vec![],
        text: Some(wide(
            "\ninstance of __PARAMETERS\n{\n\tReturnValue = 2;\n};\n",
        )),
    }]);
    let out = render(RenderRequest {
        request: &request,
        result: Ok(&result),
        node: &wide("LOCAL"),
    })
    .unwrap();
    assert_eq!(
        encode_output(&out.stdout, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/method-reference/getowner-pid0.stdout.bin")
    );
}

#[test]
fn derived_seealso_dispatches_in_process_with_reference_parent_status() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::{DestinationProfile, OutputError, encode_output},
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            assert_eq!(r.verb, Verb::Get);
            Ok(ManagementResult::Objects(ObjectSet {
                objects: vec![Record {
                    path: Some(wide(
                        r#"\\DESKTOP-N4H9E7R\ROOT\cli:MSFT_CliAlias.FriendlyName="OS""#,
                    )),
                    ..Default::default()
                }],
                schema: None,
            }))
        }
    }
    #[derive(Default)]
    struct Sink(WideText, WideText);
    impl OutputSink for Sink {
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if s == OutputStream::Stdout {
                self.0.0.extend(&t.0)
            } else {
                self.1.0.extend(&t.0)
            }
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let outcome = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("alias OS call SeeAlso"),
        EntryMode::Interactive,
        &mut Provider,
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(
        encode_output(&sink.0, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/method-reference/derived-seealso.stdout.bin")
    );
    assert_eq!(
        encode_output(&sink.1, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/method-reference/derived-seealso.stderr.bin")
    );
}

#[test]
fn external_derivations_launch_only_after_explicit_caller_dispatch() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider {
        launched: bool,
    }
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Ok(ManagementResult::Objects(ObjectSet {
                objects: vec![Record::default()],
                schema: None,
            }))
        }
        fn launch_external(&mut self, command: &WideText) -> Result<u32, ManagementError> {
            assert_eq!(command, &wide("ipconfig /FlushDns"));
            self.launched = true;
            Ok(7)
        }
    }
    struct Sink;
    impl OutputSink for Sink {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Ok(())
        }
    }
    let mut provider = Provider { launched: false };
    let mut session = Session::new(wide("LOCAL")).unwrap();
    session.execute(
        &wide("nicconfig call /?"),
        EntryMode::Interactive,
        &mut provider,
        &mut Sink,
    );
    assert!(!provider.launched);
    let result = session.execute(
        &wide("nicconfig call FlushDns"),
        EntryMode::Interactive,
        &mut provider,
        &mut Sink,
    );
    assert!(provider.launched);
    assert_eq!(result.exit_status, 7);
}

#[test]
fn method_completion_output_precedes_the_next_confirmation() {
    use wmic_compat::{
        backend::{Confirmation, Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!()
        }
        fn execute_with_confirmation(
            &mut self,
            r: &ResolvedRequest,
            c: &mut dyn Confirmation,
        ) -> Result<ManagementResult, ManagementError> {
            for _ in 0..2 {
                assert!(c.confirm(&wide("Confirm? "))?);
                c.method_completed(
                    r,
                    &MethodResult {
                        path: wide("Test=@"),
                        return_value: Some(CimValue::Uint32(0)),
                        out_parameters: vec![],
                        text: None,
                    },
                )?;
            }
            Ok(ManagementResult::Operation(OperationResult {
                action: Verb::Call,
                paths: vec![],
                instances: vec![Record::default()],
            }))
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        fn read_line(&mut self) -> Result<Option<WideText>, OutputError> {
            Ok(Some(wide("Y")))
        }
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if s == OutputStream::Stderr {
                self.0.0.extend(&t.0)
            }
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("process call GetOwner"),
        EntryMode::Interactive,
        &mut Provider,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(sink.0, wide("Confirm? \r\nConfirm? \r\n"));
}

#[test]
fn interactive_method_uses_confirmation_in_place_of_executing_line() {
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        parse::parse_command,
        render::{RenderRequest, render},
        session::SessionSettings,
    };
    let command = parse_command(
        &wide("/interactive:on process where ProcessId=0 call GetOwner"),
        EntryMode::Interactive,
    )
    .unwrap();
    let ResolvedCommand::Management(request) = AliasStore::stock()
        .unwrap()
        .resolve(&command, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    let value = ManagementResult::Methods(vec![MethodResult {
        path: wide("Win32_Process.Handle=\"0\""),
        return_value: Some(CimValue::Uint32(2)),
        out_parameters: vec![],
        text: None,
    }]);
    let out = render(RenderRequest {
        request: &request,
        result: Ok(&value),
        node: &wide("LOCAL"),
    })
    .unwrap();
    assert_eq!(
        out.stdout,
        wide(
            "Method execution successful.\r\r\nOut Parameters:\r\ninstance of __PARAMETERS\r\n{\r\n\tReturnValue = 2;\r\n};\r\n"
        )
    );
}

#[test]
fn named_method_null_strings_follow_the_separate_wmic_conversion_path() {
    let bound = bind_method(&method(), &[wide("Text=NULL")]).unwrap();
    assert_eq!(bound[0].value, CimValue::String(wide("NULL")));
    assert_eq!(
        wmic_compat::backend::write::bind_properties(
            &ClassSchema {
                name: wide("Test"),
                properties: vec![PropertySchema {
                    name: wide("Text"),
                    cim_type: 8,
                    qualifiers: vec![]
                }],
                ..Default::default()
            },
            &[wide("Text=NULL")]
        )
        .unwrap()[0]
            .value,
        CimValue::Null
    );
}
