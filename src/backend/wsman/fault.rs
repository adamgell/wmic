//! SOAP fault reading and the mapping of faults to WMI status codes.
use crate::model::{CimValue, ManagementError, Property, WideText};
use roxmltree::Node;

const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// What a fault says, gathered from every detail shape the WMI plugin produces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fault {
    /// Local name of the SOAP subcode, for example `CannotProcessFilter`.
    pub subcode: Option<String>,
    pub reason: String,
    /// `Code` attribute of the innermost `WSManFault`.
    pub wsman_code: Option<u32>,
    /// `CIMStatusCode` of an `MSFT_WmiError` detail.
    pub cim_status: Option<u32>,
    /// `StatusCode` of an `__ExtendedStatus` detail: the exact WMI status.
    pub status_code: Option<u32>,
    /// A status quoted as `HRESULT = 0x...` in the innermost message.
    pub quoted_status: Option<u32>,
    pub message: Option<String>,
    pub operation: Option<String>,
    pub parameter_info: Option<String>,
}

fn local<'a>(node: Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

fn text_of(node: Node) -> String {
    node.children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .collect()
}

fn find<'a, 'input>(root: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    root.descendants()
        .find(|n| n.is_element() && local(*n) == name)
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|c| c.is_element() && local(*c) == name)
}

fn number(node: Node) -> Option<u32> {
    if node.attribute((NS_XSI, "nil")) == Some("true") {
        return None;
    }
    text_of(node).trim().parse().ok()
}

fn quoted_status(message: &str) -> Option<u32> {
    let at = message.find("HRESULT = 0x")? + "HRESULT = 0x".len();
    u32::from_str_radix(message.get(at..at + 8)?, 16).ok()
}

impl Fault {
    pub fn from_node(fault: Node) -> Fault {
        let mut out = Fault::default();
        let subcode = child(fault, "Code")
            .and_then(|c| child(c, "Subcode"))
            .and_then(|s| child(s, "Value"));
        out.subcode = subcode.map(|v| {
            let text = text_of(v);
            let text = text.trim();
            text.rsplit_once(':')
                .map_or(text, |(_, name)| name)
                .to_owned()
        });
        out.reason = child(fault, "Reason")
            .and_then(|r| child(r, "Text"))
            .map(|t| text_of(t).trim().to_owned())
            .unwrap_or_default();
        let Some(detail) = child(fault, "Detail") else {
            return out;
        };
        let innermost = detail.descendants().rfind(|n| {
            n.is_element()
                && local(*n) == "WSManFault"
                && !n
                    .descendants()
                    .skip(1)
                    .any(|d| d.is_element() && local(d) == "WSManFault")
        });
        let wsman_message = innermost
            .and_then(|n| child(n, "Message"))
            .filter(|m| !m.children().any(|c| c.is_element()))
            .map(|m| text_of(m).trim().to_owned());
        out.wsman_code = innermost
            .and_then(|n| n.attribute("Code"))
            .and_then(|c| c.parse().ok());
        out.quoted_status = wsman_message.as_deref().and_then(quoted_status);
        if let Some(error) = find(detail, "MSFT_WmiError") {
            out.cim_status = child(error, "CIMStatusCode").and_then(number);
            out.message = child(error, "Message")
                .map(|m| text_of(m).trim().to_owned())
                .filter(|m| !m.is_empty());
        }
        if let Some(status) = find(detail, "__ExtendedStatus") {
            out.status_code = child(status, "StatusCode").and_then(number);
            let text = |name| child(status, name).map(|n| text_of(n).trim().to_owned());
            out.operation = text("Operation").filter(|t| !t.is_empty());
            out.parameter_info = text("ParameterInfo").filter(|t| !t.is_empty());
        }
        if out.message.is_none() {
            out.message = wsman_message.filter(|m| !m.is_empty());
        }
        out
    }

    fn subcode_is(&self, name: &str) -> bool {
        self.subcode.as_deref() == Some(name)
    }

