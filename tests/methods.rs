mod support;
use support::wide;
use wmic_compat::{
    backend::methods::{bind_method, convert_input},
    model::*,
};
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
