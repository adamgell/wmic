//! Classic WMIC query execution and stylesheet diagnostics.
use super::{event, wbem_dir};
use crate::{
    backend::{ResolvedRequest, build_query},
    model::{ManagementError, ManagementResult, MethodResult, Verb, WideText},
    render::{BuiltinKind, builtin_kind, compile_format},
};

const EXEC: &str = "execengine";
const FORMAT: &str = "formatengine";
const NEXT: &str = "IEnumWbemClassObject->Next(WBEM_INFINITE, 1, -, -)";

#[derive(Default)]
pub(in crate::session) struct QueryState {
    text_source: bool,
    context: bool,
}

fn supported(request: &ResolvedRequest) -> bool {
    matches!(request.verb, Verb::Get | Verb::List) && !request.class_mode
}

fn parser_rejected(request: &ResolvedRequest, error: Option<&ManagementError>) -> bool {
    error.is_some_and(|e| {
        e.operation.eq_ignore_case("class")
            || !request.from_alias && matches!(e.code, 0x80041002 | 0x8004100e | 0x80041010)
    })
}

fn connected(error: Option<&ManagementError>) -> bool {
    !error.is_some_and(|e| {
        matches!(
            e.code,
            0x800706ba | 0x8007052e | 0x80070005 | 0x80041003 | 0x8004100e
        )
    })
}

fn query_text(request: &ResolvedRequest) -> String {
    let query = build_query(request).to_string_lossy();
    let projected = match request.verb {
        Verb::Get => !request.properties.is_empty(),
        Verb::List => true,
        _ => false,
    };
    if let Some(end) = query.to_ascii_uppercase().find(" FROM ") {
        let columns = &query[7..end];
        let columns = if projected {
            columns.replace(',', ", ")
        } else {
            columns.to_owned()
        };
        format!("SELECT {columns} FROM {}", &query[end + 6..])
    } else {
        query
    }
}

#[cfg(test)]
fn execution(
    request: &ResolvedRequest,
    result: Result<&ManagementResult, &ManagementError>,
    node: &WideText,
) -> WideText {
    QueryState::default().execution(request, result, node)
}

