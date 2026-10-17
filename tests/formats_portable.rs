//! Built-in stylesheets rendered without an XSL engine, compared with captured output.
use wmic_compat::{
    model::*,
    render::{finish_text, native::transform_builtin},
};

const RAWXML: &str = include_str!("fixtures/reference/os-get-rawxml.stdout.bin");
const FORMAT_RAWXML: &str = include_str!("fixtures/format-reference/format-rawxml.stdout.bin");
const TYPES_RAWXML: &str = include_str!("fixtures/format-reference/format-types-rawxml.stdout.bin");
const SYSTEM_RAWXML: &str =
    include_str!("fixtures/format-reference/format-rawxml-system.stdout.bin");
const REFERENCE_RAWXML: &str =
    include_str!("fixtures/format-reference/reference-rawxml.stdout.bin");
const OBJECT_RAWXML: &str = include_str!("fixtures/format-reference/object-rawxml.stdout.bin");

fn wide(text: &str) -> WideText {
    WideText(text.encode_utf16().collect())
}

fn spec(stages: &[(&str, &[(&str, &str)])]) -> FormatSpec {
    FormatSpec {
        transforms: stages
            .iter()
            .map(|(name, parameters)| TransformSpec {
                name: wide(name),
                parameters: parameters.iter().map(|(n, v)| (wide(n), wide(v))).collect(),
            })
            .collect(),
        before: vec![],
        after: vec![],
    }
}

fn render(xml: &str, name: &str) -> Result<WideText, wmic_compat::render::RenderError> {
    transform_builtin(&wide(xml), &spec(&[(name, &[])])).map(|o| o.stdout)
}

fn expect(xml: &str, name: &str, golden: &str) {
    let out = render(xml, name).unwrap();
    let expected = wide(golden);
    let at = out
        .0
        .iter()
        .zip(&expected.0)
        .position(|(a, b)| a != b)
        .unwrap_or(out.0.len().min(expected.0.len()));
    assert!(
        out == expected,
        "{name}: first difference at unit {at}: got {} units, expected {}",
        out.0.len(),
        expected.0.len()
    );
}

#[test]
fn rawxml_reproduces_every_captured_envelope() {
    for golden in [
        RAWXML,
        FORMAT_RAWXML,
        TYPES_RAWXML,
        SYSTEM_RAWXML,
        REFERENCE_RAWXML,
        OBJECT_RAWXML,
    ] {
        expect(golden, "rawxml", golden);
    }
}

#[test]
fn compact_command_xml_becomes_the_captured_envelope() {
    let compact: String = RAWXML
        .replace("\r\r\n", "")
        .replace("\r\n", "")
        .replace('\n', "");
    expect(&compact, "rawxml", RAWXML);
}

#[test]
fn text_formats_match_the_os_get_goldens() {
    expect(
        RAWXML,
        "csv",
        include_str!("fixtures/reference/os-get-csv.stdout.bin"),
    );
    expect(
        RAWXML,
        "table",
        include_str!("fixtures/reference/os-get.stdout.bin"),
    );
    expect(
        RAWXML,
        "list",
        include_str!("fixtures/reference/os-get-list.stdout.bin"),
    );
    expect(
        RAWXML,
        "mof",
        include_str!("fixtures/reference/os-get-mof.stdout.bin"),
    );
}

#[test]
fn html_formats_match_the_captured_markup() {
    expect(
        RAWXML,
        "htable",
        include_str!("fixtures/format-reference/format-htable.stdout.bin"),
    );
    expect(
        RAWXML,
        "hform",
        include_str!("fixtures/format-reference/format-hform.stdout.bin"),
    );
}

