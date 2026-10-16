//! Reply parsing. Elements match on namespace-qualified local names; prefixes vary on the wire.
use super::{fault::Fault, soap::WMI_URI};
use roxmltree::{Document, Node};

const NS_SOAP: &str = "http://www.w3.org/2003/05/soap-envelope";
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// A reply that carries no usable result.
#[derive(Debug)]
pub enum ReplyError {
    Fault(Box<Fault>),
    Malformed(String),
}

/// Endpoint reference of an instance: its resource URI and key selectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Epr {
    pub resource_uri: String,
    pub selectors: Vec<(String, String)>,
}

impl Epr {
    /// Class name: the last segment of the resource URI.
    pub fn class(&self) -> &str {
        self.resource_uri
            .rsplit_once('/')
            .map_or(self.resource_uri.as_str(), |(_, class)| class)
    }

    /// Namespace with `/` separators, for example `root/cimv2`.
    pub fn namespace(&self) -> Option<String> {
        namespace_of(&self.resource_uri)
    }
}

/// Namespace part of a WMI resource URI or element namespace URI.
fn namespace_of(uri: &str) -> Option<String> {
    let path = uri.strip_prefix(WMI_URI)?;
    path.rsplit_once('/')
        .map(|(namespace, _)| namespace.to_owned())
}

