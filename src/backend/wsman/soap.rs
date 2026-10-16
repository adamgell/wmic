//! Request envelopes for the operations the WMI plugin serves.
use super::{
    convert::{cim_to_xs_datetime, cim_to_xs_duration, is_cim_datetime, value_text},
    path::parse_object_path,
};
use crate::model::{CimValue, Property};
use std::io::Read;

pub const WMI_URI: &str = "http://schemas.microsoft.com/wbem/wsman/1/wmi/";
pub const ANONYMOUS: &str = "http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous";
pub const WQL_DIALECT: &str = "http://schemas.microsoft.com/wbem/wsman/1/WQL";
pub const ACTION_GET: &str = "http://schemas.xmlsoap.org/ws/2004/09/transfer/Get";
pub const ACTION_ENUMERATE: &str = "http://schemas.xmlsoap.org/ws/2004/09/enumeration/Enumerate";
pub const ACTION_PULL: &str = "http://schemas.xmlsoap.org/ws/2004/09/enumeration/Pull";
pub const ACTION_RELEASE: &str = "http://schemas.xmlsoap.org/ws/2004/09/enumeration/Release";
const MAX_ENVELOPE_SIZE: u32 = 512_000;
const OPERATION_TIMEOUT: &str = "PT60.000S";

/// Name/value pairs of a `SelectorSet`.
pub type Selectors = [(String, String)];

/// Escapes text for element content and attribute values. Characters XML 1.0 forbids become
/// `?`, as the service does.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || matches!(c, '\u{FFFE}' | '\u{FFFF}') => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

/// `http://host:port/wsman`, with brackets around an IPv6 literal.
pub fn endpoint_url(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("http://[{host}]:{port}/wsman")
    } else {
        format!("http://{host}:{port}/wsman")
    }
}

/// Resource URI of a class, or of every class (`*`) for a query. Backslashes in the namespace
/// become slashes and `root/cimv2` is lower case.
pub fn resource_uri(namespace: &str, class: &str) -> String {
    let namespace = namespace.trim_matches('\\').replace('\\', "/");
    let namespace = if namespace.eq_ignore_ascii_case("root/cimv2") {
        "root/cimv2".to_owned()
    } else {
        namespace
    };
    format!("{WMI_URI}{namespace}/{class}")
}

/// `uuid:` and an upper-case GUID built from 16 random bytes.
pub fn new_message_id() -> String {
    let mut bytes = [0u8; 16];
    let filled = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok();
    if !filled {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        bytes.copy_from_slice(&nanos.to_le_bytes());
    }
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Header values shared by every operation.
#[derive(Clone, Copy, Debug)]
pub struct Call<'a> {
    pub to: &'a str,
    pub resource_uri: &'a str,
    pub message_id: &'a str,
}

