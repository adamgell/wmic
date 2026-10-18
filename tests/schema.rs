mod support;
use support::wide;
use wmic_compat::{
    alias::{AliasStore, ResolvedCommand},
    model::*,
    parse::parse_command,
    session::SessionSettings,
};
fn resolved(text: &str) -> wmic_compat::backend::ResolvedRequest {
    let p = parse_command(&wide(text), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(r) = AliasStore::stock()
        .unwrap()
        .resolve(&p, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    *r
}
#[test]
fn class_get_rejects_instance_property_lists() {
    assert_eq!(
        parse_command(
            &wide("class Win32_OperatingSystem get Caption"),
            EntryMode::Interactive
        )
        .unwrap_err()
        .code,
        0xACBC
    );
}
#[test]
fn list_requires_an_alias() {
    for s in [
        "path Win32_OperatingSystem list full",
        "class Win32_OperatingSystem list full",
    ] {
        let p = parse_command(&wide(s), EntryMode::Interactive).unwrap();
        assert_eq!(
            AliasStore::stock()
                .unwrap()
                .resolve(&p, &SessionSettings::default())
                .unwrap_err()
                .code,
            0xABE7
        );
    }
}
#[test]
fn explicit_list_defaults_to_full_but_bare_alias_gets_all_schema_properties() {
    let bare = resolved("os");
    assert_eq!(bare.verb, Verb::Get);
    assert!(bare.properties.is_empty());
    let list = resolved("os list");
    assert_eq!(list.verb, Verb::List);
    assert!(
        list.display
            .iter()
            .any(|(_, source)| source == &wide("QuantumType"))
    );
}
#[test]
fn direct_targets_default_to_get() {
    for s in ["path Win32_OperatingSystem", "class Win32_OperatingSystem"] {
        assert_eq!(resolved(s).verb, Verb::Get);
    }
}
#[test]
fn association_filters_are_retained_exactly() {
    let r = resolved(
        "os assoc /resultclass:Win32_ComputerSystem /assocclass:Win32_SystemOperatingSystem /resultrole:GroupComponent",
    );
    let a = r.association.unwrap();
    assert_eq!(a.result_class, Some(wide("Win32_ComputerSystem")));
    assert_eq!(a.assoc_class, Some(wide("Win32_SystemOperatingSystem")));
    assert_eq!(a.result_role, Some(wide("GroupComponent")));
}
#[test]
fn system_view_requests_system_properties() {
    let r = resolved("os list system");
    assert_eq!(r.display.len(), 10);
    assert!(r.display.iter().any(|(_, p)| p == &wide("__PATH")));
}
#[test]
fn unquoted_unc_alias_positional_matches_native_switch_error() {
    assert_eq!(parse_command(&wide(r#"alias \\DESKTOP-N4H9E7R\ROOT\cli:MSFT_CliAlias.FriendlyName="OS" get FriendlyName"#),EntryMode::Interactive).unwrap_err().code,0xAC5C);
}
#[test]
fn explicit_list_projects_alias_derivations() {
    let r = resolved("os list writeable");
    assert_eq!(
        wmic_compat::backend::build_query(&r),
        wide(
            "SELECT CurrentTimeZone,Description,ForegroundApplicationBoost,QuantumLength,QuantumType FROM Win32_OperatingSystem"
        )
    );
}
#[test]
fn path_object_expression_reaches_wql_instead_of_getobject() {
    let r = resolved("path Win32_OperatingSystem=@ get Caption");
    assert_eq!(
        wmic_compat::backend::build_query(&r),
        wide("SELECT Caption FROM Win32_OperatingSystem=@")
    );
}
#[test]
fn default_alias_query_keeps_target_query_and_filter() {
    let r = resolved("os where \"Caption='Microsoft Windows 11 Pro'\" get Caption");
    assert_eq!(
        wmic_compat::backend::build_query(&r),
        wide("SELECT Caption FROM Win32_OperatingSystem WHERE Caption='Microsoft Windows 11 Pro'")
    );
}
#[test]
fn association_results_do_not_emit_an_extra_empty_query() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Backend;
    impl Management for Backend {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Ok(ManagementResult::Associations(vec![AssociationGroup {
                source: Record {
                    class: wide("Win32_OperatingSystem"),
                    path: Some(wide("Win32_OperatingSystem=@")),
                    ..Default::default()
                },
                associated: vec![],
            }]))
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if s == OutputStream::Stderr {
                self.0.0.extend(&t.0)
            }
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let outcome = Session::new(wide("TEST")).unwrap().execute(
        &wide("os assoc /resultclass:Missing"),
        EntryMode::Interactive,
        &mut Backend,
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(
        sink.0,
        wide("Win32_OperatingSystem=@ - No Associator(s) Available.\r\r\n")
    );
}
#[test]
fn relative_alias_object_expression_is_forwarded_as_from_expression() {
    let r = resolved(r#"alias MSFT_CliAlias.FriendlyName="OS" get FriendlyName"#);
    assert_eq!(
        wmic_compat::backend::build_query(&r),
        wide(r#"SELECT FriendlyName FROM MSFT_CliAlias.FriendlyName="OS""#)
    );
}
#[test]
fn resolved_request_diagnostics_do_not_retain_password_modifier() {
    let p = parse_command(
        &wide("/password:test-only-secret os get Caption"),
        EntryMode::Interactive,
    )
    .unwrap();
    let settings = SessionSettings::default().for_command(&p).unwrap();
    let ResolvedCommand::Management(r) =
        AliasStore::stock().unwrap().resolve(&p, &settings).unwrap()
    else {
        panic!()
    };
    assert!(
        !serde_json::to_string(&r)
            .unwrap()
            .contains(&serde_json::to_string(&wide("test-only-secret")).unwrap())
    );
    assert!(!format!("{r:?}").contains(&format!("{:?}", wide("test-only-secret"))));
}
#[test]
fn live_role_loader_overrides_packaged_alias_and_connection_defaults() {
    use wmic_compat::{
        alias::{AliasDefinition, ResolveError},
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    #[derive(Default)]
    struct Backend {
        node: Option<WideText>,
    }
    impl Management for Backend {
        fn load_aliases(
            &mut self,
            role: &WideText,
            selected: Option<&WideText>,
            _: &WideText,
        ) -> Result<Option<Vec<AliasDefinition>>, ResolveError> {
            assert_eq!(role, &wide("root\\Custom"));
            assert_eq!(selected, Some(&wide("Sample")));
            Ok(Some(serde_json::from_str(r#"[{"FriendlyName":"Sample","Target":"TestClass","Connection":{"NameSpace":"root\\Test","Server":"node-a"}}]"#).unwrap()))
        }
        fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            self.node = Some(r.connection.node.clone());
            assert_eq!(r.connection.namespace, wide("root\\Test"));
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    struct Sink;
    impl OutputSink for Sink {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Ok(())
        }
    }
    let mut backend = Backend::default();
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide(r"wmic.exe /role:\\root\Custom Sample get Caption"),
        EntryMode::Process,
        &mut backend,
        &mut Sink,
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(backend.node, Some(wide("node-a")));
}

/// CALL help reads schema, but its connection error must retain method-call presentation.
#[test]
fn method_help_connection_errors_do_not_acquire_query_output() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct FailedConnection;
    impl Management for FailedConnection {
        /// Model only the external connection failure, keeping session rendering real.
        fn execute(
            &mut self,
            request: &ResolvedRequest,
        ) -> Result<ManagementResult, ManagementError> {
            assert_eq!(request.verb, Verb::Get);
            assert!(request.class_mode);
            assert!(request.method.is_none());
            Err(ManagementError {
                code: 0x8004100E,
                target: request.class_name.clone(),
                message: "Invalid namespace".into(),
                operation: "query".into(),
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
        /// Retain complete output on both channels, including empty-query formatting.
        fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
            match stream {
                OutputStream::Stdout => self.stdout.0.extend(&text.0),
                OutputStream::Stderr => self.stderr.0.extend(&text.0),
            }
            Ok(())
        }
    }
    for (line, query) in [
        ("class Win32_Process get", true),
        ("path Win32_Process call WmicCompatMissingMethod /?", false),
        ("class Win32_Process call WmicCompatMissingMethod /?", false),
    ] {
        let mut sink = Sink::default();
        let result = Session::new(wide("HOST")).unwrap().execute(
            &wide(line),
            EntryMode::Interactive,
            &mut FailedConnection,
            &mut sink,
        );
        assert_eq!(result.exit_status, 0x8004100E, "{line}");
        // Windows uses the default MOF transform; the portable fallback uses two lines.
        let expected_stdout = if !query {
            ""
        } else if cfg!(windows) {
            "\r\r\n"
        } else {
            "\r\r\n\r\r\n"
        };
        assert_eq!(sink.stdout, wide(expected_stdout), "{line}");
        let expected = if query {
            "Node - HOST\r\r\nERROR:\r\r\nDescription = Invalid namespace\r\r\n"
        } else {
            "ERROR:\r\r\nDescription = Invalid namespace\r\r\n"
        };
        assert_eq!(sink.stderr, wide(expected), "{line}");
    }
}
