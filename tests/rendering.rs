mod support;
use support::wide;
use wmic_compat::{
    alias::{AliasStore, ResolvedCommand},
    backend::ResolvedRequest,
    model::*,
    parse::parse_command,
    render::{DestinationProfile, RenderRequest, encode_output, render},
    session::SessionSettings,
};
fn request(s: &str) -> ResolvedRequest {
    let ResolvedCommand::Management(r) = AliasStore::stock()
        .unwrap()
        .resolve(
            &parse_command(&wide(s), EntryMode::Interactive).unwrap(),
            &SessionSettings::default(),
        )
        .unwrap()
    else {
        panic!()
    };
    *r
}
fn property(name: &str, value: CimValue, ty: u32) -> Property {
    Property {
        name: wide(name),
        value,
        cim_type: ty,
        system: false,
        qualifiers: vec![],
    }
}
fn result(props: Vec<Property>) -> ManagementResult {
    ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: wide("Win32_OperatingSystem"),
            server: Some(wide("DESKTOP-N4H9E7R")),
            properties: props,
            ..Default::default()
        }],
        schema: None,
    })
}
fn bytes(s: &WideText) -> Vec<u8> {
    encode_output(s, &DestinationProfile::pipe(1252)).unwrap()
}
#[test]
fn reversed_get_properties_match_reference() {
    let q = request("os get Version,Caption");
    let r = result(vec![
        property("Version", CimValue::String(wide("10.0.26200")), 8),
        property(
            "Caption",
            CimValue::String(wide("Microsoft Windows 11 Pro")),
            8,
        ),
    ]);
    let output = render(RenderRequest {
        request: &q,
        result: Ok(&r),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        bytes(&output.stdout),
        include_bytes!("fixtures/reference/os-get-reversed.stdout.bin")
    );
    assert!(output.stderr.0.is_empty());
}
#[test]
fn empty_query_matches_reference() {
    let q = request("os where Caption='missing' get Caption");
    let r = ManagementResult::Objects(ObjectSet::default());
    let o = render(RenderRequest {
        request: &q,
        result: Ok(&r),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        bytes(&o.stdout),
        include_bytes!("fixtures/reference/os-empty.stdout.bin")
    );
    assert_eq!(
        bytes(&o.stderr),
        include_bytes!("fixtures/reference/os-empty.stderr.bin")
    );
}
#[test]
fn array_null_and_boolean_rendering() {
    for (mode, file) in [(" /value", "types-value"), ("", "types-table")] {
        let q = request(&format!(
            "os get MUILanguages,Primary,OtherTypeDescription{mode}"
        ));
        let r = result(vec![
            property(
                "MUILanguages",
                CimValue::Array(vec![CimValue::String(wide("en-US"))]),
                0x2008,
            ),
            property("Primary", CimValue::Boolean(true), 11),
            property("OtherTypeDescription", CimValue::Null, 8),
        ]);
        let o = render(RenderRequest {
            request: &q,
            result: Ok(&r),
            node: &wide("DESKTOP-N4H9E7R"),
        })
        .unwrap();
        assert_eq!(
            bytes(&o.stdout),
            std::fs::read(format!("tests/fixtures/type-reference/{file}.stdout.bin")).unwrap()
        );
    }
}
#[test]
fn invalid_property_diagnostic_matches_reference() {
    let q = request("os get WmicResearchPropertyDoesNotExist");
    let e = ManagementError {
        code: 0x80041017,
        target: wide("DESKTOP-N4H9E7R"),
        operation: wide("query"),
        message: wide("Invalid query"),
        extended: vec![],
        partial: None,
    };
    let o = render(RenderRequest {
        request: &q,
        result: Err(&e),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        bytes(&o.stdout),
        include_bytes!("fixtures/reference/error-property.stdout.bin")
    );
    assert_eq!(
        bytes(&o.stderr),
        include_bytes!("fixtures/reference/error-property.stderr.bin")
    );
}
#[test]
fn value_mode_preserves_blank_lines() {
    let q = request("os get Caption,Version /value");
    let r = result(vec![
        property(
            "Caption",
            CimValue::String(wide("Microsoft Windows 11 Pro")),
            8,
        ),
        property("Version", CimValue::String(wide("10.0.26200")), 8),
    ]);
    let o = render(RenderRequest {
        request: &q,
        result: Ok(&r),
        node: &wide("DESKTOP-N4H9E7R"),
    })
    .unwrap();
    assert_eq!(
        bytes(&o.stdout),
        include_bytes!("fixtures/reference/os-get-value.stdout.bin")
    );
}