fn envelope(call: &Call, action: &str, selectors: &Selectors, body: &str) -> String {
    let mut out = String::with_capacity(1024 + body.len());
    out.push_str(concat!(
        r#"<s:Envelope xml:lang="en-US" xmlns:s="http://www.w3.org/2003/05/soap-envelope" "#,
        r#"xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" "#,
        r#"xmlns:n="http://schemas.xmlsoap.org/ws/2004/09/enumeration" "#,
        r#"xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" "#,
        r#"xmlns:p="http://schemas.microsoft.com/wbem/wsman/1/wsman.xsd"><s:Header>"#
    ));
    out.push_str(&format!("<a:To>{}</a:To>", escape(call.to)));
    out.push_str(&format!(
        r#"<w:ResourceURI s:mustUnderstand="true">{}</w:ResourceURI>"#,
        escape(call.resource_uri)
    ));
    out.push_str(&format!(
        r#"<a:ReplyTo><a:Address s:mustUnderstand="true">{ANONYMOUS}</a:Address></a:ReplyTo>"#
    ));
    out.push_str(&format!(
        r#"<a:Action s:mustUnderstand="true">{}</a:Action>"#,
        escape(action)
    ));
    out.push_str(&format!(
        r#"<w:MaxEnvelopeSize s:mustUnderstand="true">{MAX_ENVELOPE_SIZE}</w:MaxEnvelopeSize>"#
    ));
    out.push_str(&format!(
        "<a:MessageID>{}</a:MessageID>",
        escape(call.message_id)
    ));
    out.push_str(r#"<w:Locale xml:lang="en-US" s:mustUnderstand="false"/>"#);
    out.push_str(r#"<p:DataLocale xml:lang="en-US" s:mustUnderstand="false"/>"#);
    out.push_str(&selector_set(selectors));
    out.push_str(&format!(
        "<w:OperationTimeout>{OPERATION_TIMEOUT}</w:OperationTimeout></s:Header><s:Body>{body}</s:Body></s:Envelope>"
    ));
    out
}

fn selector_set(selectors: &Selectors) -> String {
    if selectors.is_empty() {
        return String::new();
    }
    let mut out = String::from("<w:SelectorSet>");
    for (name, value) in selectors {
        out.push_str(&format!(
            r#"<w:Selector Name="{}">{}</w:Selector>"#,
            escape(name),
            escape(value)
        ));
    }
    out.push_str("</w:SelectorSet>");
    out
}

/// Enumerate with optimized first page, `max_elements` per page and object-plus-EPR items.
/// A WQL text selects the query dialect; the call's resource URI must then end in `/*`.
pub fn enumerate(call: &Call, wql: Option<&str>, max_elements: u32) -> String {
    let filter = wql.map_or_else(String::new, |query| {
        format!(
            r#"<w:Filter Dialect="{WQL_DIALECT}">{}</w:Filter>"#,
            escape(query)
        )
    });
    let body = format!(
        "<n:Enumerate><w:OptimizeEnumeration/><w:MaxElements>{max_elements}</w:MaxElements>{filter}<w:EnumerationMode>EnumerateObjectAndEPR</w:EnumerationMode></n:Enumerate>"
    );
    envelope(call, ACTION_ENUMERATE, &[], &body)
}

/// Pull. `max_elements` must equal the value the enumeration started with.
pub fn pull(call: &Call, context: &str, max_elements: u32) -> String {
    let body = format!(
        "<n:Pull><n:EnumerationContext>{}</n:EnumerationContext><n:MaxElements>{max_elements}</n:MaxElements></n:Pull>",
        escape(context)
    );
    envelope(call, ACTION_PULL, &[], &body)
}

pub fn release(call: &Call, context: &str) -> String {
    let body = format!(
        "<n:Release><n:EnumerationContext>{}</n:EnumerationContext></n:Release>",
        escape(context)
    );
    envelope(call, ACTION_RELEASE, &[], &body)
}

/// Get. Singleton classes take no selectors.
pub fn get(call: &Call, selectors: &Selectors) -> String {
    envelope(call, ACTION_GET, selectors, "")
}

/// Invoke of `method` on the class or instance the call addresses. Inputs follow the order
/// given; null inputs are omitted.
pub fn invoke(
    call: &Call,
    namespace: &str,
    method: &str,
    selectors: &Selectors,
    inputs: &[Property],
) -> String {
    let action = format!("{}/{method}", call.resource_uri);
    let mut body = format!(
        r#"<p:{0}_INPUT xmlns:p="{1}" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:cim="http://schemas.dmtf.org/wbem/wscim/1/common">"#,
        escape(method),
        escape(call.resource_uri)
    );
    let mut prefixes = 0;
    for input in inputs {
        write_value(
            &mut body,
            "p",
            &input.name.to_string_lossy(),
            &input.value,
            namespace,
            &mut prefixes,
        );
    }
    body.push_str(&format!("</p:{}_INPUT>", escape(method)));
    envelope(call, &action, selectors, &body)
}

fn write_value(
    out: &mut String,
    prefix: &str,
    name: &str,
    value: &CimValue,
    namespace: &str,
    prefixes: &mut u32,
) {
    let tag = escape(name);
    match value {
        CimValue::Null | CimValue::Empty => {}
        CimValue::Array(items) => {
            for item in items {
                write_value(out, prefix, name, item, namespace, prefixes);
            }
        }
        CimValue::DateTime(text) => {
            let text = text.to_string_lossy();
            let inner = if let Some(xs) = cim_to_xs_datetime(&text) {
                format!("<cim:Datetime>{xs}</cim:Datetime>")
            } else if let Some(xs) = cim_to_xs_duration(&text) {
                format!("<cim:Interval>{xs}</cim:Interval>")
            } else if is_cim_datetime(&text) || text.contains('*') {
                format!("<cim:CIM_DateTime>{}</cim:CIM_DateTime>", escape(&text))
            } else {
                escape(&text)
            };
            out.push_str(&format!("<{prefix}:{tag}>{inner}</{prefix}:{tag}>"));
        }
        CimValue::Reference(path) => {
            let text = path.to_string_lossy();
            match parse_object_path(&text) {
                Some(parsed) => {
                    let ns = parsed.namespace.as_deref().unwrap_or(namespace);
                    let uri = resource_uri(ns, &parsed.class);
                    out.push_str(&format!(
                        "<{prefix}:{tag}><a:Address>{ANONYMOUS}</a:Address><a:ReferenceParameters><w:ResourceURI>{}</w:ResourceURI>{}</a:ReferenceParameters></{prefix}:{tag}>",
                        escape(&uri),
                        selector_set(&parsed.selectors())
                    ));
                }
                None => out.push_str(&format!(
                    "<{prefix}:{tag}>{}</{prefix}:{tag}>",
                    escape(&text)
                )),
            }
        }
        CimValue::Object(record) => {
            *prefixes += 1;
            let own = format!("e{prefixes}");
            let class = record.class.to_string_lossy();
            out.push_str(&format!(
                r#"<{prefix}:{tag} xmlns:{own}="{}" xsi:type="{own}:{}_Type">"#,
                escape(&resource_uri(namespace, &class)),
                escape(&class)
            ));
            for property in record.properties.iter().filter(|p| !p.system) {
                write_value(
                    out,
                    &own,
                    &property.name.to_string_lossy(),
                    &property.value,
                    namespace,
                    prefixes,
                );
            }
            out.push_str(&format!("</{prefix}:{tag}>"));
        }
        scalar => {
            if let Some(text) = value_text(scalar) {
                out.push_str(&format!(
                    "<{prefix}:{tag}>{}</{prefix}:{tag}>",
                    escape(&text)
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Record;

    fn call<'a>(uri: &'a str) -> Call<'a> {
        Call {
            to: "http://host:5985/wsman",
            resource_uri: uri,
            message_id: "uuid:1",
        }
    }

    fn property(name: &str, value: CimValue) -> Property {
        Property {
            name: name.into(),
            value,
            cim_type: 8,
            system: false,
            qualifiers: vec![],
        }
    }

    #[test]
    fn resource_uris_follow_the_namespace() {
        assert_eq!(
            resource_uri("ROOT\\CIMV2", "Win32_Process"),
            "http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Win32_Process"
        );
        assert_eq!(
            resource_uri("\\root\\Microsoft\\Windows\\Storage", "*"),
            "http://schemas.microsoft.com/wbem/wsman/1/wmi/root/Microsoft/Windows/Storage/*"
        );
    }

    #[test]
    fn endpoint_brackets_ipv6_and_ids_are_guids() {
        assert_eq!(endpoint_url("h", 5985), "http://h:5985/wsman");
        assert_eq!(endpoint_url("fe80::1", 5985), "http://[fe80::1]:5985/wsman");
        let id = new_message_id();
        assert_eq!(id.len(), 5 + 36);
        assert!(
            id.starts_with("uuid:") && id[5..].chars().all(|c| c == '-' || c.is_ascii_hexdigit())
        );
        assert_ne!(id, new_message_id());
    }

    #[test]
    fn escape_covers_markup_and_forbidden_controls() {
        assert_eq!(escape("a&b<c>\"d'\u{1}"), "a&amp;b&lt;c&gt;&quot;d&apos;?");
    }

    #[test]
    fn enumerate_wql_carries_filter_and_mode() {
        let xml = enumerate(
            &call("http://x/*"),
            Some("SELECT * FROM A WHERE B='a&b'"),
            32,
        );
        assert!(xml.contains("<w:MaxElements>32</w:MaxElements>"));
        assert!(xml.contains("B=&apos;a&amp;b&apos;</w:Filter>"));
        assert!(xml.contains("EnumerateObjectAndEPR"));
        assert!(xml.contains("<w:MaxEnvelopeSize s:mustUnderstand=\"true\">512000<"));
        roxmltree::Document::parse(&xml).unwrap();
    }

    #[test]
    fn invoke_binds_parameters_and_embedded_objects() {
        let startup = Record {
            class: "Win32_ProcessStartup".into(),
            properties: vec![property("ShowWindow", CimValue::Uint16(0))],
            ..Default::default()
        };
        let inputs = [
            property("CommandLine", CimValue::String("cmd /c \"x\" & y".into())),
            property("Skipped", CimValue::Null),
            property(
                "Many",
                CimValue::Array(vec![CimValue::Uint8(1), CimValue::Uint8(2)]),
            ),
            property(
                "ProcessStartupInformation",
                CimValue::Object(Box::new(startup)),
            ),
            property(
                "When",
                CimValue::DateTime("20261013154708.097097-240".into()),
            ),
        ];
        let uri = resource_uri("root\\cimv2", "Win32_Process");
        let xml = invoke(&call(&uri), "root\\cimv2", "Create", &[], &inputs);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert!(xml.contains("/Win32_Process/Create</a:Action>"));
        assert!(!xml.contains("Skipped"));
        assert!(xml.contains("<p:Many>1</p:Many><p:Many>2</p:Many>"));
        assert!(xml.contains("&quot;x&quot; &amp; y"));
        assert!(xml.contains(r#"xsi:type="e1:Win32_ProcessStartup_Type""#));
        assert!(xml.contains("<e1:ShowWindow>0</e1:ShowWindow>"));
        assert!(xml.contains("<cim:Datetime>2026-10-13T15:47:08.097097-04:00</cim:Datetime>"));
        assert_eq!(
            doc.descendants()
                .filter(|n| n.tag_name().name() == "Create_INPUT")
                .count(),
            1
        );
    }

    #[test]
    fn reference_inputs_become_endpoint_references() {
        let inputs = [property(
            "Target",
            CimValue::Reference(r#"\\H\ROOT\CIMV2:Win32_Directory.Name="C:\\Windows""#.into()),
        )];
        let uri = resource_uri("root\\cimv2", "Win32_Directory");
        let xml = invoke(&call(&uri), "root\\cimv2", "Copy", &[], &inputs);
        assert!(xml.contains("<w:ResourceURI>http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Win32_Directory</w:ResourceURI>"));
        assert!(xml.contains(r#"<w:Selector Name="Name">C:\Windows</w:Selector>"#));
    }

    #[test]
    fn get_and_release_shapes() {
        let selectors = [("Handle".to_owned(), "4".to_owned())];
        let xml = get(&call("http://x/Win32_Process"), &selectors);
        assert!(xml.contains(
            r#"<w:SelectorSet><w:Selector Name="Handle">4</w:Selector></w:SelectorSet>"#
        ));
        assert!(xml.contains("<s:Body></s:Body>"));
        let xml = release(&call("http://x/C"), "uuid:A");
        assert!(xml.contains(
            "<n:Release><n:EnumerationContext>uuid:A</n:EnumerationContext></n:Release>"
        ));
    }
}