    fn says(&self, needle: &str) -> bool {
        [Some(self.reason.as_str()), self.message.as_deref()]
            .into_iter()
            .flatten()
            .any(|text| text.contains(needle))
    }

    /// The enumeration context expired, was released, or never existed.
    pub fn is_stale_context(&self) -> bool {
        self.subcode_is("InvalidEnumerationContext")
    }

    /// The Pull ran into its operation timeout; the enumeration context stays valid.
    pub fn is_timed_out(&self) -> bool {
        self.subcode_is("TimedOut")
    }

    /// The class exists and the addressed instance does not.
    pub fn is_instance_not_found(&self) -> bool {
        self.hresult() == 0x8004_1002
            && (self.subcode_is("DestinationUnreachable") || self.subcode_is("CannotProcessFilter"))
    }

    fn cim_hresult(&self, status: u32) -> u32 {
        match status {
            2 => 0x8004_1003,
            3 => 0x8004_100E,
            4 if self.subcode_is("CannotProcessFilter") || self.says("WQL query is invalid") => {
                0x8004_1017
            }
            4 => 0x8004_1008,
            5 => 0x8004_1010,
            6 => 0x8004_1002,
            7 => 0x8004_100C,
            8 => 0x8004_1025,
            9 => 0x8004_1026,
            10 => 0x8004_100D,
            11 => 0x8004_1019,
            13 => 0x8004_1005,
            14 => 0x8004_1018,
            15 => 0x8004_1017,
            16 | 17 => 0x8004_102E,
            27 => 0x8004_106C,
            _ => 0x8004_1001,
        }
    }

    fn subcode_hresult(&self) -> Option<u32> {
        Some(match self.subcode.as_deref()? {
            "ActionNotSupported" => 0x8004_102E,
            "DestinationUnreachable" => 0x8004_1002,
            "CannotProcessFilter" => 0x8004_1017,
            "InvalidSelectors" => 0x8004_1008,
            "TimedOut" => 0x8004_1069,
            "AccessDenied" => 0x8007_0005,
            "InternalError" if self.says("class name must be") => 0x8004_1008,
            _ if self.wsman_code == Some(5) => 0x8007_0005,
            _ => return None,
        })
    }

    /// WMI status for the fault. Order: exact provider status, quoted status, CIM status,
    /// SOAP subcode, then generic failure. WinRM codes such as 0x80338000 never appear here.
    pub fn hresult(&self) -> u32 {
        self.status_code
            .or(self.quoted_status)
            .or_else(|| self.cim_status.map(|s| self.cim_hresult(s)))
            .or_else(|| self.subcode_hresult())
            .unwrap_or(0x8004_1001)
    }

    /// Server text: WMI error message, innermost WSMan message, then the SOAP reason.
    pub fn text(&self) -> &str {
        self.message
            .as_deref()
            .filter(|m| !m.is_empty())
            .unwrap_or(&self.reason)
    }
}

/// How the failed request addressed its target.
#[derive(Clone, Copy, Debug)]
pub struct FaultContext<'a> {
    pub node: &'a WideText,
    /// Operation reported when the status has no operation of its own.
    pub operation: &'a str,
    /// Lookup by path or class name: an unknown class reports "Not found" for the class.
    pub by_path: bool,
}

/// Short text WMIC prints for a status code.
pub fn status_text(code: u32) -> Option<&'static str> {
    Some(match code {
        0x8004_1001 => "Generic failure",
        0x8004_1002 => "Not found",
        0x8004_1003 | 0x8007_0005 => "Access denied",
        0x8004_1005 => "Type mismatch",
        0x8004_1008 => "Invalid parameter",
        0x8004_100C => "Not supported",
        0x8004_100D => "Invalid superclass",
        0x8004_100E => "Invalid namespace",
        0x8004_1010 => "Invalid class",
        0x8004_1014 => "Initialization failure",
        0x8004_1017 => "Invalid query",
        0x8004_1018 => "Invalid query type",
        0x8004_1019 => "Already exists",
        0x8004_1025 => "Class has children",
        0x8004_1026 => "Class has instances",
        0x8004_102E => "Invalid method",
        0x8004_103A => "Invalid object path",
        0x8004_1069 => "Timed out",
        0x8004_106C => "Quota violation",
        0x8007_06BA => "The RPC server is unavailable.",
        _ => return None,
    })
}