#[test]
fn html_array_cells_preserve_msxml_indentation() {
    for (property, expected) in [
        (
            "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"><VALUE.ARRAY><VALUE>x &amp; y</VALUE><VALUE/></VALUE.ARRAY></PROPERTY.ARRAY>",
            "<td>\r\r\n\t{\"x &amp; y\",\"\"}\r\r\n<span style=\"height:1px;overflow-y:hidden\">.</span></td>\r\r\n",
        ),
        (
            "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"uint32\"><VALUE.ARRAY><VALUE>1</VALUE><VALUE>2</VALUE></VALUE.ARRAY></PROPERTY.ARRAY>",
            "<td>\r\r\n\t{1,2}\r\r\n<span style=\"height:1px;overflow-y:hidden\">.</span></td>\r\r\n",
        ),
        (
            "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"><VALUE.ARRAY/></PROPERTY.ARRAY>",
            "<td>\r\r\n\t{}\r\r\n<span style=\"height:1px;overflow-y:hidden\">.</span></td>\r\r\n",
        ),
        (
            "<PROPERTY.ARRAY NAME=\"A\" TYPE=\"string\"/>",
            "<td>\r\r\n\t\r\r\n<span style=\"height:1px;overflow-y:hidden\">.</span></td>\r\r\n",
        ),
        (
            "<PROPERTY NAME=\"A\" TYPE=\"string\"><VALUE>scalar</VALUE></PROPERTY>",
            "<td>scalar<span style=\"height:1px;overflow-y:hidden\">.</span></td>\r\r\n",
        ),
    ] {
        let xml = format!(
            "<COMMAND><RESULTS NODE=\"N\"><CIM><INSTANCE CLASSNAME=\"X\">{property}</INSTANCE></CIM></RESULTS></COMMAND>"
        );
        for name in ["htable", "hform"] {
            let out = render(&xml, name).unwrap().to_string_lossy();
            assert!(out.contains(expected), "{name} {property}: {out:?}");
        }
    }
}

#[test]
fn htable_empty_groups_close_without_an_inner_newline() {
    let xml = "<COMMAND><RESULTS NODE=\"A\"><CIM/></RESULTS><RESULTS NODE=\"B\"><CIM><INSTANCE CLASSNAME=\"X\"/></CIM></RESULTS><RESULTS NODE=\"C\"><CIM/></RESULTS></COMMAND>";
    expect(
        xml,
        "htable",
        "<html>\r\r\n<H3>1 Instances of X</H3>\r\r\n<table border=\"1\">\r\r\n<tr style=\"background-color:#a0a0ff;font:10pt Tahoma;font-weight:bold;\" align=\"left\">\r\r\n<td>Node</td>\r\r\n</tr>\r\r\n<tr style=\"background-color:#e0f0f0;font:10pt Tahoma;\"></tr>\r\r\n<tr style=\"background-color:#f0f0f0;font:10pt Tahoma;\">\r\r\n<tr style=\"background-color:#f0f0f0;font:10pt Tahoma;\">\r\r\n<td align=\"left\">B</td>\r\r\n</tr>\r\r\n</tr>\r\r\n<tr style=\"background-color:#e0f0f0;font:10pt Tahoma;\"></tr>\r\r\n</table>\r\r\n</html>\r\r\n\r\r\n",
    );
}

#[test]
fn hform_empty_groups_close_without_an_inner_newline() {
    expect(
        "<COMMAND><RESULTS NODE=\"A\"><CIM/></RESULTS><RESULTS NODE=\"B\"><CIM/></RESULTS></COMMAND>",
        "hform",
        "<html>\r\r\n<H3>Node: A - 0 Instances of </H3>\r\r\n<table border=\"1\"></table>\r\r\n<H3>Node: B - 0 Instances of </H3>\r\r\n<table border=\"1\"></table>\r\r\n</html>\r\r\n\r\r\n",
    );
}

#[test]
fn xml_view_matches_the_captured_markup() {
    expect(
        &FORMAT_RAWXML.replace("/format:rawxml", "/format:xml"),
        "xml",
        include_str!("fixtures/format-reference/format-xml.stdout.bin"),
    );
    expect(
        &TYPES_RAWXML.replace("/format:rawxml", "/format:xml"),
        "xml",
        include_str!("fixtures/format-reference/format-types-xml.stdout.bin"),
    );
}