pub(in crate::session) fn method_record(
    request: &ResolvedRequest,
    result: &MethodResult,
) -> WideText {
    let mut out = WideText::default();
    let method = request
        .method
        .as_ref()
        .map(WideText::to_string_lossy)
        .unwrap_or_default();
    let path = result.path.to_string_lossy();
    event(
        &mut out,
        EXEC,
        4994,
        "IWbemClassObject::Get(L\"__PATH\", 0, -, 0, 0)",
        Some(&path),
        true,
    );
    event(
        &mut out,
        EXEC,
        2991,
        &format!(r#"IWbemServices::ExecMethod(L"{path}", L"{method}", 0, NULL, -, -, NULL)"#),
        None,
        true,
    );
    event(
        &mut out,
        EXEC,
        3057,
        "IWbemClassObject->GetObjectText(0, -)",
        None,
        true,
    );
    out
}

impl QueryState {
    pub(in crate::session) fn observe(
        &mut self,
        request: &ResolvedRequest,
        result: Result<&ManagementResult, &ManagementError>,
    ) {
        let error = result.err();
        if supported(request) && !parser_rejected(request, error) {
            self.text_source = true;
            self.context |= connected(error);
        }
    }

    pub(in crate::session) fn execution(
        &mut self,
        request: &ResolvedRequest,
        result: Result<&ManagementResult, &ManagementError>,
        node: &WideText,
    ) -> WideText {
        let mut out = WideText::default();
        let method_results = match result {
            Ok(ManagementResult::Methods(methods))
                if request.verb == Verb::Call && request.arguments.is_empty() =>
            {
                Some(methods)
            }
            _ => None,
        };
        if !supported(request) && method_results.is_none() {
            return out;
        }
        let error = result.err();
        // PATH class/namespace validation belongs to the parser and precedes execution.
        if parser_rejected(request, error) {
            return out;
        }
        let connection = &request.connection;
        if method_results.is_none() && !self.text_source {
            self.text_source = true;
            event(
                &mut out,
                EXEC,
                2787,
                "CoCreateInstance(CLSID_WbemObjectTextSrc, NULL,CLSCTX_INPROC_SERVER, IID_IWbemObjectTextSrc, -)",
                None,
                true,
            );
        }
        let connected = connected(error);
        let namespace = connection.namespace.to_string_lossy();
        let locale = connection
            .locale
            .as_ref()
            .map_or_else(|| "<null>".into(), WideText::to_string_lossy);
        let user = connection
            .user
            .as_ref()
            .map_or_else(|| "<null>".into(), WideText::to_string_lossy);
        let authority = connection
            .authority
            .as_ref()
            .map_or_else(|| "<null>".into(), WideText::to_string_lossy);
        event(
            &mut out,
            EXEC,
            2697,
            &format!(
                r#"IWbemLocator::ConnectServer(L"\\{node}\{namespace}", L"{user}", *, L"{locale}", 0L, L"{authority}", NULL, -)"#
            ),
            None,
            connected,
        );
        if !connected {
            return out;
        }
        event(
            &mut out,
            EXEC,
            2740,
            &format!(
                "CoSetProxyBlanket(-, RPC_C_AUTHN_DEFAULT, RPC_C_AUTHZ_NONE,NULL, {},   {}, -, EOAC_NONE)",
                connection.authentication, connection.impersonation
            ),
            None,
            true,
        );
        if let Some(methods) = method_results {
            let method = request
                .method
                .as_ref()
                .map(WideText::to_string_lossy)
                .unwrap_or_default();
            event(
                &mut out,
                EXEC,
                690,
                &format!(
                    r#"IWbemServices::GetObject(L"{}", 0, NULL, -)"#,
                    request.class_name
                ),
                None,
                true,
            );
            event(
                &mut out,
                EXEC,
                705,
                &format!(r#"IWbemClassObject::GetMethod(L"{method}", 0, -, -)"#),
                None,
                true,
            );
            event(
                &mut out,
                EXEC,
                4927,
                &format!(
                    r#"IWbemServices::ExecQuery(L"WQL", L"{}", 0, NULL, -)"#,
                    query_text(request)
                ),
                None,
                true,
            );
            event(
                &mut out,
                EXEC,
                4972,
                &format!(
                    "CoSetProxyBlanket(-, RPC_C_AUTHN_DEFAULT, RPC_C_AUTHZ_NONE, NULL, {},   {}, -, EOAC_NONE)",
                    connection.authentication, connection.impersonation
                ),
                None,
                true,
            );
            for method_result in methods {
                out.0.extend(method_record(request, method_result).0);
            }
            return out;
        }
        if !self.context {
            self.context = true;
            event(
                &mut out,
                EXEC,
                2838,
                "CoCreateInstanceEx(CLSID_WbemContext, NULL,CLSCTX_INPROC_SERVER, 0, 1, -)",
                None,
                true,
            );
        }
        let query_success =
            !error.is_some_and(|e| e.operation.eq_ignore_case("query") && e.code != 0x80041017);
        event(
            &mut out,
            EXEC,
            319,
            &format!(
                r#"IWbemServices::ExecQuery(L"WQL", L"{}", 0, NULL, -)"#,
                query_text(request)
            ),
            None,
            query_success,
        );
        if !query_success {
            return out;
        }
        event(
            &mut out,
            EXEC,
            338,
            "IWbemContext::SetValue(L\"ExcludeSystemProperties\",0, -)",
            None,
            true,
        );
        let blanket = if let Some(principal) = authority
            .get(9..)
            .filter(|_| authority[..9].eq_ignore_ascii_case("KERBEROS:"))
        {
            format!(
                "CoSetProxyBlanket(-, RPC_C_AUTHN_GSS_KERBEROS,RPC_C_AUTHZ_NONE, {principal}, {},   {}, -, EOAC_NONE)",
                connection.authentication, connection.impersonation
            )
        } else {
            format!(
                "CoSetProxyBlanket(-, RPC_C_AUTHN_DEFAULT,RPC_C_AUTHZ_NONE, NULL, {},   {}, -, EOAC_NONE)",
                connection.authentication, connection.impersonation
            )
        };
        let secured = !error.is_some_and(|e| e.operation.eq_ignore_case("enumeration security"));
        event(&mut out, EXEC, 385, &blanket, None, secured);
        if !secured {
            return out;
        }
        let objects = match result
            .ok()
            .or_else(|| error.and_then(|e| e.partial.as_deref()))
        {
            Some(ManagementResult::Objects(set)) => set.objects.len(),
            _ => 0,
        };
        let enumerated = error.is_none() || objects != 0;
        event(&mut out, EXEC, 454, NEXT, None, enumerated);
        if enumerated {
            for index in 0..objects {
                event(
                    &mut out,
                    EXEC,
                    473,
                    "IWbemObjectTextSrc::GetText(0, -, WMI_OBJECT_TEXT_CIM_DTD_2_0, -, -)",
                    None,
                    true,
                );
                event(
                    &mut out,
                    EXEC,
                    530,
                    NEXT,
                    None,
                    error.is_none() || index + 1 < objects,
                );
            }
        }
        out
    }
}

pub(in crate::session) fn formatting(request: &ResolvedRequest, _has_rows: bool) -> WideText {
    let mut out = WideText::default();
    if !supported(request) {
        return out;
    }
    let compiled;
    let spec = if let Some(spec) = &request.format_spec {
        spec
    } else {
        let Ok(spec) = compile_format(request, &[]) else {
            return out;
        };
        compiled = spec;
        &compiled
    };
    // The legacy sequence below is measured for the single built-in stylesheet paths.
    if spec.transforms.len() != 1 {
        return out;
    }
    let filename = match builtin_kind(&spec.transforms[0].name) {
        Some(BuiltinKind::Table) => r"\texttable.xsl",
        Some(BuiltinKind::List) => r"\textvaluelist.xsl",
        Some(BuiltinKind::Rawxml) => r"\rawxml.xsl",
        Some(BuiltinKind::Csv) => r"en-US\csv.xsl",
        _ => return out,
    };
    for (line, call) in [
        (
            197,
            "CoCreateInstance(CLSID_FreeThreadedDOMDocument60, NULL, CLSCTX_INPROC_SERVER, IID_IXMLDOMDocument2, -)",
        ),
        (228, "IXMLDOMDocument::loadXML(-, -)"),
        (
            3240,
            "CoCreateInstance(CLSID_XSLTemplate60, NULL, CLSCTX_SERVER, IID_IXSLTemplate, -)",
        ),
        (
            3263,
            "CoCreateInstance(CLSID_FreeThreadedDOMDocument60, NULL, CLSCTX_SERVER,IID_IXMLDOMDocument2, -)",
        ),
        (3275, "IXSLDOMDocument2::put_async(VARIANT_FALSE)"),
        (3290, "IXSLDOMDocument2::setProperty(VARIANT_TRUE)"),
    ] {
        event(&mut out, FORMAT, line, call, None, true);
    }
    event(
        &mut out,
        FORMAT,
        3305,
        &format!(r#"IXSLDOMDocument2::load(L"{}{filename}", -)"#, wbem_dir()),
        None,
        true,
    );
    for (line, call) in [
        (3319, "IXSTemplate::putref_stylesheet(-)"),
        (3331, "IXSTemplate::createProcessor(-)"),
        (3368, "IXSProcessor::put_input(-)"),
        (3409, "IXSProcessor::tranform(-)"),
        (3449, "IXSProcessor::get_output(-)"),
    ] {
        event(&mut out, FORMAT, line, call, None, true);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        alias::{AliasStore, ResolvedCommand},
        model::{EntryMode, MethodResult, ObjectSet, Record},
        parse::parse_command,
        session::SessionSettings,
    };

    fn request(text: &str) -> ResolvedRequest {
        let command = parse_command(&text.into(), EntryMode::Interactive).unwrap();
        let ResolvedCommand::Management(request) = AliasStore::stock()
            .unwrap()
            .resolve(&command, &SessionSettings::default())
            .unwrap()
        else {
            panic!("not a management request")
        };
        *request
    }

    fn rows(count: usize) -> ManagementResult {
        ManagementResult::Objects(ObjectSet {
            objects: vec![Record::default(); count],
            schema: None,
        })
    }

    fn canonical() -> String {
        let text = String::from_utf8(
            include_bytes!("../../tests/fixtures/connection-reference/trace.stderr.bin").to_vec(),
        )
        .unwrap();
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\WINDOWS".into());
        text.replace("C:\\WINDOWS", &system_root)
    }

    #[test]
    fn one_row_query_reproduces_microsoft_execution_diagnostics() {
        let text = canonical();
        let start = text
            .find("SUCCESS: CoCreateInstance(CLSID_WbemObjectTextSrc")
            .unwrap();
        let end = text
            .find("SUCCESS: CoCreateInstance(CLSID_FreeThreadedDOMDocument60")
            .unwrap();
        assert_eq!(
            execution(
                &request("os get Caption"),
                Ok(&rows(1)),
                &"DESKTOP-N4H9E7R".into(),
            )
            .to_string_lossy(),
            text[start..end]
        );
    }

    #[test]
    fn query_projection_spacing_preserves_where_values() {
        let output = execution(
            &request("process where \"Name='a,b.exe'\" get Name,ProcessId"),
            Ok(&rows(0)),
            &"NODE".into(),
        )
        .to_string_lossy();
        assert!(output.contains("SELECT Name, ProcessId FROM Win32_Process WHERE Name='a,b.exe'"));
    }

    #[test]
    fn empty_query_has_only_initial_enumeration_diagnostic() {
        let output = execution(
            &request("path Win32_Process get Name"),
            Ok(&rows(0)),
            &"NODE".into(),
        )
        .to_string_lossy();
        assert!(output.contains("Line:    454 File:"));
        assert!(!output.contains("Line:    473 File:"));
        assert!(!output.contains("Line:    530 File:"));
    }

    #[test]
    fn enumeration_diagnostics_follow_the_actual_row_count() {
        let output = execution(
            &request("path Win32_Process get Name"),
            Ok(&rows(3)),
            &"NODE".into(),
        )
        .to_string_lossy();
        assert_eq!(output.matches("Line:    454 File:").count(), 1);
        assert_eq!(output.matches("Line:    473 File:").count(), 3);
        assert_eq!(output.matches("Line:    530 File:").count(), 3);
    }

    #[test]
    fn invalid_property_fails_when_the_enumerator_is_read() {
        let error = ManagementError {
            code: 0x80041017,
            target: "NODE".into(),
            message: "Invalid query".into(),
            operation: "enumerate".into(),
            extended: vec![],
            partial: None,
        };
        let output = execution(
            &request("os get WmicTraceMissingProperty"),
            Err(&error),
            &"NODE".into(),
        )
        .to_string_lossy();
        assert!(output.contains("SUCCESS: IWbemServices::ExecQuery"));
        assert!(output.ends_with(concat!(
            "FAIL: IEnumWbemClassObject->Next(WBEM_INFINITE, 1, -, -)\r\n",
            "Line:    454 File: onecore\\admin\\wmi\\wbem\\tools\\wmic\\execengine.cpp\r\n\r\n"
        )));
    }

    #[test]
    fn table_format_reproduces_microsoft_stylesheet_diagnostics() {
        let text = canonical();
        let start = text
            .find("SUCCESS: CoCreateInstance(CLSID_FreeThreadedDOMDocument60")
            .unwrap();
        assert_eq!(
            formatting(&request("os get Caption"), true).to_string_lossy(),
            text[start..]
        );
    }

    #[test]
    fn selected_formats_report_the_microsoft_resource_filename() {
        let text = canonical();
        let start = text
            .find("SUCCESS: CoCreateInstance(CLSID_FreeThreadedDOMDocument60")
            .unwrap();
        for (command, filename) in [
            ("os get Caption /format:value", r"\textvaluelist.xsl"),
            ("os get Caption /format:rawxml", r"\rawxml.xsl"),
            ("os get Caption /format:csv", r"en-US\csv.xsl"),
        ] {
            // The independently captured valid original traces differ from the table tail
            // only by this resource name, on both ARM64 and x86.
            let expected = text[start..].replace(r"\texttable.xsl", filename);
            assert_eq!(
                formatting(&request(command), true).to_string_lossy(),
                expected,
                "{command}"
            );
        }
    }

    #[test]
    fn missing_rows_still_report_the_complete_format_pipeline() {
        let text = canonical();
        let start = text
            .find("SUCCESS: CoCreateInstance(CLSID_FreeThreadedDOMDocument60")
            .unwrap();
        assert_eq!(
            formatting(&request("os get WmicTraceMissingProperty"), false).to_string_lossy(),
            text[start..]
        );
    }

    #[test]
    fn no_input_method_reproduces_microsoft_execution_diagnostics() {
        let text = String::from_utf8(
            include_bytes!("../../tests/fixtures/trace-reference/call-readonly.stderr.bin")
                .to_vec(),
        )
        .unwrap();
        let start = text
            .find("SUCCESS: IWbemLocator::ConnectServer(L\"\\\\DESKTOP-N4H9E7R")
            .unwrap();
        let result = ManagementResult::Methods(vec![MethodResult {
            path: r#"\\DESKTOP-N4H9E7R\ROOT\CIMV2:Win32_Process.Handle="0""#.into(),
            return_value: Some(crate::model::CimValue::Uint32(2)),
            out_parameters: vec![],
            text: None,
        }]);
        // The existing method renderer appends its own final CRLF independently of TRACE.
        let expected = &text[start..text.len() - 2];
        assert_eq!(
            execution(
                &request("process where \"ProcessId=0\" call GetOwner"),
                Ok(&result),
                &"DESKTOP-N4H9E7R".into(),
            )
            .to_string_lossy(),
            expected
        );
    }

    #[test]
    fn second_query_reuses_native_text_source_and_context() {
        let mut state = QueryState::default();
        let request = request("os get Caption");
        let objects = rows(1);
        state.execution(&request, Ok(&objects), &"DESKTOP-N4H9E7R".into());
        let actual = state.execution(&request, Ok(&objects), &"DESKTOP-N4H9E7R".into());
        let reference = std::str::from_utf8(include_bytes!(
            "../../tests/fixtures/trace-reference/interactive-toggle.stderr.bin"
        ))
        .unwrap();
        let marker = "SUCCESS: IWbemLocator::ConnectServer(L\"\\\\DESKTOP-N4H9E7R";
        let start = reference.match_indices(marker).nth(1).unwrap().0;
        let end = reference[start..]
            .find("SUCCESS: CoCreateInstance(CLSID_FreeThreadedDOMDocument60")
            .unwrap()
            + start;
        assert_eq!(actual.to_string_lossy(), reference[start..end]);
    }

    #[test]
    fn connection_failure_does_not_create_a_query_context() {
        let mut state = QueryState::default();
        let request = request("os get Caption");
        let failure = ManagementError {
            code: 0x800706ba,
            target: "NODE".into(),
            message: "The RPC server is unavailable.".into(),
            operation: "query".into(),
            extended: vec![],
            partial: None,
        };
        let failed = state
            .execution(&request, Err(&failure), &"NODE".into())
            .to_string_lossy();
        assert!(failed.contains("Line:   2787 File:"));
        assert!(!failed.contains("Line:   2838 File:"));
        let recovered = state
            .execution(&request, Ok(&rows(1)), &"NODE".into())
            .to_string_lossy();
        assert!(!recovered.contains("Line:   2787 File:"));
        assert!(recovered.contains("Line:   2838 File:"));
    }

    #[test]
    fn methods_and_unsupported_class_requests_do_not_warm_query_resources() {
        let mut state = QueryState::default();
        state.execution(
            &request("process where \"ProcessId=0\" call GetOwner"),
            Ok(&ManagementResult::Methods(vec![])),
            &"NODE".into(),
        );
        state.execution(
            &request("class Win32_Process"),
            Ok(&rows(1)),
            &"NODE".into(),
        );
        let query = state
            .execution(&request("os get Caption"), Ok(&rows(1)), &"NODE".into())
            .to_string_lossy();
        assert!(query.contains("Line:   2787 File:"));
        assert!(query.contains("Line:   2838 File:"));
    }

    #[test]
    fn observing_an_untraced_query_warms_resources_for_later_trace() {
        let mut state = QueryState::default();
        let request = request("os get Caption");
        let result = rows(1);
        state.observe(&request, Ok(&result));
        let trace = state
            .execution(&request, Ok(&result), &"NODE".into())
            .to_string_lossy();
        assert!(!trace.contains("Line:   2787 File:"));
        assert!(!trace.contains("Line:   2838 File:"));
        assert!(trace.contains("Line:    319 File:"));
    }
}