#[derive(Clone, Debug, PartialEq)]
pub enum RawValue {
    Nil,
    Text(String),
    DateTime(String),
    Interval(String),
    Date(String),
    Time(String),
    /// A timestamp already in CIM form, sent when wildcards make `xs:dateTime` unusable.
    CimDateTime(String),
    Reference(Epr),
    Object(RawInstance),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawField {
    pub name: String,
    pub value: RawValue,
}

/// An instance or method output as it appears on the wire. Arrays are repeated fields.
#[derive(Clone, Debug, PartialEq)]
pub struct RawInstance {
    /// Empty for projection fragments.
    pub class: String,
    pub namespace: Option<String>,
    pub fragment: bool,
    pub fields: Vec<RawField>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawItem {
    pub instance: Option<RawInstance>,
    pub epr: Option<Epr>,
}

#[derive(Debug, Default)]
pub struct EnumerationPage {
    pub items: Vec<RawItem>,
    /// Present while the server holds the enumeration.
    pub context: Option<String>,
    pub end_of_sequence: bool,
}

fn is_element(node: &Node) -> bool {
    node.is_element()
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

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|c| c.is_element() && local(*c) == name)
}

fn is_nil(node: Node) -> bool {
    node.attribute((NS_XSI, "nil"))
        .is_some_and(|v| v == "true" || v == "1")
}

fn malformed(error: impl std::fmt::Display) -> ReplyError {
    ReplyError::Malformed(error.to_string())
}

/// First element in the body, or `None` for an empty body. A fault becomes an error.
fn with_body<T>(
    xml: &str,
    read: impl FnOnce(Option<Node>) -> Result<T, ReplyError>,
) -> Result<T, ReplyError> {
    let doc = Document::parse(xml).map_err(malformed)?;
    let envelope = doc.root_element();
    if local(envelope) != "Envelope" {
        return Err(ReplyError::Malformed("reply is not a SOAP envelope".into()));
    }
    let body = child(envelope, "Body")
        .ok_or_else(|| ReplyError::Malformed("reply has no SOAP body".into()))?;
    let first = body.children().find(is_element);
    if let Some(node) = first
        && local(node) == "Fault"
        && node.tag_name().namespace() == Some(NS_SOAP)
    {
        return Err(ReplyError::Fault(Box::new(Fault::from_node(node))));
    }
    read(first)
}

/// Parses a SOAP fault out of a reply body, if the body holds one.
pub fn parse_fault(xml: &str) -> Option<Fault> {
    match with_body(xml, |_| Ok(())) {
        Err(ReplyError::Fault(fault)) => Some(*fault),
        _ => None,
    }
}

fn epr_of(node: Node) -> Epr {
    let resource_uri = node
        .descendants()
        .find(|n| n.is_element() && local(*n) == "ResourceURI")
        .map(|n| text_of(n).trim().to_owned())
        .unwrap_or_default();
    let selectors = node
        .descendants()
        .filter(|n| n.is_element() && local(*n) == "Selector")
        .filter_map(|n| Some((n.attribute("Name")?.to_owned(), text_of(n))))
        .collect();
    Epr {
        resource_uri,
        selectors,
    }
}

/// Class and namespace named by `xsi:type`, when it names a WMI class type.
fn declared_type(node: Node) -> Option<(String, Option<String>)> {
    let value = node.attribute((NS_XSI, "type"))?;
    let (prefix, name) = value.split_once(':').unwrap_or(("", value));
    let class = name.strip_suffix("_Type")?;
    let uri = node.lookup_namespace_uri(Some(prefix).filter(|p| !p.is_empty()))?;
    uri.starts_with(WMI_URI)
        .then(|| (class.to_owned(), namespace_of(uri)))
}

fn value_of(node: Node) -> RawValue {
    if is_nil(node) {
        return RawValue::Nil;
    }
    if let Some((class, namespace)) = declared_type(node) {
        return RawValue::Object(read_instance(node, class, namespace, false));
    }
    let children: Vec<Node> = node.children().filter(is_element).collect();
    if children.is_empty() {
        return RawValue::Text(text_of(node));
    }
    if let [only] = children.as_slice() {
        let text = text_of(*only);
        match local(*only) {
            "Datetime" => return RawValue::DateTime(text),
            "Interval" => return RawValue::Interval(text),
            "Date" => return RawValue::Date(text),
            "Time" => return RawValue::Time(text),
            "CIM_DateTime" => return RawValue::CimDateTime(text),
            _ => {}
        }
    }
    if children
        .iter()
        .any(|c| matches!(local(*c), "Address" | "ReferenceParameters"))
    {
        return RawValue::Reference(epr_of(node));
    }
    let class = local(node).to_owned();
    RawValue::Object(read_instance(node, class, None, false))
}

fn read_instance(
    node: Node,
    class: String,
    namespace: Option<String>,
    fragment: bool,
) -> RawInstance {
    RawInstance {
        class,
        namespace,
        fragment,
        fields: node
            .children()
            .filter(is_element)
            .map(|field| RawField {
                name: local(field).to_owned(),
                value: value_of(field),
            })
            .collect(),
    }
}

fn top_instance(node: Node) -> RawInstance {
    if local(node) == "XmlFragment" {
        return read_instance(node, String::new(), None, true);
    }
    let namespace = node.tag_name().namespace().and_then(namespace_of);
    read_instance(node, local(node).to_owned(), namespace, false)
}

fn item_of(node: Node) -> RawItem {
    match local(node) {
        "EndpointReference" => RawItem {
            instance: None,
            epr: Some(epr_of(node)),
        },
        "Item" => RawItem {
            instance: node
                .children()
                .find(|c| c.is_element() && local(*c) != "EndpointReference")
                .map(top_instance),
            epr: child(node, "EndpointReference").map(epr_of),
        },
        _ => RawItem {
            instance: Some(top_instance(node)),
            epr: None,
        },
    }
}

/// Reads an `EnumerateResponse` or `PullResponse`.
pub fn parse_enumeration(xml: &str) -> Result<EnumerationPage, ReplyError> {
    with_body(xml, |first| {
        let response = first
            .filter(|n| matches!(local(*n), "EnumerateResponse" | "PullResponse"))
            .ok_or_else(|| ReplyError::Malformed("reply is not an enumeration response".into()))?;
        let mut page = EnumerationPage::default();
        for part in response.children().filter(is_element) {
            match local(part) {
                "EnumerationContext" => {
                    let context = text_of(part).trim().to_owned();
                    page.context = Some(context).filter(|c| !c.is_empty());
                }
                "EndOfSequence" => page.end_of_sequence = true,
                "Items" => page
                    .items
                    .extend(part.children().filter(is_element).map(item_of)),
                _ => {}
            }
        }
        Ok(page)
    })
}

/// Reads the instance of a `GetResponse`.
pub fn parse_get(xml: &str) -> Result<RawInstance, ReplyError> {
    with_body(xml, |first| {
        first
            .map(top_instance)
            .ok_or_else(|| ReplyError::Malformed("reply has no instance".into()))
    })
}

/// Reads the `Method_OUTPUT` element of an Invoke reply.
pub fn parse_invoke(xml: &str) -> Result<RawInstance, ReplyError> {
    with_body(xml, |first| {
        let output = first
            .filter(|n| local(*n).ends_with("_OUTPUT"))
            .ok_or_else(|| ReplyError::Malformed("reply has no method output".into()))?;
        let mut instance = top_instance(output);
        instance.class = instance.class.trim_end_matches("_OUTPUT").to_owned();
        Ok(instance)
    })
}

/// Checks a reply that carries no data, such as the answer to Release.
pub fn parse_empty(xml: &str) -> Result<(), ReplyError> {
    with_body(xml, |_| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:n="http://schemas.xmlsoap.org/ws/2004/09/enumeration" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><s:Body>"#;

    fn wrap(body: &str) -> String {
        format!("{HEAD}{body}</s:Body></s:Envelope>")
    }

    #[test]
    fn pull_response_with_items_and_context() {
        let xml = wrap(
            r#"<n:PullResponse><n:EnumerationContext>uuid:ABC</n:EnumerationContext><n:Items><w:Item><p:C xmlns:p="http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/C"><p:A>1</p:A><p:A>2</p:A><p:N xsi:nil="true"/></p:C><a:EndpointReference><a:Address>x</a:Address><a:ReferenceParameters><w:ResourceURI>http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/C</w:ResourceURI><w:SelectorSet><w:Selector Name="K">v &amp; w</w:Selector></w:SelectorSet></a:ReferenceParameters></a:EndpointReference></w:Item></n:Items></n:PullResponse>"#,
        );
        let page = parse_enumeration(&xml).unwrap();
        assert_eq!(page.context.as_deref(), Some("uuid:ABC"));
        assert!(!page.end_of_sequence);
        let item = &page.items[0];
        let epr = item.epr.as_ref().unwrap();
        assert_eq!(epr.class(), "C");
        assert_eq!(epr.namespace().as_deref(), Some("root/cimv2"));
        assert_eq!(epr.selectors, vec![("K".into(), "v & w".into())]);
        let instance = item.instance.as_ref().unwrap();
        assert_eq!(instance.class, "C");
        assert_eq!(instance.fields.len(), 3);
        assert_eq!(instance.fields[2].value, RawValue::Nil);
    }

    #[test]
    fn empty_context_and_end_of_sequence() {
        let xml = wrap(
            r#"<n:EnumerateResponse><n:EnumerationContext></n:EnumerationContext><w:EndOfSequence/></n:EnumerateResponse>"#,
        );
        let page = parse_enumeration(&xml).unwrap();
        assert!(page.context.is_none() && page.end_of_sequence && page.items.is_empty());
    }

    #[test]
    fn fragment_children_have_no_class() {
        let xml = wrap(
            r#"<n:EnumerateResponse><w:Items><w:XmlFragment><Name>System</Name><CreationDate><Datetime>2026-10-13T15:47:11.87422-04:00</Datetime></CreationDate></w:XmlFragment></w:Items></n:EnumerateResponse>"#,
        );
        let page = parse_enumeration(&xml).unwrap();
        let instance = page.items[0].instance.as_ref().unwrap();
        assert!(instance.fragment && instance.class.is_empty());
        assert!(matches!(instance.fields[1].value, RawValue::DateTime(_)));
    }

    #[test]
    fn embedded_objects_resolve_xsi_type_through_scope() {
        let xml = wrap(
            r#"<p:M_OUTPUT xmlns:p="http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/C"><p:D xmlns:p1="http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Win32_SecurityDescriptor" xsi:type="p1:Win32_SecurityDescriptor_Type"><p1:ControlFlags>1</p1:ControlFlags></p:D><p:ReturnValue>0</p:ReturnValue></p:M_OUTPUT>"#,
        );
        let output = parse_invoke(&xml).unwrap();
        assert_eq!(output.class, "M");
        let RawValue::Object(object) = &output.fields[0].value else {
            panic!("not an object")
        };
        assert_eq!(object.class, "Win32_SecurityDescriptor");
        assert_eq!(object.namespace.as_deref(), Some("root/cimv2"));
    }

    #[test]
    fn faults_and_bad_envelopes_are_errors() {
        let fault = wrap(
            r#"<s:Fault><s:Code><s:Value>s:Sender</s:Value></s:Code><s:Reason><s:Text>bad</s:Text></s:Reason></s:Fault>"#,
        );
        assert!(matches!(
            parse_enumeration(&fault),
            Err(ReplyError::Fault(_))
        ));
        assert!(parse_fault(&fault).is_some());
        assert!(matches!(
            parse_enumeration("<html/>"),
            Err(ReplyError::Malformed(_))
        ));
        assert!(matches!(
            parse_get("not xml"),
            Err(ReplyError::Malformed(_))
        ));
        assert!(parse_empty(&wrap("")).is_ok());
    }
}