#[test]
fn array_and_empty_properties_match_the_captured_formats() {
    expect(
        TYPES_RAWXML,
        "csv",
        include_str!("fixtures/format-reference/format-types-csv.stdout.bin"),
    );
    expect(
        TYPES_RAWXML,
        "mof",
        include_str!("fixtures/format-reference/format-types-mof.stdout.bin"),
    );
}

#[test]
fn every_value_alias_matches_its_captured_output() {
    for (names, golden) in [
        (
            &["value", "wmiclivalueformat", "textvaluelist", "list"][..],
            include_str!("fixtures/format-reference/format-value.stdout.bin"),
        ),
        (
            &["texttable", "texttablewsys", "wmiclitableformat", "table"][..],
            include_str!("fixtures/format-reference/format-texttable.stdout.bin"),
        ),
    ] {
        for name in names {
            // REA pass 19/followups 13-14 distinguish bare aliases from .xsl filenames.
            for variant in [name.to_string(), name.to_uppercase()] {
                expect(FORMAT_RAWXML, &variant, golden);
            }
        }
    }
    expect(
        RAWXML,
        "wmiclimofformat",
        include_str!("fixtures/reference/os-get-mof.stdout.bin"),
    );
    expect(
        RAWXML,
        "htable-sortby",
        include_str!("fixtures/format-reference/format-htable.stdout.bin"),
    );
}

#[test]
fn reference_properties_follow_their_structured_paths() {
    expect(
        REFERENCE_RAWXML,
        "table",
        include_str!("fixtures/format-reference/reference-table.stdout.bin"),
    );
    expect(
        REFERENCE_RAWXML,
        "csv",
        include_str!("fixtures/format-reference/reference-csv.stdout.bin"),
    );
}

#[test]
fn embedded_objects_are_visited_as_instances() {
    expect(
        OBJECT_RAWXML,
        "table",
        include_str!("fixtures/format-reference/object-table.stdout.bin"),
    );
    expect(
        OBJECT_RAWXML,
        "csv",
        include_str!("fixtures/format-reference/object-csv.stdout.bin"),
    );
}

#[test]
fn norefcomma_replaces_the_reference_separator() {
    let out = transform_builtin(
        &wide(REFERENCE_RAWXML),
        &spec(&[("csv", &[("norefcomma", "true")])]),
    )
    .unwrap();
    assert!(
        out.stdout
            .to_string_lossy()
            .contains("Name=\"DESKTOP-N4H9E7R\" \",\"")
    );
    let other = transform_builtin(
        &wide(REFERENCE_RAWXML),
        &spec(&[("csv", &[("norefcomma", "false")])]),
    )
    .unwrap();
    assert_eq!(
        other.stdout,
        render(REFERENCE_RAWXML, "csv").unwrap(),
        "any value except true keeps the comma"
    );
}

const SORTABLE: &str = "<COMMAND><RESULTS NODE=\"H\"><CIM>\
<INSTANCE CLASSNAME=\"C\"><PROPERTY NAME=\"N\" TYPE=\"string\"><VALUE>b</VALUE></PROPERTY><PROPERTY NAME=\"S\" TYPE=\"uint32\"><VALUE>10</VALUE></PROPERTY></INSTANCE>\
<INSTANCE CLASSNAME=\"C\"><PROPERTY NAME=\"N\" TYPE=\"string\"><VALUE>a</VALUE></PROPERTY><PROPERTY NAME=\"S\" TYPE=\"uint32\"><VALUE>9</VALUE></PROPERTY></INSTANCE>\
<INSTANCE CLASSNAME=\"C\"><PROPERTY NAME=\"N\" TYPE=\"string\"><VALUE>c</VALUE></PROPERTY><PROPERTY NAME=\"S\" TYPE=\"uint32\"><VALUE>100</VALUE></PROPERTY></INSTANCE>\
</CIM></RESULTS></COMMAND>";

