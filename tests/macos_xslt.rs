#![cfg(target_os = "macos")]
//! Stylesheet files that run through the system libxslt.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use wmic_compat::{
    model::*,
    platform::macos::xslt::{prepare, transform},
    render::{finish_text, transform as run_pipeline},
};

const RAWXML: &str = include_str!("fixtures/reference/os-get-rawxml.stdout.bin");
const FORMAT_RAWXML: &str = include_str!("fixtures/format-reference/format-rawxml.stdout.bin");

fn wide(text: &str) -> WideText {
    WideText(text.encode_utf16().collect())
}

struct Sheet(PathBuf);

impl Sheet {
    fn new(body: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wmic-xslt-{}-{}.xsl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, body).unwrap();
        Self(path)
    }

    fn spec(&self, parameters: &[(&str, &str)]) -> TransformSpec {
        spec(self.0.to_str().unwrap(), parameters)
    }
}

impl Drop for Sheet {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn spec(name: &str, parameters: &[(&str, &str)]) -> TransformSpec {
    TransformSpec {
        name: wide(name),
        parameters: parameters.iter().map(|(n, v)| (wide(n), wide(v))).collect(),
    }
}

fn fixture(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .to_str()
        .unwrap()
        .to_string()
}

const ECHO: &str = r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="text" encoding="utf-16" omit-xml-declaration="yes"/>
<xsl:param name="value" select="'unset'"/>
<xsl:template match="/"><xsl:value-of select="$value"/>|<xsl:value-of select="name(/*)"/>|<xsl:value-of select="/*/@a"/></xsl:template>
</xsl:stylesheet>"#;

#[test]
fn parameters_arrive_as_strings() {
    let sheet = Sheet::new(ECHO);
    for value in [
        "hello",
        "it's",
        "say \"hi\"",
        "both ' and \"",
        "1+1",
        "/*",
        "",
        "\u{5b57} \u{1f600}",
    ] {
        let out = transform(&wide("<r a='x'/>"), &sheet.spec(&[("value", value)])).unwrap();
        assert_eq!(out, wide(&format!("{value}|r|x")), "{value:?}");
    }
}

#[test]
fn the_last_parameter_value_and_unknown_names() {
    let sheet = Sheet::new(ECHO);
    let out = transform(
        &wide("<r a='x'/>"),
        &sheet.spec(&[("unused", "1"), ("value", "first"), ("value", "second")]),
    )
    .unwrap();
    assert_eq!(out, wide("second|r|x"));
    let default = transform(&wide("<r a='x'/>"), &sheet.spec(&[])).unwrap();
    assert_eq!(default, wide("unset|r|x"));
}

#[test]
fn repository_parameter_stylesheet_matches_the_captured_text() {
    let path = fixture("tests/fixtures/xsl/parameters.xsl");
    let hello = transform(&wide(FORMAT_RAWXML), &spec(&path, &[("greeting", "hello")])).unwrap();
    assert_eq!(
        hello,
        wide("hello|COMMAND|Win32_OperatingSystem|Microsoft Windows 11 Pro")
    );
    let default = transform(&wide(FORMAT_RAWXML), &spec(&path, &[])).unwrap();
    assert_eq!(
        default,
        wide("default|COMMAND|Win32_OperatingSystem|Microsoft Windows 11 Pro")
    );
}

#[test]
fn script_stylesheets_are_refused_before_parsing() {
    let err = match prepare(&spec(&fixture("tests/fixtures/xsl/script.xsl"), &[])) {
        Ok(_) => panic!("script stylesheet accepted"),
        Err(e) => e,
    };
    assert_eq!(err.code, 0xACB2);
    assert_eq!(
        err.message,
        wide(
            "Invalid XSL format (or) file name. Script extensions are not supported on this platform."
        )
    );
    let default_namespace = Sheet::new(
        r#"<stylesheet version="1.0" xmlns="urn:schemas-microsoft-com:xslt" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><script language="JScript">1</script></stylesheet>"#,
    );
    assert_eq!(
        prepare(&default_namespace.spec(&[])).err().map(|e| e.code),
        Some(0xACB2)
    );
}

#[test]
fn the_vendor_namespace_without_a_script_element_is_accepted() {
    let sheet = Sheet::new(
        r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:m='urn:schemas-microsoft-com:xslt'><xsl:output method="text"/><xsl:template match="/">ok</xsl:template></xsl:stylesheet>"#,
    );
    assert_eq!(
        transform(&wide("<r/>"), &sheet.spec(&[])).unwrap(),
        wide("ok")
    );
}

#[test]
fn text_output_stays_text_when_the_stylesheet_declares_utf16() {
    let sheet = Sheet::new(
        r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:output method="text" encoding="utf-16"/><xsl:template match="/">caf&#233; <xsl:value-of select="/r"/></xsl:template></xsl:stylesheet>"#,
    );
    let out = transform(&wide("<r>\u{5b57}\u{1f600}</r>"), &sheet.spec(&[])).unwrap();
    assert_eq!(out, wide("caf\u{e9} \u{5b57}\u{1f600}"));
}

#[test]
fn failures_use_the_windows_status_codes() {
    let sheet = Sheet::new(ECHO);
    let invalid = transform(&wide("<r>"), &sheet.spec(&[])).unwrap_err();
    assert_eq!(invalid.code, 0xACB5);
    assert_eq!(invalid.message, wide("Invalid XML content."));
    let missing = prepare(&spec("/nonexistent/wmic-missing.xsl", &[]))
        .err()
        .unwrap();
    assert_eq!(missing.code, 0xACB2);
    assert_eq!(missing.message, wide("Invalid XSL format (or) file name."));
    for body in [
        "not xml",
        "<root/>",
        "<xsl:stylesheet xmlns:xsl='http://www.w3.org/1999/XSL/Transform' version='1.0'><xsl:bogus/></xsl:stylesheet>",
        "<!DOCTYPE xsl:stylesheet [<!ENTITY x 'y'>]><xsl:stylesheet xmlns:xsl='http://www.w3.org/1999/XSL/Transform' version='1.0'/>",
    ] {
        let sheet = Sheet::new(body);
        assert_eq!(
            prepare(&sheet.spec(&[])).err().map(|e| e.code),
            Some(0xACB2),
            "{body}"
        );
    }
}

#[test]
fn a_runtime_error_fails_the_stylesheet() {
    let sheet = Sheet::new(
        r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:output method="text"/><xsl:template match="/">before<xsl:value-of select="no-such-function()"/>after</xsl:template></xsl:stylesheet>"#,
    );
    let err = transform(&wide("<r/>"), &sheet.spec(&[])).unwrap_err();
    assert_eq!(err.code, 0xACB2);
}

#[test]
fn the_rawxml_stylesheet_reproduces_the_captured_envelope() {
    let path = fixture("assets/formats/rawxml.xsl");
    let out = transform(&wide(RAWXML), &spec(&path, &[])).unwrap();
    assert_eq!(finish_text(&out), wide(RAWXML));
}

#[test]
fn stylesheet_files_follow_built_in_stages_in_a_chain() {
    let path = fixture("tests/fixtures/xsl/parameters.xsl");
    let chain = |stages: &[(&str, &[(&str, &str)])]| FormatSpec {
        transforms: stages.iter().map(|(n, p)| spec(n, p)).collect(),
        before: vec![],
        after: vec![],
    };
    let out = run_pipeline(
        &wide(FORMAT_RAWXML),
        &chain(&[("rawxml", &[]), (&path, &[("greeting", "hello")])]),
    )
    .unwrap();
    assert_eq!(
        out.stdout,
        wide("hello|COMMAND|Win32_OperatingSystem|Microsoft Windows 11 Pro\r\r\n")
    );
    let text_stage =
        run_pipeline(&wide(FORMAT_RAWXML), &chain(&[("csv", &[]), (&path, &[])])).unwrap_err();
    assert_eq!(text_stage.code, 0xACB5);
    let unresolved = run_pipeline(
        &wide("not xml"),
        &chain(&[("csv", &[]), ("/nonexistent/wmic-missing.xsl", &[])]),
    )
    .unwrap_err();
    assert_eq!(unresolved.code, 0xACB2);
}

#[test]
fn stylesheets_cannot_write_files() {
    let target = std::env::temp_dir().join(format!("wmic-xslt-write-{}.xml", std::process::id()));
    let _ = std::fs::remove_file(&target);
    let sheet = Sheet::new(&format!(
        r#"<xsl:stylesheet version="1.1" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:output method="text"/><xsl:template match="/"><xsl:document href="{}"><written/></xsl:document>done</xsl:template></xsl:stylesheet>"#,
        target.display()
    ));
    let _ = transform(&wide("<r/>"), &sheet.spec(&[]));
    let created = target.exists();
    let _ = std::fs::remove_file(&target);
    assert!(!created, "the stylesheet created {}", target.display());
}

mod secondary_documents {
    use super::*;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    const SECRET: &str = "TOPSECRET";

