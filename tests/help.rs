mod support;
use support::wide;
use wmic_compat::{
    alias::AliasStore,
    model::EntryMode,
    parse::parse_command,
    render::{
        DestinationProfile, encode_output,
        help::{HelpRequest, render_help},
    },
};
fn help(command: &str) -> Vec<u8> {
    let command = parse_command(&wide(command), EntryMode::Interactive).unwrap();
    let aliases = AliasStore::stock().unwrap();
    let out = render_help(HelpRequest {
        command: &command,
        aliases: &aliases,
        schema: None,
    })
    .unwrap();
    assert!(out.stderr.0.is_empty());
    encode_output(&out.stdout, &DestinationProfile::pipe(1252)).unwrap()
}
#[test]
fn alias_get_help_matches_native_metadata() {
    assert_eq!(
        help("os get /?"),
        include_bytes!("fixtures/reference/os-get-help.stdout.bin")
    );
}
#[test]
fn alias_usage_matches_native() {
    assert_eq!(
        help("os /?"),
        include_bytes!("fixtures/reference/os-help.stdout.bin")
    );
}
#[test]
fn global_help_matches_native() {
    assert_eq!(
        help("/?"),
        include_bytes!("fixtures/reference/global-help.stdout.bin")
    );
}
#[test]
fn individual_method_help_uses_parameter_metadata() {
    assert_eq!(
        help("process call Create /?"),
        include_bytes!("fixtures/schema-reference/method-help.stdout.bin")
    );
}
#[test]
fn property_help_distinguishes_full_view_from_provider_properties() {
    assert_eq!(
        help("os get Caption /?"),
        include_bytes!("fixtures/schema-reference/property-help.stdout.bin")
    );
}
#[test]
fn schema_help_has_no_instance_property_list() {
    assert_eq!(
        help("class Win32_OperatingSystem get /?"),
        include_bytes!("fixtures/schema-reference/class-get-help.stdout.bin")
    );
}
#[test]
fn every_captured_global_switch_help_matches() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/help-cases.json")).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if id.starts_with("help-switch-") {
            assert_eq!(
                help(case["arguments"].as_str().unwrap()),
                std::fs::read(format!("tests/fixtures/help-reference/{id}.stdout.bin")).unwrap(),
                "{id}"
            );
        }
    }
}
#[test]
fn all_344_captured_help_contexts_match() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/help-cases.json")).unwrap();
    let mut differences = vec![];
    for case in manifest["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let actual = help(case["arguments"].as_str().unwrap());
        let expected =
            std::fs::read(format!("tests/fixtures/help-reference/{id}.stdout.bin")).unwrap();
        if actual != expected {
            let at = actual
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(actual.len().min(expected.len()));
            differences.push(format!(
                "{id} byte {at}: actual {:?}; expected {:?}",
                String::from_utf8_lossy(&actual[at..actual.len().min(at + 90)]),
                String::from_utf8_lossy(&expected[at..expected.len().min(at + 90)])
            ));
        }
    }
    assert!(
        differences.is_empty(),
        "{} mismatches:\n{}",
        differences.len(),
        differences.join("\n")
    );
}
#[test]
fn session_routes_switch_help_before_missing_value_validation() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        model::*,
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct NoProvider;
    impl Management for NoProvider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("help must not enumerate provider instances")
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if !t.0.is_empty() {
                assert_eq!(s, OutputStream::Stdout);
                self.0.0.extend(&t.0);
            }
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let outcome = Session::new(wide("TEST")).unwrap().execute(
        &wide("/NODE /?"),
        EntryMode::Interactive,
        &mut NoProvider,
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(
        encode_output(&sink.0, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/help-reference/help-switch-node.stdout.bin")
    );
}
#[test]
fn valid_alias_property_help_has_column_heading() {
    assert_eq!(
        help("os get Version /?"),
        include_bytes!("fixtures/schema-reference/property-help-valid.stdout.bin")
    );
}
#[test]
fn direct_property_help_uses_native_schema_types() {
    use wmic_compat::model::*;
    let command = parse_command(
        &wide("path Win32_OperatingSystem get Caption /?"),
        EntryMode::Interactive,
    )
    .unwrap();
    let aliases = AliasStore::stock().unwrap();
    let schema = ClassSchema {
        name: wide("Win32_OperatingSystem"),
        properties: vec![PropertySchema {
            name: wide("Caption"),
            cim_type: 8,
            qualifiers: vec![],
        }],
        ..Default::default()
    };
    let out = render_help(HelpRequest {
        command: &command,
        aliases: &aliases,
        schema: Some(&schema),
    })
    .unwrap();
    assert_eq!(
        encode_output(&out.stdout, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/schema-reference/path-property-help.stdout.bin")
    );
}
#[test]
fn session_fetches_schema_for_direct_property_help() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        model::*,
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct SchemaOnly;
    impl Management for SchemaOnly {
        fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            assert!(r.class_mode);
            assert_eq!(r.verb, Verb::Get);
            Ok(ManagementResult::Schema(ClassSchema {
                name: wide("Win32_OperatingSystem"),
                properties: vec![PropertySchema {
                    name: wide("Caption"),
                    cim_type: 8,
                    qualifiers: vec![],
                }],
                ..Default::default()
            }))
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if !t.0.is_empty() {
                assert_eq!(s, OutputStream::Stdout);
                self.0.0.extend(&t.0);
            }
            Ok(())
        }
    }
    let mut sink = Sink::default();
    let out = Session::new(wide("TEST")).unwrap().execute(
        &wide("path Win32_OperatingSystem get Caption /?"),
        EntryMode::Interactive,
        &mut SchemaOnly,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(
        encode_output(&sink.0, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/schema-reference/path-property-help.stdout.bin")
    );
}

#[test]
fn custom_role_empty_writeable_view_does_not_inherit_stock_properties() {
    let aliases = AliasStore::from_json(r#"[{"FriendlyName":"OS","Target":"CustomClass","Connection":{},"Formats":[{"Name":"WRITEABLE","Properties":[]}]}]"#).unwrap();
    let command = parse_command(&wide("os list /?"), EntryMode::Interactive).unwrap();
    let output = render_help(HelpRequest {
        command: &command,
        aliases: &aliases,
        schema: None,
    })
    .unwrap();
    assert!(!output.stdout.to_string_lossy().contains("QuantumLength"));
}
