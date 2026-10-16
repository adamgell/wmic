//! Native MSXML 6 transformation; no shell, .NET, or Microsoft WMIC resources.
use crate::{
    model::{TransformSpec, WideText},
    render::RenderError,
};
use windows::{
    Win32::{
        Data::Xml::MsXml::*,
        Foundation::VARIANT_BOOL,
        System::{Com::*, Variant::VARIANT},
    },
    core::*,
};
fn bstr(text: &WideText) -> BSTR {
    BSTR::from_wide(&text.0)
}
fn format_error() -> RenderError {
    RenderError::new("Invalid XSL format (or) file name.")
}
struct Apartment(bool);
impl Apartment {
    fn new() -> Result<Self> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.0 == 0x80010106u32 as i32 {
                Ok(Self(false))
            } else {
                hr.ok()?;
                Ok(Self(true))
            }
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() }
        }
    }
}
pub fn transform(
    xml: &WideText,
    spec: &TransformSpec,
) -> std::result::Result<WideText, RenderError> {
    prepare(spec)?.apply(xml)
}
pub fn transform_source(
    xml: &WideText,
    source: &WideText,
    parameters: &[(WideText, WideText)],
) -> std::result::Result<WideText, RenderError> {
    prepare_source(source, parameters)?.apply(xml)
}
/// A processor and its COM initialization stay on their creating thread.
pub struct Prepared {
    processor: IXSLProcessor,
    _apartment: Apartment,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub fn prepare(spec: &TransformSpec) -> std::result::Result<Prepared, RenderError> {
    prepare_style(Stylesheet::File(&spec.name), &spec.parameters)
}
pub fn prepare_source(
    source: &WideText,
    parameters: &[(WideText, WideText)],
) -> std::result::Result<Prepared, RenderError> {
    prepare_style(Stylesheet::Source(source), parameters)
}
enum Stylesheet<'a> {
    File(&'a WideText),
    Source(&'a WideText),
}
fn prepare_style(
    stylesheet: Stylesheet<'_>,
    parameters: &[(WideText, WideText)],
) -> std::result::Result<Prepared, RenderError> {
    let apartment = Apartment::new().map_err(|_| format_error())?;
    unsafe {
        let style: IXMLDOMDocument2 =
            CoCreateInstance(&FreeThreadedDOMDocument60, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| format_error())?;
        style
            .Setasync(VARIANT_BOOL(0))
            .map_err(|_| format_error())?;
        style
            .setProperty(&BSTR::from("AllowXsltScript"), &VARIANT::from(true))
            .map_err(|_| format_error())?;
        let loaded = match stylesheet {
            Stylesheet::File(path) => style.load(&VARIANT::from(bstr(path))),
            Stylesheet::Source(source) => style.loadXML(&bstr(source)),
        }
        .map_err(|_| format_error())?;
        if !loaded.as_bool() {
            return Err(format_error());
        }
        let template: IXSLTemplate = CoCreateInstance(&XSLTemplate60, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| format_error())?;
        template
            .putref_stylesheet(&style)
            .map_err(|_| format_error())?;
        let processor = template.createProcessor().map_err(|_| format_error())?;
        for (name, value) in parameters {
            processor
                .addParameter(&bstr(name), &VARIANT::from(bstr(value)), &BSTR::new())
                .map_err(|_| format_error())?;
        }
        Ok(Prepared {
            processor,
            _apartment: apartment,
            _thread: std::marker::PhantomData,
        })
    }
}
impl Prepared {
    pub fn apply(&self, xml: &WideText) -> std::result::Result<WideText, RenderError> {
        unsafe {
            let input: IXMLDOMDocument2 =
                CoCreateInstance(&FreeThreadedDOMDocument60, None, CLSCTX_INPROC_SERVER)
                    .map_err(|_| format_error())?;
            input
                .Setasync(VARIANT_BOOL(0))
                .map_err(|_| format_error())?;
            if !input
                .loadXML(&bstr(xml))
                .map_err(|_| RenderError::status(0xACB5, "Invalid XML content."))?
                .as_bool()
            {
                return Err(RenderError::status(0xACB5, "Invalid XML content."));
            }
            self.processor
                .Setinput(&VARIANT::from(
                    input.cast::<IDispatch>().map_err(|_| format_error())?,
                ))
                .map_err(|_| format_error())?;
            if !self
                .processor
                .transform()
                .map_err(|_| format_error())?
                .as_bool()
            {
                return Err(format_error());
            }
            let value = self.processor.output().map_err(|_| format_error())?;
            if value.vt() != windows::Win32::System::Variant::VT_BSTR {
                return Err(format_error());
            }
            Ok(WideText(
                value.Anonymous.Anonymous.Anonymous.bstrVal.to_vec(),
            ))
        }
    }
}
#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6 and repository fixture share"]
    fn custom_xsl_receives_parameters_and_full_command_root() {
        let spec=TransformSpec{name:r"\\Mac\Home\Documents\ChatGPT\wmic-dropinrepalcement\tests\fixtures\xsl\parameters.xsl".into(),parameters:vec![("greeting".into(),"hello".into())]};
        let text=transform(&"<COMMAND><CIM><INSTANCE CLASSNAME=\"Sample\"><PROPERTY NAME=\"Caption\"><VALUE>test</VALUE></PROPERTY></INSTANCE></CIM></COMMAND>".into(),&spec).unwrap();
        assert_eq!(text, WideText::from("hello|COMMAND|Sample|test"));
    }
}

#[cfg(test)]
mod script_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6 and harmless fixture script"]
    fn script_transform_matches_original_wmic_capability() {
        let spec = TransformSpec {
            name:
                r"\\Mac\Home\Documents\ChatGPT\wmic-dropinrepalcement\tests\fixtures\xsl\script.xsl"
                    .into(),
            parameters: vec![],
        };
        assert_eq!(
            transform(&"<COMMAND/>".into(), &spec).unwrap(),
            WideText::from("script-ok")
        );
    }
}