    struct Fixture {
        listener: TcpListener,
        _secret: Sheet,
        dtd: String,
        entity: String,
    }

    fn fixture_files() -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let secret = Sheet::new(SECRET);
        Fixture {
            listener,
            dtd: format!("http://127.0.0.1:{port}/x.dtd"),
            entity: format!("file://{}", secret.0.display()),
            _secret: secret,
        }
    }

    fn nobody_connected(listener: &TcpListener) -> bool {
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok(_) => return false,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(_) => return true,
            }
        }
        true
    }

    fn document_sheet(path: &std::path::Path) -> Sheet {
        Sheet::new(&format!(
            r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:output method="text"/><xsl:template match="/">[<xsl:value-of select="document('{}')"/>]</xsl:template></xsl:stylesheet>"#,
            path.display()
        ))
    }

    fn include_sheet(path: &std::path::Path) -> Sheet {
        Sheet::new(&format!(
            r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:include href="{}"/><xsl:output method="text"/><xsl:template match="/">[<xsl:call-template name="t"/>]</xsl:template></xsl:stylesheet>"#,
            path.display()
        ))
    }

    fn included(doctype: &str, body: &str) -> Sheet {
        Sheet::new(&format!(
            r#"{doctype}<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template name="t">{body}</xsl:template></xsl:stylesheet>"#
        ))
    }

    #[test]
    fn document_does_not_load_a_network_dtd() {
        let f = fixture_files();
        let data = Sheet::new(&format!(r#"<!DOCTYPE r SYSTEM "{}"><r>visible</r>"#, f.dtd));
        let sheet = document_sheet(&data.0);
        let out = transform(&wide("<r/>"), &sheet.spec(&[]));
        assert!(nobody_connected(&f.listener));
        assert_eq!(out.unwrap(), wide("[visible]"));
    }

    #[test]
    fn document_does_not_expand_an_external_entity() {
        let f = fixture_files();
        let data = Sheet::new(&format!(
            r#"<!DOCTYPE r [<!ENTITY e SYSTEM "{}">]><r>a&e;b</r>"#,
            f.entity
        ));
        let sheet = document_sheet(&data.0);
        let out = transform(&wide("<r/>"), &sheet.spec(&[]));
        if let Ok(text) = out {
            assert!(!text.to_string_lossy().contains(SECRET), "{text:?}");
        }
    }

    #[test]
    fn document_does_not_expand_an_external_entity_through_a_network_dtd() {
        let f = fixture_files();
        let data = Sheet::new(&format!(
            r#"<!DOCTYPE r SYSTEM "{}" [<!ENTITY e SYSTEM "{}">]><r>&e;</r>"#,
            f.dtd, f.entity
        ));
        let sheet = document_sheet(&data.0);
        let out = transform(&wide("<r/>"), &sheet.spec(&[]));
        assert!(nobody_connected(&f.listener));
        if let Ok(text) = out {
            assert!(!text.to_string_lossy().contains(SECRET), "{text:?}");
        }
    }

    #[test]
    fn include_does_not_load_a_network_dtd() {
        let f = fixture_files();
        let part = included(
            &format!(r#"<!DOCTYPE xsl:stylesheet SYSTEM "{}">"#, f.dtd),
            "visible",
        );
        let sheet = include_sheet(&part.0);
        let out = transform(&wide("<r/>"), &sheet.spec(&[]));
        assert!(nobody_connected(&f.listener));
        assert_eq!(out.unwrap(), wide("[visible]"));
    }

    #[test]
    fn include_does_not_expand_an_external_entity() {
        let f = fixture_files();
        let part = included(
            &format!(
                r#"<!DOCTYPE xsl:stylesheet [<!ENTITY e SYSTEM "{}">]>"#,
                f.entity
            ),
            "<xsl:text>a&e;b</xsl:text>",
        );
        let sheet = include_sheet(&part.0);
        let out = transform(&wide("<r/>"), &sheet.spec(&[]));
        if let Ok(text) = out {
            assert!(!text.to_string_lossy().contains(SECRET), "{text:?}");
        }
    }

    #[test]
    fn import_does_not_load_a_network_dtd() {
        let f = fixture_files();
        let part = included(
            &format!(r#"<!DOCTYPE xsl:stylesheet SYSTEM "{}">"#, f.dtd),
            "visible",
        );
        let sheet = Sheet::new(&format!(
            r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:import href="{}"/><xsl:output method="text"/><xsl:template match="/">[<xsl:call-template name="t"/>]</xsl:template></xsl:stylesheet>"#,
            part.0.display()
        ));
        let out = transform(&wide("<r/>"), &sheet.spec(&[]));
        assert!(nobody_connected(&f.listener));
        assert_eq!(out.unwrap(), wide("[visible]"));
    }
}