/// Diagnostic properties kept on an error: the server text and the failing WMI call.
pub fn detail_properties(detail: &str, fault: Option<&Fault>) -> Vec<Property> {
    let property = |name: &str, value: &str| Property {
        name: name.into(),
        value: CimValue::String(value.into()),
        cim_type: 8,
        system: false,
        qualifiers: vec![],
    };
    let mut out = vec![];
    if !detail.is_empty() {
        out.push(property("Detail", detail));
    }
    if let Some(fault) = fault {
        if let Some(operation) = &fault.operation {
            out.push(property("Operation", operation));
        }
        if let Some(info) = &fault.parameter_info {
            out.push(property("ParameterInfo", info));
        }
    }
    out
}

/// Builds the error the session renders for a fault.
pub fn to_error(fault: &Fault, context: &FaultContext) -> ManagementError {
    let mut code = fault.hresult();
    let mut operation = context.operation;
    if code == 0x8004_1010 && context.by_path {
        code = 0x8004_1002;
        operation = "class";
    } else if matches!(code, 0x8004_1010 | 0x8004_100E) {
        operation = "query";
    }
    let message = status_text(code).unwrap_or_else(|| fault.text());
    ManagementError {
        code,
        target: context.node.clone(),
        message: message.into(),
        operation: operation.into(),
        extended: detail_properties(fault.text(), Some(fault)),
        partial: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    fn parse(subcode: &str, detail: &str) -> Fault {
        let xml = format!(
            r#"<s:Fault xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="x" xmlns:n="y" xmlns:w="z" xmlns:f="http://schemas.microsoft.com/wbem/wsman/1/wsmanfault" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><s:Code><s:Value>s:Sender</s:Value><s:Subcode><s:Value>{subcode}</s:Value></s:Subcode></s:Code><s:Reason><s:Text>reason </s:Text></s:Reason><s:Detail>{detail}</s:Detail></s:Fault>"#
        );
        let doc = Document::parse(&xml).unwrap();
        Fault::from_node(doc.root_element())
    }

    #[test]
    fn extended_status_is_the_exact_wmi_status() {
        let fault = parse(
            "w:InternalError",
            r#"<f:WSManFault Code="1359"><f:Message><f:ProviderFault><f:WSManFault Code="1359"><f:Message>HRESULT = 0x80041014</f:Message></f:WSManFault><f:ExtendedError><p:__ExtendedStatus xmlns:p="q"><p:Operation>ExecQuery</p:Operation><p:StatusCode>2147749908</p:StatusCode></p:__ExtendedStatus></f:ExtendedError></f:ProviderFault></f:Message></f:WSManFault>"#,
        );
        assert_eq!(fault.status_code, Some(0x8004_1014));
        assert_eq!(fault.quoted_status, Some(0x8004_1014));
        assert_eq!(fault.operation.as_deref(), Some("ExecQuery"));
        assert_eq!(fault.hresult(), 0x8004_1014);
    }

    #[test]
    fn nil_status_code_falls_through_to_the_subcode() {
        let fault = parse(
            "a:DestinationUnreachable",
            r#"<f:WSManFault Code="2150858752"><f:Message><f:ProviderFault><f:WSManFault Code="2150858752"><f:Message>missing </f:Message></f:WSManFault><f:ExtendedError><p:__ExtendedStatus xmlns:p="q"><p:StatusCode xsi:nil="true"/></p:__ExtendedStatus></f:ExtendedError></f:ProviderFault></f:Message></f:WSManFault>"#,
        );
        assert_eq!(fault.status_code, None);
        assert_eq!(fault.wsman_code, Some(2150858752));
        assert_eq!(fault.message.as_deref(), Some("missing"));
        assert_eq!(fault.hresult(), 0x8004_1002);
        assert!(fault.is_instance_not_found());
    }

    #[test]
    fn wmi_error_cim_status_maps_through_the_table() {
        let detail = |status: u32, message: &str| {
            format!(
                r#"<p:MSFT_WmiError xmlns:p="q" xmlns:cim="c"><p:CIMStatusCode xsi:type="cim:cimUnsignedInt">{status}</p:CIMStatusCode><p:Message>{message} </p:Message></p:MSFT_WmiError>"#
            )
        };
        let class = parse("a:DestinationUnreachable", &detail(5, "no class"));
        assert_eq!(class.hresult(), 0x8004_1010);
        assert_eq!(class.text(), "no class");
        assert!(!class.is_instance_not_found());
        assert_eq!(
            parse(
                "n:CannotProcessFilter",
                &detail(4, "The WQL query is invalid.")
            )
            .hresult(),
            0x8004_1017
        );
        assert_eq!(
            parse("w:InternalError", &detail(4, "other")).hresult(),
            0x8004_1008
        );
        assert_eq!(
            parse("w:InternalError", &detail(2, "denied")).hresult(),
            0x8004_1003
        );
        assert_eq!(
            parse("a:DestinationUnreachable", &detail(3, "ns")).hresult(),
            0x8004_100E
        );
        let missing = parse("n:CannotProcessFilter", &detail(6, "No instance found"));
        assert_eq!(missing.hresult(), 0x8004_1002);
        assert!(missing.is_instance_not_found());
        assert_eq!(parse("x:Y", &detail(99, "odd")).hresult(), 0x8004_1001);
    }

    #[test]
    fn subcode_fallbacks() {
        let none = "";
        assert_eq!(parse("a:ActionNotSupported", none).hresult(), 0x8004_102E);
        assert_eq!(parse("n:CannotProcessFilter", none).hresult(), 0x8004_1017);
        assert_eq!(parse("w:InvalidSelectors", none).hresult(), 0x8004_1008);
        assert_eq!(parse("w:TimedOut", none).hresult(), 0x8004_1069);
        assert_eq!(parse("w:InternalError", none).hresult(), 0x8004_1001);
        assert!(parse("n:InvalidEnumerationContext", none).is_stale_context());
        let access = parse(
            "w:InternalError",
            r#"<f:WSManFault Code="5"><f:Message>Access is denied.</f:Message></f:WSManFault>"#,
        );
        assert_eq!(access.hresult(), 0x8007_0005);
    }

    #[test]
    fn errors_use_wmic_texts_and_operations() {
        let node: WideText = "SRV".into();
        let ctx = |operation, by_path| FaultContext {
            node: &node,
            operation,
            by_path,
        };
        let class = parse(
            "a:DestinationUnreachable",
            r#"<p:MSFT_WmiError xmlns:p="q"><p:CIMStatusCode>5</p:CIMStatusCode></p:MSFT_WmiError>"#,
        );
        let query = to_error(&class, &ctx("query", false));
        assert_eq!(
            (
                query.code,
                query.message.to_string_lossy(),
                query.operation.to_string_lossy()
            ),
            (0x8004_1010, "Invalid class".into(), "query".into())
        );
        let path = to_error(&class, &ctx("query", true));
        assert_eq!(
            (
                path.code,
                path.message.to_string_lossy(),
                path.operation.to_string_lossy()
            ),
            (0x8004_1002, "Not found".into(), "class".into())
        );
        let method = to_error(&parse("a:ActionNotSupported", ""), &ctx("method", false));
        assert_eq!(method.code, 0x8004_102E);
        assert_eq!(method.operation, WideText::from("method"));
        assert_eq!(method.target, node);
    }
}