#[cfg(test)]
mod builtin_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6 and own rawxml stylesheet"]
    fn rawxml_style_matches_original_envelope_golden() {
        let source = include_str!("../../../tests/fixtures/reference/os-get-rawxml.stdout.bin");
        let spec = TransformSpec {
            name: r"\\Mac\Home\Documents\ChatGPT\wmic-dropinrepalcement\assets\formats\rawxml.xsl"
                .into(),
            parameters: vec![],
        };
        let text = transform(&WideText::from(source), &spec).unwrap();
        let mut final_text = text.0;
        final_text.extend("\r\n".encode_utf16());
        let final_text: Vec<_> = final_text
            .into_iter()
            .flat_map(|u| if u == 10 { vec![13, 10] } else { vec![u] })
            .collect();
        let expected: Vec<_> = source.encode_utf16().collect();
        let diff = final_text
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or(final_text.len().min(expected.len()));
        assert_eq!(final_text, expected, "first difference {diff}");
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6"]
    fn embedded_stylesheet_has_no_installed_file_dependency() {
        let style: WideText = include_str!("../../../assets/formats/rawxml.xsl").into();
        let output = transform_source(
            &"<ROOT A=\"x\"><VALUE>data</VALUE></ROOT>".into(),
            &style,
            &[],
        )
        .unwrap();
        assert_eq!(
            output,
            WideText::from("<ROOT A=\"x\">\r\n<VALUE>data</VALUE>\r\n</ROOT>\r\n")
        );
    }
}

#[cfg(test)]
mod renderer_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6"]
    fn embedded_csv_table_and_list_match_reference_goldens() {
        let input: WideText =
            include_str!("../../../tests/fixtures/reference/os-get-rawxml.stdout.bin").into();
        for (mode, expected) in [
            (
                "csv",
                include_str!("../../../tests/fixtures/reference/os-get-csv.stdout.bin"),
            ),
            (
                "table",
                include_str!("../../../tests/fixtures/reference/os-get.stdout.bin"),
            ),
            (
                "list",
                include_str!("../../../tests/fixtures/reference/os-get-list.stdout.bin"),
            ),
        ] {
            let source = crate::render::builtin_source(&mode.into()).unwrap();
            let output = transform_source(&input, &source, &[]).unwrap();
            let mut text = output.0;
            text.extend("\r\n".encode_utf16());
            let text: Vec<_> = text
                .into_iter()
                .flat_map(|u| if u == 10 { vec![13, 10] } else { vec![u] })
                .collect();
            assert_eq!(text, expected.encode_utf16().collect::<Vec<_>>(), "{mode}");
        }
    }
}