fn first_column(html: &str) -> Vec<&str> {
    html.split("<td>")
        .skip(1)
        .filter_map(|cell| cell.split('<').next())
        .filter(|cell| matches!(*cell, "a" | "b" | "c"))
        .collect()
}

#[test]
fn htable_sorts_by_text_or_number() {
    let sorted = |parameters: &[(&str, &str)]| {
        let out = transform_builtin(&wide(SORTABLE), &spec(&[("htable", parameters)]))
            .unwrap()
            .stdout
            .to_string_lossy();
        first_column(&out).join("")
    };
    assert_eq!(sorted(&[]), "bac");
    assert_eq!(sorted(&[("sortby", "N")]), "abc");
    assert_eq!(
        sorted(&[("sortby", "S")]),
        "bca",
        "text order: 10 < 100 < 9"
    );
    assert_eq!(sorted(&[("sortby", "S"), ("datatype", "number")]), "abc");
    assert_eq!(sorted(&[("sortby", "Missing")]), "bac");
}

#[test]
fn a_chain_stage_that_receives_text_fails_like_the_xsl_pipeline() {
    for chain in [["csv", "rawxml"], ["csv", "list"], ["table", "csv"]] {
        let err = transform_builtin(&wide(RAWXML), &spec(&[(chain[0], &[]), (chain[1], &[])]))
            .unwrap_err();
        assert_eq!(err.code, 0xACB5, "{chain:?}");
        assert_eq!(err.message, wide("Invalid XML content."));
    }
}

#[test]
fn xml_producing_stages_chain() {
    let direct = render(RAWXML, "csv").unwrap();
    let chained = transform_builtin(&wide(RAWXML), &spec(&[("rawxml", &[]), ("csv", &[])]))
        .unwrap()
        .stdout;
    assert_eq!(chained, direct);
    let twice = transform_builtin(&wide(RAWXML), &spec(&[("rawxml", &[]), ("rawxml", &[])]))
        .unwrap()
        .stdout;
    assert_eq!(twice, render(RAWXML, "rawxml").unwrap());
}

#[test]
fn every_stylesheet_is_resolved_before_any_stage_runs() {
    let err = transform_builtin(
        &wide("not xml at all"),
        &spec(&[("csv", &[]), ("WmicCompatMissingStylesheet=42", &[])]),
    )
    .unwrap_err();
    assert_eq!(err.code, 0xACB2);
    assert_eq!(err.message, wide("Invalid XSL format (or) file name."));
}

#[test]
fn text_that_is_not_one_xml_document_is_rejected() {
    for bad in [
        "",
        "plain text",
        "<a>",
        "<a></b>",
        "<a/><b/>",
        "<a b=1/>",
        "<a b=\"1\" b=\"2\"/>",
        "<!DOCTYPE a><a/>",
        "<a>&unknown;</a>",
        "<a>\u{1}</a>",
        "<a><!-- -- --></a>",
        "text<a/>",
    ] {
        let err = render(bad, "rawxml").unwrap_err();
        assert_eq!(err.code, 0xACB5, "{bad:?}");
    }
}

#[test]
fn table_limits_fail_the_stylesheet() {
    let rows = |n: usize| {
        format!(
            "<COMMAND><RESULTS NODE=\"H\"><CIM>{}</CIM></RESULTS></COMMAND>",
            "<INSTANCE CLASSNAME=\"C\"></INSTANCE>".repeat(n)
        )
    };
    assert!(render(&rows(2049), "table").is_ok());
    let err = render(&rows(2050), "table").unwrap_err();
    assert_eq!(err.code, 0xACB2);
    let columns = format!(
        "<COMMAND><RESULTS NODE=\"H\"><CIM><INSTANCE CLASSNAME=\"C\">{}</INSTANCE></CIM></RESULTS></COMMAND>",
        "<PROPERTY NAME=\"P\" TYPE=\"string\"></PROPERTY>".repeat(130)
    );
    assert_eq!(render(&columns, "table").unwrap_err().code, 0xACB2);
}

