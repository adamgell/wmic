mod support;
use support::wide;
use wmic_compat::{
    alias::{AliasStore, ResolvedCommand},
    model::*,
    parse::parse_command,
    render::{DestinationProfile, RenderRequest, encode_output, render},
    session::SessionSettings,
};
fn request(command: &str) -> wmic_compat::backend::ResolvedRequest {
    let parsed = parse_command(&wide(command), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(r) = AliasStore::stock()
        .unwrap()
        .resolve(&parsed, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    *r
}
fn os() -> ManagementResult {
    ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: wide("Win32_OperatingSystem"),
            server: Some(wide("DESKTOP-N4H9E7R")),
            properties: vec![
                Property {
                    name: wide("Caption"),
                    value: CimValue::String(wide("Microsoft Windows 11 Pro")),
                    cim_type: 8,
                    system: false,
                    qualifiers: vec![],
                },
                Property {
                    name: wide("Version"),
                    value: CimValue::String(wide("10.0.26200")),
                    cim_type: 8,
                    system: false,
                    qualifiers: vec![],
                },
            ],
            ..Default::default()
        }],
        schema: None,
    })
}
#[test]
fn csv_includes_the_node_column_and_native_blank_lines() {
    let req = request("os get Caption,Version /format:csv");
    let data = os();
    let output = render(RenderRequest {
        request: &req,
        result: Ok(&data),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        encode_output(&output.stdout, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/reference/os-get-csv.stdout.bin")
    );
}
#[test]
fn mof_instance_matches_native_markup() {
    let req = request("os get Caption,Version /format:mof");
    let data = os();
    let output = render(RenderRequest {
        request: &req,
        result: Ok(&data),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        encode_output(&output.stdout, &DestinationProfile::pipe(1252)).unwrap(),
        include_bytes!("fixtures/reference/os-get-mof.stdout.bin")
    );
}

#[test]
fn format_pipeline_retains_quoted_filename_and_parameter_tokens() {
    use wmic_compat::parse::parse_format_spec;
    let spec = parse_format_spec(&wide(
        r#"rawxml,"C:\formats\my-style.xsl":"greeting=hello":"unused=value""#,
    ))
    .unwrap();
    assert_eq!(spec.transforms.len(), 2);
    assert_eq!(spec.transforms[1].name, wide(r#"C:\formats\my-style.xsl"#));
    assert_eq!(
        spec.transforms[1].parameters,
        vec![
            (wide("greeting"), wide("hello")),
            (wide("unused"), wide("value"))
        ]
    );
    assert_eq!(
        parse_format_spec(&wide(r#""parameters.xsl":greeting=hello"#))
            .unwrap_err()
            .code,
        0xABE5
    );
    assert_eq!(
        parse_format_spec(&wide("htable-sortby")).unwrap_err().code,
        0xACB4
    );
}
#[test]
fn command_parser_preserves_raw_format_quotes() {
    let p = parse_command(
        &wide(r#"os get Caption /format:"rawxml,csv""#),
        EntryMode::Interactive,
    )
    .unwrap();
    assert_eq!(p.modifiers[0].raw_value, Some(wide(r#""rawxml,csv""#)));
}

#[test]
fn cim_xml_keeps_null_arrays_and_text_distinct() {
    use wmic_compat::render::to_wmi_xml;
    let data = ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: wide("Sample"),
            properties: vec![
                Property {
                    name: wide("Empty"),
                    value: CimValue::String(wide("")),
                    cim_type: 8,
                    system: false,
                    qualifiers: vec![],
                },
                Property {
                    name: wide("Null"),
                    value: CimValue::Null,
                    cim_type: 8,
                    system: false,
                    qualifiers: vec![],
                },
                Property {
                    name: wide("Values"),
                    value: CimValue::Array(vec![CimValue::String(wide("a & <b>"))]),
                    cim_type: 0x2008,
                    system: false,
                    qualifiers: vec![],
                },
            ],
            ..Default::default()
        }],
        schema: None,
    });
    let xml = to_wmi_xml(&data).unwrap().to_string_lossy();
    assert!(xml.contains("<PROPERTY NAME=\"Empty\" TYPE=\"string\"><VALUE></VALUE></PROPERTY>"));
    assert!(xml.contains("<PROPERTY NAME=\"Null\" TYPE=\"string\"></PROPERTY>"));
    assert!(xml.contains("<VALUE.ARRAY><VALUE>a &amp; &lt;b&gt;</VALUE></VALUE.ARRAY>"));
}

fn owned_string_result() -> ManagementResult {
    ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: wide("Win32_Environment"),
            properties: vec![Property {
                name: wide("VariableValue"),
                value: CimValue::String(wide("a,b\"c & <tag>")),
                cim_type: 8,
                system: false,
                qualifiers: vec![],
            }],
            ..Default::default()
        }],
        schema: None,
    })
}
#[test]
fn classic_csv_preserves_its_unquoted_commas_and_html_entities() {
    let r = request("path Win32_Environment get VariableValue /format:csv");
    let value = owned_string_result();
    let output = render(RenderRequest {
        request: &r,
        result: Ok(&value),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        output.stdout,
        wide("\r\r\nNode,VariableValue\r\r\nDESKTOP-N4H9E7R,a,b\"c &amp; &lt;tag&gt;\r\r\n")
    );
}
#[test]
fn default_table_preserves_text_characters_while_value_escapes_them() {
    let r = request("path Win32_Environment get VariableValue");
    let value = owned_string_result();
    let output = render(RenderRequest {
        request: &r,
        result: Ok(&value),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        output.stdout,
        wide("VariableValue  \r\r\na,b\"c & <tag>  \r\r\n\r\r\n")
    );
}

#[test]
fn command_xml_wraps_results_with_explicit_session_metadata() {
    use wmic_compat::{model::RenderContext, render::to_command_xml};
    let context = RenderContext {
        components_xml: wide(""),
        sequence: 1,
        every_count: 0,
        issued_from: wide("HOST"),
        started_at: wide("10-15-2026T20:02:02"),
        command_line: wide(" os get Caption /format:rawxml"),
        nodes: vec![wide("HOST")],
        settings: vec![
            (wide("NAMESPACE"), wide("root\\cimv2")),
            (wide("USER"), wide("N/A")),
        ],
    };
    let xml = to_command_xml(&os(), &context, &wide("HOST"))
        .unwrap()
        .to_string_lossy();
    assert!(xml.starts_with("<COMMAND SEQUENCENUM=\"1\" ISSUEDFROM=\"HOST\" STARTTIME=\"10-15-2026T20:02:02\" EVERYCOUNT=\"0\">"));
    assert!(xml.contains("<COMMANDLINE> os get Caption /format:rawxml</COMMANDLINE>"));
    assert!(xml.contains("<NODELIST><NODE>HOST</NODE></NODELIST>"));
    assert!(xml.contains("<CONTEXT><NAMESPACE>root\\cimv2</NAMESPACE><USER>N/A</USER></CONTEXT>"));
    assert!(xml.contains("<RESULTS NODE=\"HOST\"><CIM><INSTANCE"));
}

#[test]
fn aggregation_keeps_each_result_node_in_the_xml_envelope() {
    use wmic_compat::{model::RenderContext, render::to_command_xml};
    let mut first = match os() {
        ManagementResult::Objects(s) => s.objects[0].clone(),
        _ => unreachable!(),
    };
    first.result_node = Some(wide("192.168.2.107"));
    let mut second = first.clone();
    second.result_node = Some(wide("HOST"));
    let data = ManagementResult::Objects(ObjectSet {
        objects: vec![first, second],
        schema: None,
    });
    let context = RenderContext {
        issued_from: wide("HOST"),
        ..Default::default()
    };
    let xml = to_command_xml(&data, &context, &wide("HOST"))
        .unwrap()
        .to_string_lossy();
    assert_eq!(xml.matches("<RESULTS NODE=").count(), 2);
    assert!(
        xml.find("<RESULTS NODE=\"192.168.2.107\"").unwrap()
            < xml.find("<RESULTS NODE=\"HOST\"").unwrap()
    );
}

#[test]
fn translation_steps_keep_their_position_relative_to_the_last_format_switch() {
    use wmic_compat::render::compile_format;
    let request = request(
        "os get MUILanguages /translate:NoComma /format:mof /format:csv /translate:BasicXml",
    );
    let spec = compile_format(
        &request,
        &[
            (wide("NoComma"), vec![(wide(","), wide(" "))]),
            (wide("BasicXml"), vec![(wide("&lt;"), wide("<"))]),
        ],
    )
    .unwrap();
    assert_eq!(spec.transforms[0].name, wide("csv"));
    assert_eq!(spec.before[0].name, wide("NoComma"));
    assert_eq!(spec.after[0].name, wide("BasicXml"));
}

#[test]
fn missing_stylesheet_names_reach_format_lookup_instead_of_becoming_syntax_errors() {
    use wmic_compat::parse::parse_format_spec;
    let spec = parse_format_spec(&wide(r#""parameters.xsl",greeting=hello"#)).unwrap();
    assert_eq!(spec.transforms[1].name, wide("greeting=hello"));
    assert_eq!(
        parse_format_spec(&wide(r#""parameters.xsl"(greeting=hello)"#))
            .unwrap()
            .transforms[0]
            .name,
        wide("parameters.xsl(greeting=hello)")
    );
}

#[test]
fn table_width_and_scalar_newlines_match_classic_formatter() {
    let property = |name: &str, value: &str| Property {
        name: name.into(),
        value: CimValue::String(value.into()),
        cim_type: 8,
        system: false,
        qualifiers: vec![],
    };
    let data = ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: "Sample".into(),
            properties: vec![property("A", "字\r\nX"), property("B", "a")],
            ..Default::default()
        }],
        schema: None,
    });
    let req = request("path Sample get A,B");
    let out = render(RenderRequest {
        request: &req,
        result: Ok(&data),
        node: &"HOST".into(),
    })
    .unwrap();
    assert_eq!(
        out.stdout,
        WideText::from("A    B  \r\r\n字X  a  \r\r\n\r\r\n")
    );
}
#[test]
fn value_array_delimiter_has_no_space() {
    assert_eq!(
        wmic_compat::render::display_value(&CimValue::Array(vec![
            CimValue::Uint32(1),
            CimValue::Uint32(2)
        ])),
        WideText::from("{1,2}")
    );
}

#[test]
fn embedded_cim_objects_use_the_sdk_object_property_tags() {
    let result = ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: "Outer".into(),
            properties: vec![Property {
                name: "Child".into(),
                cim_type: 13,
                system: false,
                qualifiers: vec![],
                value: CimValue::Object(Box::new(Record {
                    class: "Inner".into(),
                    ..Default::default()
                })),
            }],
            ..Default::default()
        }],
        schema: None,
    });
    let xml = wmic_compat::render::to_wmi_xml(&result)
        .unwrap()
        .to_string_lossy();
    assert!(
        xml.contains(
            "<PROPERTY.OBJECT NAME=\"Child\"><VALUE.OBJECT><INSTANCE CLASSNAME=\"Inner\">"
        )
    );
}

#[cfg(windows)]
#[test]
fn native_html_arrays_and_empty_results_match_the_microsoft_captures() {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    for (format, array_fixture, empty_fixture) in [
        ("htable", "query-format-066", "query-format-366"),
        ("hform", "query-format-067", "query-format-367"),
    ] {
        let mut req = request(&format!("os get MUILanguages /format:{format}"));
        req.render_context = Some(RenderContext::default());
        req.format_spec = Some(FormatSpec {
            before: vec![],
            after: vec![],
            transforms: vec![TransformSpec {
                name: wide(format),
                parameters: vec![],
            }],
        });
        let data = ManagementResult::Objects(ObjectSet {
            objects: vec![Record {
                class: wide("Win32_OperatingSystem"),
                properties: vec![Property {
                    name: wide("MUILanguages"),
                    value: CimValue::Array(vec![CimValue::String(wide("en-US"))]),
                    cim_type: 0x2008,
                    system: false,
                    qualifiers: vec![],
                }],
                ..Default::default()
            }],
            schema: None,
        });
        for (result, fixture) in [
            (data, array_fixture),
            (
                ManagementResult::Objects(ObjectSet::default()),
                empty_fixture,
            ),
        ] {
            let output = render(RenderRequest {
                request: &req,
                result: Ok(&result),
                node: &wide("DESKTOP-N4H9E7R"),
            })
            .unwrap();
            let want = match fixture {
                "query-format-066" => {
                    include_bytes!("fixtures/compatibility-reference/query-format-066.stdout.bin")
                        .as_slice()
                }
                "query-format-067" => {
                    include_bytes!("fixtures/compatibility-reference/query-format-067.stdout.bin")
                        .as_slice()
                }
                "query-format-366" => {
                    include_bytes!("fixtures/compatibility-reference/query-format-366.stdout.bin")
                        .as_slice()
                }
                _ => include_bytes!("fixtures/compatibility-reference/query-format-367.stdout.bin")
                    .as_slice(),
            };
            assert_eq!(
                encode_output(&output.stdout, &DestinationProfile::pipe(1252)).unwrap(),
                want,
                "{fixture}"
            );
        }
    }
}