#[cfg(test)]
mod translation_tests {
    use super::*;
    use crate::model::{FormatSpec, TranslationTable};
    #[test]
    #[ignore = "requires Windows MSXML6"]
    fn pre_translation_operates_on_decoded_values_without_breaking_xml() {
        let source:WideText="<COMMAND><REQUEST><COMMANDLINE>a,b&lt;tag&gt;</COMMANDLINE></REQUEST><RESULTS NODE=\"HOST\"><CIM><INSTANCE CLASSNAME=\"Sample\"><PROPERTY NAME=\"Value\" TYPE=\"string\"><VALUE>a,b&amp; &lt;tag&gt;</VALUE></PROPERTY></INSTANCE></CIM></RESULTS></COMMAND>".into();
        let spec = FormatSpec {
            transforms: vec![TransformSpec {
                name: "csv".into(),
                parameters: vec![],
            }],
            before: vec![TranslationTable {
                name: "BasicXml".into(),
                entries: vec![("&lt;".into(), "<".into()), ("&gt;".into(), ">".into())],
            }],
            after: vec![],
        };
        assert_eq!(
            crate::render::transform(&source, &spec).unwrap().stdout,
            WideText::from("\r\r\nNode,Value\r\r\nHOST,a,b&amp; &lt;tag&gt;\r\r\n")
        );
    }
}

pub fn translate_values(
    xml: &WideText,
    tables: &[crate::model::TranslationTable],
) -> std::result::Result<WideText, RenderError> {
    if tables.is_empty() {
        return Ok(xml.clone());
    }
    let _apartment = Apartment::new().map_err(|_| format_error())?;
    unsafe {
        let doc: IXMLDOMDocument2 =
            CoCreateInstance(&FreeThreadedDOMDocument60, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| format_error())?;
        if !doc
            .loadXML(&bstr(xml))
            .map_err(|_| format_error())?
            .as_bool()
        {
            return Err(RenderError::status(0xACB5, "Invalid XML content."));
        }
        let values = doc
            .selectNodes(&BSTR::from("//RESULTS//VALUE | //RESULTS//KEYVALUE"))
            .map_err(|_| format_error())?;
        for i in 0..values.length().map_err(|_| format_error())? {
            let node = values.get_item(i).map_err(|_| format_error())?;
            let mut text = WideText(node.text().map_err(|_| format_error())?.to_vec());
            for table in tables {
                for (from, to) in &table.entries {
                    text = crate::render::replace(&text, from, to)
                }
            }
            node.Settext(&bstr(&text)).map_err(|_| format_error())?;
        }
        Ok(WideText(doc.xml().map_err(|_| format_error())?.to_vec()))
    }
}

#[cfg(test)]
mod html_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6"]
    fn embedded_mof_and_html_formats_match_native_goldens() {
        let source: WideText =
            include_str!("../../../tests/fixtures/reference/os-get-rawxml.stdout.bin").into();
        for (mode, expected) in [
            (
                "mof",
                include_str!("../../../tests/fixtures/reference/os-get-mof.stdout.bin"),
            ),
            (
                "htable",
                include_str!("../../../tests/fixtures/format-reference/format-htable.stdout.bin"),
            ),
            (
                "hform",
                include_str!("../../../tests/fixtures/format-reference/format-hform.stdout.bin"),
            ),
        ] {
            let style =
                crate::render::builtin_source(&mode.into()).expect("built-in format absent");
            let output = transform_source(&source, &style, &[]).unwrap();
            let output = crate::render::finish_text(&output);
            let expected: Vec<_> = expected.encode_utf16().collect();
            let at = output
                .0
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(output.0.len().min(expected.len()));
            assert!(
                output.0 == expected,
                "{mode}: first difference {at}, got {} units expected {}",
                output.0.len(),
                expected.len()
            );
        }
    }
}

#[cfg(test)]
mod xml_view_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6"]
    fn xml_view_matches_native_colored_markup() {
        let input =
            include_str!("../../../tests/fixtures/format-reference/format-rawxml.stdout.bin")
                .replace("/format:rawxml", "/format:xml");
        let style = crate::render::builtin_source(&"xml".into()).unwrap();
        let output =
            crate::render::finish_text(&transform_source(&input.into(), &style, &[]).unwrap());
        let expected: Vec<_> =
            include_str!("../../../tests/fixtures/format-reference/format-xml.stdout.bin")
                .encode_utf16()
                .collect();
        let at = output
            .0
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or(output.0.len().min(expected.len()));
        assert!(
            output.0 == expected,
            "first difference {at}: got {} expected {}",
            output.0.len(),
            expected.len()
        );
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;
    #[test]
    #[ignore = "requires Windows MSXML6"]
    fn all_stylesheets_are_resolved_before_any_stage_runs() {
        let spec = crate::model::FormatSpec {
            transforms: vec![
                TransformSpec {
                    name: "csv".into(),
                    parameters: vec![],
                },
                TransformSpec {
                    name: r"C:\WmicCompatMissingStylesheet=42".into(),
                    parameters: vec![],
                },
            ],
            before: vec![],
            after: vec![],
        };
        let err = crate::render::transform(&"<COMMAND/>".into(), &spec).unwrap_err();
        assert_eq!(err.code, 0xACB2);
    }
}