const TRANSLATABLE: &str = "<COMMAND><REQUEST><COMMANDLINE>a,b&lt;tag&gt;</COMMANDLINE></REQUEST><RESULTS NODE=\"HOST\"><CIM><INSTANCE CLASSNAME=\"Sample\"><PROPERTY NAME=\"Value\" TYPE=\"string\"><VALUE>a,b&amp; &lt;tag&gt;</VALUE></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>";

fn table(name: &str, entries: &[(&str, &str)]) -> TranslationTable {
    TranslationTable {
        name: wide(name),
        entries: entries.iter().map(|(a, b)| (wide(a), wide(b))).collect(),
    }
}

#[test]
fn before_tables_change_decoded_values_and_keep_the_xml_valid() {
    let mut spec = spec(&[("csv", &[])]);
    spec.before = vec![table("BasicXml", &[("&lt;", "<"), ("&gt;", ">")])];
    assert_eq!(
        transform_builtin(&wide(TRANSLATABLE), &spec)
            .unwrap()
            .stdout,
        wide("\r\r\nNode,Value\r\r\nHOST,a,b&amp; &lt;tag&gt;\r\r\n")
    );
    spec.before = vec![table("Comma", &[(",", " "), ("<", "(")])];
    assert_eq!(
        transform_builtin(&wide(TRANSLATABLE), &spec)
            .unwrap()
            .stdout,
        wide("\r\r\nNode,Value\r\r\nHOST,a b&amp; (tag&gt;\r\r\n")
    );
}

#[test]
fn before_tables_run_in_order_and_skip_attributes_and_request_text() {
    let mut spec = spec(&[("rawxml", &[])]);
    spec.before = vec![table("One", &[("a", "b")]), table("Two", &[("b", "c")])];
    let out = transform_builtin(&wide(TRANSLATABLE), &spec)
        .unwrap()
        .stdout
        .to_string_lossy();
    assert!(out.contains("<VALUE>c,c&amp; &lt;tcg&gt;</VALUE>"), "{out}");
    assert!(out.contains("<COMMANDLINE>a,b&lt;tag&gt;</COMMANDLINE>"));
    assert!(out.contains("CLASSNAME=\"Sample\""));
}

#[test]
fn after_tables_replace_text_in_the_final_output() {
    let mut spec = spec(&[("csv", &[])]);
    spec.after = vec![table("NoComma", &[(",", " ")])];
    assert_eq!(
        transform_builtin(&wide(RAWXML), &spec).unwrap().stdout,
        wide(
            "\r\r\nNode Caption Version\r\r\nDESKTOP-N4H9E7R Microsoft Windows 11 Pro 10.0.26200\r\r\n"
        )
    );
}

#[test]
fn before_tables_fail_on_text_that_is_not_xml() {
    let mut spec = spec(&[("csv", &[])]);
    spec.before = vec![table("T", &[("a", "b")])];
    assert_eq!(
        transform_builtin(&wide("plain"), &spec).unwrap_err().code,
        0xACB5
    );
}

#[test]
fn finish_text_matches_the_rendered_line_endings() {
    let out = render("<COMMAND/>", "rawxml").unwrap();
    assert_eq!(out, finish_text(&wide("<COMMAND></COMMAND>\r\n")));
}

