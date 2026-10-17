//! Provider-free regressions from REA passes 19-21 and followups 13-17.
mod support;

use support::{request, wide};
use wmic_compat::{
    model::*,
    render::{
        DestinationProfile, RenderError, RenderRequest, builtin_source, compile_format,
        encode_output, render,
    },
};

fn output(command: &str) -> Result<Vec<u8>, RenderError> {
    let mut request = request(command);
    request.render_context = Some(RenderContext {
        issued_from: wide("DESKTOP-N4H9E7R"),
        ..Default::default()
    });
    request.format_spec = Some(compile_format(&request, &[])?);
    let result = ManagementResult::Objects(ObjectSet {
        objects: vec![Record {
            class: wide("Win32_OperatingSystem"),
            server: Some(wide("DESKTOP-N4H9E7R")),
            properties: vec![Property {
                name: wide("Caption"),
                value: CimValue::String(wide("Microsoft Windows 11 Pro")),
                cim_type: 8,
                system: false,
                qualifiers: vec![],
            }],
            ..Default::default()
        }],
        schema: None,
    });
    let rendered = render(RenderRequest {
        request: &request,
        result: Ok(&result),
        node: &wide("DESKTOP-N4H9E7R"),
    })?;
    Ok(encode_output(&rendered.stdout, &DestinationProfile::pipe(1252)).unwrap())
}

#[test]
fn cli_format_aliases_ignore_character_width() {
    assert_eq!(
        output("os get Caption /format:ＣＳＶ").unwrap(),
        b"\r\r\nNode,Caption\r\r\nDESKTOP-N4H9E7R,Microsoft Windows 11 Pro\r\r\n"
    );
}

#[test]
fn bare_format_keeps_the_default_table() {
    assert_eq!(
        output("os get Caption /format").unwrap(),
        b"Caption                   \r\r\nMicrosoft Windows 11 Pro  \r\r\n\r\r\n"
    );
}

#[test]
fn empty_format_reports_an_invalid_stylesheet() {
    let error = output("os get Caption /format:").unwrap_err();
    assert_eq!(error.code, 0xACB2);
    assert_eq!(error.message, wide("Invalid XSL format (or) file name."));
}

#[test]
fn stylesheet_filenames_do_not_resolve_as_builtin_aliases() {
    for name in ["csv.xsl", "CSV.XSL"] {
        let error = output(&format!("os get Caption /format:{name}")).unwrap_err();
        assert_eq!(error.code, 0xACB2, "{name}");
        assert_eq!(error.message, wide("Invalid XSL format (or) file name."));
    }
    // Internal MSXML tests and resource consumers still use literal resource names.
    assert!(builtin_source(&wide("csv.xsl")).is_some());
}

#[test]
fn empty_translation_reports_an_invalid_switch() {
    let error = output("os get Caption /translate:").unwrap_err();
    assert_eq!(error.code, 0xABEC);
    assert_eq!(error.message, wide("Invalid TRANSLATE switch."));
}

#[test]
fn parameter_colons_accept_separating_whitespace() {
    assert_eq!(
        output("os get Caption /format:csv : \"unused=x\"").unwrap(),
        output("os get Caption /format:csv").unwrap()
    );
}

#[test]
fn a_quoted_parameter_comma_still_terminates_the_native_parameter_token() {
    use wmic_compat::parse::parse_format_spec;
    for token in [
        "unused=a,b",
        "unused=a,b,c",
        "unused=a,",
        "unused=a=b,c",
        "unused=a\\,b",
    ] {
        let error = parse_format_spec(&format!("csv:\"{token}\"").into()).unwrap_err();
        assert_eq!(error.code, 0x80004005, "{token}");
        assert_eq!(
            error.message,
            wide("ERROR:\r\r\nDescription = Unspecified error")
        );
    }
    for token in ["unused=,b", "unused==,b"] {
        assert_eq!(
            parse_format_spec(&format!("csv:\"{token}\"").into())
                .unwrap_err()
                .code,
            0xABE5
        );
    }
}

#[test]
fn translation_table_values_remain_sensitive_to_character_width() {
    let tables = [(wide("BasicXml"), vec![(wide("&lt;"), wide("<"))])];
    assert!(compile_format(&request("os get Caption /translate:basicxml"), &tables).is_ok());
    let error = compile_format(
        &request("os get Caption /translate:ＢａｓｉｃＸｍｌ"),
        &tables,
    )
    .unwrap_err();
    assert_eq!(error.code, 0xABED);
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn existing_stylesheet_files_remain_available() {
    let directory = std::env::temp_dir().join(format!("wmic-rea-format-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let stylesheet = directory.join("csv.xsl");
    std::fs::write(
        &stylesheet,
        r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:output method="text"/><xsl:template match="/">external stylesheet</xsl:template></xsl:stylesheet>"#,
    )
    .unwrap();
    let actual = output(&format!(
        "os get Caption /format:\"{}\"",
        stylesheet.display()
    ));
    std::fs::remove_file(&stylesheet).unwrap();
    std::fs::remove_dir(&directory).unwrap();
    assert_eq!(actual.unwrap(), b"external stylesheet\r\r\n");
}
