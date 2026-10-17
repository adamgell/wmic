mod support;
use support::wide;
use wmic_compat::{
    alias::{AliasStore, ResolveError, ResolvedCommand, RoleLoader},
    model::*,
    parse::parse_command,
    session::SessionSettings,
};
fn resolve(text: &str) -> ResolvedCommand {
    AliasStore::stock()
        .unwrap()
        .resolve(
            &parse_command(&wide(text), EntryMode::Interactive).unwrap(),
            &SessionSettings::default(),
        )
        .unwrap()
}
#[test]
fn stock_aliases_work_without_root_cli() {
    let store = AliasStore::stock().unwrap();
    assert_eq!(store.definitions().len(), 81);
    for (alias, class) in [
        ("os", "Win32_OperatingSystem"),
        ("csproduct", "Win32_ComputerSystemProduct"),
        ("memorychip", "Win32_PhysicalMemory"),
    ] {
        let ResolvedCommand::Management(r) = resolve(&format!("{alias} get Name")) else {
            panic!()
        };
        assert_eq!(r.class_name, wide(class));
    }
}
struct Missing;
impl RoleLoader for Missing {
    fn load(
        &mut self,
        _: &WideText,
    ) -> Result<Option<Vec<wmic_compat::alias::AliasDefinition>>, ResolveError> {
        Ok(None)
    }
}
#[test]
fn missing_explicit_role_does_not_fallback() {
    let mut store = AliasStore::stock().unwrap();
    let settings = SessionSettings {
        role: wide(r"root\WmicResearchMissingRole"),
        explicit_role: true,
        ..Default::default()
    };
    assert_eq!(
        store.load_role(&settings, &mut Missing).unwrap_err().code,
        0x8004100e
    );
}
#[test]
fn alias_positional_selector_uses_pwhere() {
    let ResolvedCommand::Management(r) = resolve("alias OS list brief") else {
        panic!()
    };
    assert_eq!(r.class_name, wide("Msft_CliAlias"));
    assert_eq!(r.connection.namespace, wide(r"ROOT\cli"));
    assert_eq!(r.selector, Some(wide("FriendlyName = 'OS'")));
    assert_eq!(
        r.display.iter().map(|x| x.0.clone()).collect::<Vec<_>>(),
        vec![wide("FriendlyName"), wide("PWhere"), wide("Target")]
    );
}
#[test]
fn alias_seealso_expands_to_derived_command() {
    for line in [
        "alias OS call SeeAlso",
        "alias OS SeeAlso",
        "alias OS call ＳｅｅＡｌｓｏ",
        "alias OS ＳｅｅＡｌｓｏ",
    ] {
        let ResolvedCommand::Derived(d) = resolve(line) else {
            panic!()
        };
        assert_eq!(d.template, wide("wmic Alias #__PATH assoc"));
        assert_eq!(
            d.expand(&Record {
                wmi_xml: None,
                result_node: None,
                class: wide("MSFT_CliAlias"),
                path: Some(wide(r#"MSFT_CliAlias.FriendlyName="OS""#)),
                server: None,
                properties: vec![]
            })
            .unwrap(),
            wide(r#"wmic Alias MSFT_CliAlias.FriendlyName="OS" assoc"#)
        );
        assert_eq!(d.selection.selector, Some(wide("FriendlyName = 'OS'")));
    }
}
#[test]
fn process_shorthand_and_undefined_pwhere() {
    let ResolvedCommand::Management(r) = resolve("process 4 get Name") else {
        panic!()
    };
    assert_eq!(r.selector, Some(wide("ProcessId='4'")));
    let store = AliasStore::stock().unwrap();
    let err = store
        .resolve(
            &parse_command(&wide("os ge Caption"), EntryMode::Interactive).unwrap(),
            &SessionSettings::default(),
        )
        .unwrap_err();
    assert_eq!(err.code, 0xABEA);
}
#[test]
fn namespace_switch_does_not_override_alias_connection() {
    let settings = SessionSettings {
        namespace: wide(r"root\default"),
        explicit_namespace: true,
        ..Default::default()
    };
    let ResolvedCommand::Management(r) = AliasStore::stock()
        .unwrap()
        .resolve(
            &parse_command(&wide("os get Caption"), EntryMode::Interactive).unwrap(),
            &settings,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(r.connection.namespace, wide(r"ROOT\CIMV2"));
}
#[test]
fn external_verb_is_resolved_without_execution() {
    for line in ["nicconfig call FlushDns", "nicconfig call ＦｌｕｓｈＤｎｓ"] {
        let ResolvedCommand::External(d) = resolve(line) else {
            panic!()
        };
        assert_eq!(d.template, wide("ipconfig /FlushDns"));
    }
}

#[test]
fn fullwidth_wmi_method_name_is_preserved_for_provider_validation() {
    let ResolvedCommand::Management(r) = resolve("process call ＧｅｔＯｗｎｅｒ") else {
        panic!()
    };
    assert_eq!(r.method, Some(wide("ＧｅｔＯｗｎｅｒ")));
}
#[test]
fn all_views_preserve_derived_properties_and_names() {
    let store = AliasStore::stock().unwrap();
    assert_eq!(
        store
            .definitions()
            .iter()
            .map(|a| a.formats.len())
            .sum::<usize>(),
        426
    );
    let os = store.find(&wide("OS")).unwrap();
    assert_eq!(
        os.formats
            .iter()
            .find(|f| f.name == "FULL")
            .unwrap()
            .format
            .as_deref(),
        Some("list")
    );
    assert!(
        store
            .definitions()
            .iter()
            .flat_map(|a| &a.formats)
            .all(|f| !f.name.is_empty())
    );
}
#[test]
fn unknown_alias_preserves_native_code() {
    assert_eq!(
        AliasStore::stock()
            .unwrap()
            .resolve(
                &parse_command(
                    &wide("WmicResearchMissing get Name"),
                    EntryMode::Interactive
                )
                .unwrap(),
                &SessionSettings::default()
            )
            .unwrap_err()
            .code,
        0xAC67
    );
}
#[test]
fn alias_connection_password_is_retained_without_debug_exposure() {
    let store=AliasStore::from_json(r#"[{"FriendlyName":"Sample","Target":"Win32_OperatingSystem","Connection":{"NameSpace":"root\\cimv2","User":"DOMAIN\\user","Password":"alias-test-only-secret"}}]"#).unwrap();
    assert!(!format!("{store:?}").contains("alias-test-only-secret"));
    let p = parse_command(&wide("Sample get Caption"), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(r) = store.resolve(&p, &SessionSettings::default()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        r.connection.password.unwrap().expose(),
        &wide("alias-test-only-secret")
    );
}

#[test]
fn unicode_alias_and_pwhere_from_clause_resolve_without_losing_utf16() {
    let store=AliasStore::from_json(r#"[{"FriendlyName":"Café","Target":"Select * from BaseClass","PWhere":"FROM OtherClass WHERE Name='#Value'","Connection":{"NameSpace":"root\\Test"}}]"#).unwrap();
    let parsed = parse_command(&wide("CAFÉ example get Name"), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(request) =
        store.resolve(&parsed, &SessionSettings::default()).unwrap()
    else {
        panic!()
    };
    assert_eq!(request.class_name, wide("OtherClass"));
    assert_eq!(
        wmic_compat::backend::build_query(&request),
        wide("SELECT Name FROM OtherClass WHERE Name='example'")
    );
}

#[test]
fn packaged_aliases_are_queryable_without_the_cli_namespace() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Missing;
    impl Management for Missing {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("bundled ALIAS introspection must not query absent root\\cli")
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if s == OutputStream::Stdout {
                self.0.0.extend(&t.0)
            }
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let out = Session::new(wide("HOST")).unwrap().execute(
        &wide("alias OS get FriendlyName,Target"),
        EntryMode::Interactive,
        &mut Missing,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
    assert!(sink.0.to_string_lossy().contains("Win32_OperatingSystem"));
    assert!(sink.0.to_string_lossy().contains("OS"));
}

#[test]
fn virtual_alias_predicates_support_wql_boolean_null_and_like() {
    let mut store = AliasStore::stock().unwrap();
    struct Absent;
    impl wmic_compat::alias::RoleLoader for Absent {
        fn load(
            &mut self,
            _: &WideText,
        ) -> Result<
            Option<Vec<wmic_compat::alias::AliasDefinition>>,
            wmic_compat::alias::ResolveError,
        > {
            Ok(None)
        }
    }
    let settings = SessionSettings::default();
    store.load_role(&settings, &mut Absent).unwrap();
    for (predicate, names) in [
        ("FriendlyName='OS' OR FriendlyName='CPU'", vec!["OS", "CPU"]),
        ("PWhere IS NULL AND FriendlyName LIKE 'O%'", vec![]),
        (
            "NOT (FriendlyName LIKE '%S%') AND FriendlyName='CPU'",
            vec!["CPU"],
        ),
        ("FriendlyName LIKE '[O]S'", vec!["OS"]),
    ] {
        let command = format!("alias where \"{predicate}\" get FriendlyName");
        let ResolvedCommand::Management(r) = store
            .resolve(
                &parse_command(&wide(&command), EntryMode::Interactive).unwrap(),
                &settings,
            )
            .unwrap()
        else {
            panic!()
        };
        let ManagementResult::Objects(objects) =
            store.virtual_query(&r, &wide("HOST")).unwrap().unwrap()
        else {
            panic!()
        };
        let actual = objects
            .objects
            .iter()
            .map(|o| match &o.properties[0].value {
                CimValue::String(s) => s.to_string_lossy(),
                _ => panic!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, names, "{predicate}");
    }
}

#[test]
fn explicit_default_role_uses_the_bundle_but_an_absent_custom_role_errors() {
    struct Missing;
    impl wmic_compat::alias::RoleLoader for Missing {
        fn load(
            &mut self,
            _: &WideText,
        ) -> Result<
            Option<Vec<wmic_compat::alias::AliasDefinition>>,
            wmic_compat::alias::ResolveError,
        > {
            Ok(None)
        }
    }
    let mut store = AliasStore::stock().unwrap();
    let mut settings = SessionSettings {
        explicit_role: true,
        ..Default::default()
    };
    store.load_role(&settings, &mut Missing).unwrap();
    assert!(store.find(&wide("os")).is_some());
    settings.role = wide("root\\custom");
    assert_eq!(
        store.load_role(&settings, &mut Missing).unwrap_err().code,
        0x8004100E
    );
}

#[test]
fn virtual_aliases_reject_unknown_properties_even_under_or() {
    let mut store = AliasStore::stock().unwrap();
    struct Missing;
    impl wmic_compat::alias::RoleLoader for Missing {
        fn load(
            &mut self,
            _: &WideText,
        ) -> Result<
            Option<Vec<wmic_compat::alias::AliasDefinition>>,
            wmic_compat::alias::ResolveError,
        > {
            Ok(None)
        }
    }
    store
        .load_role(&SessionSettings::default(), &mut Missing)
        .unwrap();
    let parsed = parse_command(
        &wide("alias where \"FriendlyName='OS' OR Missing='x'\" get FriendlyName"),
        EntryMode::Interactive,
    )
    .unwrap();
    let ResolvedCommand::Management(r) =
        store.resolve(&parsed, &SessionSettings::default()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        store
            .virtual_query(&r, &wide("HOST"))
            .unwrap()
            .unwrap_err()
            .code,
        0x80041017
    );
}