mod session_rendering {
    use super::*;
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        parse::parse_command,
        render::{DestinationProfile, RenderRequest, compile_format, encode_output, render},
        session::SessionSettings,
    };

    fn property(name: &str, cim_type: u32, value: CimValue) -> Property {
        Property {
            name: wide(name),
            value,
            cim_type,
            system: name.starts_with("__"),
            qualifiers: vec![],
        }
    }

    fn operating_system() -> ManagementResult {
        let text = |s: &str| CimValue::String(wide(s));
        ManagementResult::Objects(ObjectSet {
            objects: vec![Record {
                class: wide("Win32_OperatingSystem"),
                server: Some(wide("DESKTOP-N4H9E7R")),
                properties: vec![
                    property("__GENUS", 3, CimValue::Sint32(2)),
                    property("__CLASS", 8, text("Win32_OperatingSystem")),
                    property("__SUPERCLASS", 8, text("CIM_OperatingSystem")),
                    property("__DYNASTY", 8, text("")),
                    property("__RELPATH", 8, text("Win32_OperatingSystem=@")),
                    property("__PROPERTY_COUNT", 3, CimValue::Sint32(0)),
                    property(
                        "__DERIVATION",
                        0x2008,
                        CimValue::Array(vec![
                            text("CIM_OperatingSystem"),
                            text("CIM_LogicalElement"),
                            text("CIM_ManagedSystemElement"),
                        ]),
                    ),
                    property("__SERVER", 8, text("")),
                    property("__NAMESPACE", 8, text("")),
                    property("__PATH", 8, text("")),
                    property("Version", 8, text("10.0.26200")),
                    property("Caption", 8, text("Microsoft Windows 11 Pro")),
                ],
                ..Default::default()
            }],
            schema: None,
        })
    }

    fn output(command: &str) -> Vec<u8> {
        let parsed = parse_command(&wide(command), EntryMode::Interactive).unwrap();
        let ResolvedCommand::Management(mut req) = AliasStore::stock()
            .unwrap()
            .resolve(&parsed, &SessionSettings::default())
            .unwrap()
        else {
            panic!()
        };
        req.render_context = Some(RenderContext {
            issued_from: wide("DESKTOP-N4H9E7R"),
            ..Default::default()
        });
        req.format_spec = Some(compile_format(&req, &[]).unwrap());
        let data = operating_system();
        let out = render(RenderRequest {
            request: &req,
            result: Ok(&data),
            node: &wide("DESKTOP-N4H9E7R"),
        })
        .unwrap();
        encode_output(&out.stdout, &DestinationProfile::pipe(1252)).unwrap()
    }

    #[test]
    fn system_properties_appear_only_when_named() {
        assert_eq!(
            output("os get Caption,Version /format:csv"),
            include_bytes!("fixtures/reference/os-get-csv.stdout.bin")
        );
        assert_eq!(
            output("os get Caption,Version"),
            include_bytes!("fixtures/reference/os-get.stdout.bin")
        );
        assert_eq!(
            output("os get Version,Caption"),
            include_bytes!("fixtures/reference/os-get-reversed.stdout.bin")
        );
        assert_eq!(
            output("os get Caption,Version /value"),
            include_bytes!("fixtures/reference/os-get-value.stdout.bin")
        );
    }

    #[test]
    fn naming_a_system_property_adds_the_whole_system_set() {
        assert_eq!(
            output("os get __DERIVATION /format:csv"),
            include_bytes!("fixtures/format-reference/system-csv.stdout.bin")
        );
    }

    fn text(command: &str) -> String {
        let bytes = output(command);
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn a_wildcard_get_renders_every_non_system_property() {
        for command in ["os get *", "os get * /value", "os get * /format:csv"] {
            let out = text(command);
            assert!(out.contains("Caption"), "{command}: {out:?}");
            assert!(out.contains("Version"), "{command}: {out:?}");
            assert!(
                out.contains("Microsoft Windows 11 Pro"),
                "{command}: {out:?}"
            );
            assert!(!out.contains("__"), "{command}: {out:?}");
        }
    }

    #[test]
    fn a_wildcard_get_adds_the_system_set_only_when_one_is_named() {
        let out = text("os get *,__CLASS /value");
        assert!(out.contains("__CLASS=Win32_OperatingSystem"), "{out:?}");
        assert!(out.contains("__GENUS=2"), "{out:?}");
        assert!(out.contains("Caption="), "{out:?}");
    }
}
