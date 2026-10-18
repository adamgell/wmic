#![cfg(not(windows))]
mod support;
use std::{cell::RefCell, collections::VecDeque, net::TcpListener, rc::Rc};
use support::{request, wide};
use wmic_compat::{
    backend::{
        Confirmation, Management, ResolvedRequest, SecretWide,
        wsman::{
            Authentication, Endpoint, Response, Transport, TransportError,
            client::{Target, WsmanClient, default_connector, listener_reachable},
            manage::WsmanManagement,
        },
    },
    model::*,
    render::OutputError,
    session::{OutputSink, OutputStream, Session},
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/wsman/");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}{name}.xml")).unwrap()
}

enum Reply {
    Xml(u16, String),
    Connect,
    Io,
    Authentication,
}

fn ok(name: &str) -> Reply {
    Reply::Xml(200, fixture(name))
}

fn fault(name: &str) -> Reply {
    Reply::Xml(500, fixture(name))
}

struct Rule {
    action: &'static str,
    resource: Option<&'static str>,
    replies: VecDeque<Reply>,
}

#[derive(Clone, Debug)]
struct Sent {
    action: String,
    resource: String,
    xml: String,
}

impl Sent {
    fn texts(&self, local: &str) -> Vec<String> {
        let doc = roxmltree::Document::parse(&self.xml).unwrap();
        doc.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == local)
            .map(|n| n.text().unwrap_or_default().to_owned())
            .collect()
    }

    fn selectors(&self) -> Vec<(String, String)> {
        let doc = roxmltree::Document::parse(&self.xml).unwrap();
        doc.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Selector")
            .map(|n| {
                (
                    n.attribute("Name").unwrap().to_owned(),
                    n.text().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    fn kind(&self) -> &str {
        self.action.rsplit('/').next().unwrap()
    }
}

#[derive(Default)]
struct Wire {
    rules: Vec<Rule>,
    sent: Vec<Sent>,
    connections: usize,
    /// Computer name the transports report after their first reply.
    server_name: Option<String>,
    /// Mechanism the transports report as their authentication.
    mechanism: Option<&'static str>,
    /// Number of upcoming reuse checks that find the connection closed by the peer.
    stale: usize,
}

type Shared = Rc<RefCell<Wire>>;

fn wire() -> Shared {
    Rc::default()
}

fn script(
    wire: &Shared,
    action: &'static str,
    resource: Option<&'static str>,
    replies: Vec<Reply>,
) {
    wire.borrow_mut().rules.push(Rule {
        action,
        resource,
        replies: replies.into(),
    });
}

fn sent(wire: &Shared) -> Vec<Sent> {
    wire.borrow().sent.clone()
}

fn kinds(wire: &Shared) -> Vec<String> {
    sent(wire).iter().map(|s| s.kind().to_owned()).collect()
}

struct FakeTransport(Shared);

impl Transport for FakeTransport {
    fn server_name(&self) -> Option<String> {
        self.0.borrow().server_name.clone()
    }

    fn authentication(&self) -> Option<Authentication> {
        self.0.borrow().mechanism.map(|mechanism| Authentication {
            mechanism,
            server_scheme: None,
        })
    }

    fn is_reusable(&mut self) -> bool {
        let mut wire = self.0.borrow_mut();
        let stale = wire.stale > 0;
        wire.stale = wire.stale.saturating_sub(1);
        !stale
    }

    fn post(&mut self, envelope: &str) -> Result<Response, TransportError> {
        let doc = roxmltree::Document::parse(envelope).expect("request is well-formed XML");
        let text = |local: &str| {
            doc.descendants()
                .find(|n| n.is_element() && n.tag_name().name() == local)
                .and_then(|n| n.text())
                .unwrap_or_default()
                .to_owned()
        };
        let message = Sent {
            action: text("Action"),
            resource: text("ResourceURI"),
            xml: envelope.to_owned(),
        };
        let mut wire = self.0.borrow_mut();
        wire.sent.push(message.clone());
        let rule = wire
            .rules
            .iter_mut()
            .find(|r| {
                message.action.ends_with(r.action)
                    && r.resource.is_none_or(|s| message.resource.ends_with(s))
                    && !r.replies.is_empty()
            })
            .unwrap_or_else(|| panic!("no scripted reply for {}", message.action));
        match rule.replies.pop_front().unwrap() {
            Reply::Xml(status, body) => Ok(Response { status, body }),
            Reply::Connect => Err(TransportError::Connect(std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused,
            ))),
            Reply::Io => Err(TransportError::Io(std::io::Error::from(
                std::io::ErrorKind::BrokenPipe,
            ))),
            Reply::Authentication => Err(TransportError::Authentication),
        }
    }
}

fn client(wire: &Shared) -> WsmanClient {
    let wire = wire.clone();
    WsmanClient::new(Box::new(move |_| {
        wire.borrow_mut().connections += 1;
        Ok(Box::new(FakeTransport(wire.clone())) as Box<dyn Transport>)
    }))
}

fn endpoint(port: u16) -> Endpoint {
    Endpoint {
        host: "AYW".into(),
        port,
        user: "admin".into(),
        domain: "AYW".into(),
        password: SecretWide::new(wide("secret")),
        tls: None,
        connect: None,
        authority: Default::default(),
    }
}

fn target() -> Target {
    Target {
        endpoint: endpoint(5985),
        namespace: "root\\cimv2".into(),
        node: wide("AYW"),
        server: "AYW".into(),
        multiple_nodes: false,
    }
}

fn management(wire: &Shared) -> WsmanManagement {
    WsmanManagement::with_client(client(wire))
}

/// A request for the node AYW with an account.
fn remote(text: &str) -> ResolvedRequest {
    let mut r = request(text);
    r.connection.node = wide("AYW");
    r.connection.user = Some(wide("AYW\\admin"));
    r.connection.password = Some(SecretWide::new(wide("secret")));
    r
}

fn property<'a>(record: &'a Record, name: &str) -> &'a Property {
    record
        .properties
        .iter()
        .find(|p| p.name.eq_ignore_case(name))
        .unwrap_or_else(|| panic!("record has no {name}"))
}

fn text(record: &Record, name: &str) -> String {
    match &property(record, name).value {
        CimValue::String(s) | CimValue::DateTime(s) | CimValue::Reference(s) => s.to_string_lossy(),
        other => panic!("{name} is {other:?}"),
    }
}

fn objects(result: ManagementResult) -> Vec<Record> {
    match result {
        ManagementResult::Objects(set) => set.objects,
        other => panic!("not objects: {other:?}"),
    }
}

fn plain_names(record: &Record) -> Vec<String> {
    record
        .properties
        .iter()
        .filter(|p| !p.system)
        .map(|p| p.name.to_string_lossy())
        .collect()
}

fn failure(error: ManagementError) -> (u32, String, String) {
    (
        error.code,
        error.message.to_string_lossy(),
        error.operation.to_string_lossy(),
    )
}

fn expect(code: u32, message: &str, operation: &str) -> (u32, String, String) {
    (code, message.into(), operation.into())
}

const CIMV2: &str = "/root/cimv2/";

#[test]
fn typed_operating_system_instance_with_paths_and_system_properties() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        Some("/root/cimv2/*"),
        vec![ok("enumerate-os")],
    );
    let result = management(&w).execute(&remote("os get")).unwrap();
    let records = objects(result);
    assert_eq!(records.len(), 1);
    let os = &records[0];
    assert_eq!(os.class, wide("Win32_OperatingSystem"));
    assert_eq!(
        os.path,
        Some(wide("\\\\AYW\\ROOT\\CIMV2:Win32_OperatingSystem=@"))
    );
    assert_eq!(os.server, Some(wide("AYW")));
    assert_eq!(
        plain_names(os)[0],
        "Caption",
        "schema order, not wire order"
    );
    assert_eq!(
        property(os, "Caption").value,
        CimValue::String(wide("Microsoft Windows 11 Pro"))
    );
    assert_eq!(
        property(os, "FreePhysicalMemory").value,
        CimValue::Uint64(7778204)
    );
    assert_eq!(property(os, "FreePhysicalMemory").cim_type, 21);
    assert_eq!(
        property(os, "LastBootUpTime").value,
        CimValue::DateTime(wide("20261013154708.097097-240"))
    );
    assert_eq!(property(os, "Primary").value, CimValue::Boolean(true));
    assert_eq!(
        property(os, "MUILanguages").value,
        CimValue::Array(vec![CimValue::String(wide("en-US"))])
    );
    assert_eq!(property(os, "MUILanguages").cim_type, 8 | 0x2000);
    assert_eq!(property(os, "CSDVersion").value, CimValue::Null);
    assert_eq!(
        property(os, "CurrentTimeZone").value,
        CimValue::Sint16(-240)
    );

    let system: Vec<_> = os.properties.iter().filter(|p| p.system).collect();
    let names: Vec<_> = system.iter().map(|p| p.name.to_string_lossy()).collect();
    assert_eq!(
        names,
        [
            "__PATH",
            "__NAMESPACE",
            "__SERVER",
            "__DERIVATION",
            "__PROPERTY_COUNT",
            "__RELPATH",
            "__DYNASTY",
            "__SUPERCLASS",
            "__CLASS",
            "__GENUS"
        ]
    );
    assert_eq!(text(os, "__NAMESPACE"), "ROOT\\CIMV2");
    assert_eq!(text(os, "__RELPATH"), "Win32_OperatingSystem=@");
    assert_eq!(text(os, "__SUPERCLASS"), "CIM_OperatingSystem");
    assert_eq!(text(os, "__DYNASTY"), "CIM_ManagedSystemElement");
    assert_eq!(property(os, "__GENUS").value, CimValue::Sint32(2));
    let CimValue::Array(derivation) = &property(os, "__DERIVATION").value else {
        panic!("derivation is not an array")
    };
    assert_eq!(derivation.len(), 3);
}

#[test]
fn projection_returns_requested_properties_and_system_properties_only() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        Some("/root/cimv2/*"),
        vec![ok("enumerate-os")],
    );
    let records = objects(
        management(&w)
            .execute(&remote("os get Caption,Primary"))
            .unwrap(),
    );
    assert_eq!(plain_names(&records[0]), ["Caption", "Primary"]);
    assert_eq!(
        records[0].properties.iter().filter(|p| p.system).count(),
        10
    );
    let query = sent(&w)[0].texts("Filter");
    assert!(
        query[0].starts_with("SELECT Caption,Primary FROM"),
        "{query:?}"
    );
}

fn class_metadata(record: &Record) -> (CimValue, CimValue, CimValue, CimValue) {
    (
        property(record, "__DERIVATION").value.clone(),
        property(record, "__PROPERTY_COUNT").value.clone(),
        property(record, "__DYNASTY").value.clone(),
        property(record, "__SUPERCLASS").value.clone(),
    )
}

#[test]
fn a_projection_reduces_the_class_metadata() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
    let records = objects(
        management(&w)
            .execute(&remote(
                "process where \"ProcessId=4\" get Name,ProcessId,CommandLine,CreationDate",
            ))
            .unwrap(),
    );
    let (derivation, count, dynasty, superclass) = class_metadata(&records[0]);
    assert_eq!(derivation, CimValue::Array(vec![]));
    assert_eq!(
        count,
        CimValue::Sint32(5),
        "the four named properties and the key Handle the server added"
    );
    assert_eq!(dynasty, CimValue::String(wide("")));
    assert_eq!(superclass, CimValue::String(wide("")));
    for name in ["__PATH", "__RELPATH", "__NAMESPACE", "__SERVER", "__CLASS"] {
        assert!(
            matches!(&property(&records[0], name).value, CimValue::String(s) if !s.0.is_empty()),
            "{name}"
        );
    }
    assert_eq!(property(&records[0], "__GENUS").value, CimValue::Sint32(2));
}

#[test]
fn a_projection_fills_the_derivation_and_superclass_it_names() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
    let records = objects(
        management(&w)
            .execute(&remote(
                "process where \"ProcessId=4\" get Name,__DERIVATION,__SUPERCLASS,__DYNASTY",
            ))
            .unwrap(),
    );
    let (derivation, count, dynasty, superclass) = class_metadata(&records[0]);
    assert_eq!(
        derivation,
        CimValue::Array(
            [
                "CIM_Process",
                "CIM_LogicalElement",
                "CIM_ManagedSystemElement"
            ]
            .map(|class| CimValue::String(wide(class)))
            .to_vec()
        )
    );
    assert_eq!(superclass, CimValue::String(wide("CIM_Process")));
    assert_eq!(dynasty, CimValue::String(wide("")));
    assert_eq!(count, CimValue::Sint32(2), "Name and the Handle key only");
}

#[test]
fn a_command_without_a_projection_keeps_the_class_metadata() {
    for command in ["os get", "os get *", "os list full"] {
        let w = wire();
        script(
            &w,
            "/Enumerate",
            Some("/root/cimv2/*"),
            vec![ok("enumerate-os")],
        );
        let records = objects(management(&w).execute(&remote(command)).unwrap());
        let (derivation, count, dynasty, superclass) = class_metadata(&records[0]);
        assert!(
            matches!(derivation, CimValue::Array(items) if items.len() == 3),
            "{command}"
        );
        assert_ne!(count, CimValue::Sint32(0), "{command}");
        assert_eq!(dynasty, CimValue::String(wide("CIM_ManagedSystemElement")));
        assert_eq!(superclass, CimValue::String(wide("CIM_OperatingSystem")));
    }
}

/// Stdout and stderr of one command run through the session against the scripted wire.
fn session_output(w: &Shared, command: &str) -> (String, String, u32) {
    struct Capture(String, String);
    impl OutputSink for Capture {
        fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
            match stream {
                OutputStream::Stdout => self.0.push_str(&text.to_string_lossy()),
                OutputStream::Stderr => self.1.push_str(&text.to_string_lossy()),
            }
            Ok(())
        }
    }
    let mut capture = Capture(String::new(), String::new());
    let outcome = Session::new(wide("LOCAL")).unwrap().execute(
        &wide(&format!(
            " /node:AYW /user:AYW\\admin /password:secret {command}"
        )),
        EntryMode::Interactive,
        &mut management(w),
        &mut capture,
    );
    (capture.0, capture.1, outcome.exit_status)
}

// Case 03 of the remote measurement: the system properties of a projection that names none of the
// class properties. The reply is what a server returns for a query with system properties only.
#[test]
fn a_system_property_list_matches_the_measured_classic_output() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-os-system-only")]);
    let (stdout, stderr, status) =
        session_output(&w, "os get __SERVER,__PATH,__NAMESPACE /format:list");
    let measured = include_str!("fixtures/remote-reference/system-properties-list.stdout.bin")
        .replace("NODE1", "AYW");
    assert_eq!(stdout, measured);
    assert_eq!((stderr.as_str(), status), ("", 0));
}

// Case E15: an alias carries its own namespace, so the switch does not change it. A path target
// uses the switch.
#[test]
fn the_namespace_switch_does_not_change_an_alias_target() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-os")]);
    let (stdout, stderr, status) = session_output(&w, r"/namespace:\\root\nosuchns os get Caption");
    assert!(stdout.starts_with("Caption"), "{stdout}");
    assert_eq!((stderr.as_str(), status), ("", 0));
    assert!(
        sent(&w)[0].resource.ends_with("/root/cimv2/*"),
        "{:?}",
        sent(&w)
    );

    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![fault("fault-unknown-namespace")],
    );
    let (stdout, stderr, status) = session_output(
        &w,
        r"/namespace:\\root\nosuchns path Win32_OperatingSystem get Caption",
    );
    assert!(
        sent(&w)[0].resource.contains("/root/nosuchns/"),
        "{:?}",
        sent(&w)
    );
    assert_eq!(stdout, "");
    assert_eq!(stderr, "ERROR:\r\r\nDescription = Invalid namespace\r\r\n");
    assert_eq!(status, 0x8004100e);
}

#[test]
fn wql_projection_fragment_gets_a_path_and_drops_server_added_keys() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
    let records = objects(
        management(&w)
            .execute(&remote(
                "process where \"ProcessId=4\" get Name,ProcessId,CommandLine,CreationDate",
            ))
            .unwrap(),
    );
    let message = &sent(&w)[0];
    assert!(message.resource.ends_with("/root/cimv2/*"));
    assert_eq!(
        message.texts("Filter")[0].to_ascii_uppercase(),
        "SELECT NAME,PROCESSID,COMMANDLINE,CREATIONDATE FROM WIN32_PROCESS WHERE PROCESSID=4"
    );
    assert!(message.xml.contains("EnumerateObjectAndEPR"));
    let process = &records[0];
    let mut names = plain_names(process);
    names.sort();
    assert_eq!(names, ["CommandLine", "CreationDate", "Name", "ProcessId"]);
    assert_eq!(property(process, "ProcessId").value, CimValue::Uint32(4));
    assert_eq!(property(process, "CommandLine").value, CimValue::Null);
    assert_eq!(
        property(process, "CreationDate").value,
        CimValue::DateTime(wide("20261013154711.874220-240"))
    );
    assert_eq!(
        process.path,
        Some(wide("\\\\AYW\\ROOT\\CIMV2:Win32_Process.Handle=\"4\""))
    );
    assert_eq!(text(process, "__CLASS"), "Win32_Process");
}

#[test]
fn enumerate_pulls_to_the_end_with_the_same_max_elements() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(
        &w,
        "/Pull",
        None,
        vec![ok("pull-driver-page2"), ok("pull-driver-page3")],
    );
    let found = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap();
    let paths: Vec<_> = found
        .iter()
        .map(|f| f.record.path.clone().unwrap().to_string_lossy())
        .collect();
    assert_eq!(
        paths,
        [
            "\\\\AYW\\ROOT\\CIMV2:Win32_SystemDriver.Name=\"1394ohci\"",
            "\\\\AYW\\ROOT\\CIMV2:Win32_SystemDriver.Name=\"acpi\"",
            "\\\\AYW\\ROOT\\CIMV2:Win32_SystemDriver.Name=\"afd\"",
        ]
    );
    assert_eq!(kinds(&w), ["Enumerate", "Pull", "Pull"]);
    let messages = sent(&w);
    assert!(
        messages[0]
            .resource
            .ends_with("/root/cimv2/Win32_SystemDriver")
    );
    let limits: Vec<_> = messages.iter().map(|m| m.texts("MaxElements")).collect();
    assert_eq!(limits, [["32"], ["32"], ["32"]]);
    assert_eq!(
        messages[1].texts("EnumerationContext"),
        ["uuid:1AE55BF9-E8C1-4BF1-B354-DCA14D1042FC"]
    );
    assert_eq!(
        messages[2].texts("EnumerationContext"),
        ["uuid:2BF66CA0-F9D2-5CF2-C465-EDB25E153ED"]
    );
    assert_eq!(messages[0].texts("MessageID").len(), 1);
    assert_ne!(
        messages[0].texts("MessageID"),
        messages[1].texts("MessageID")
    );
    assert_eq!(
        found[0].selectors,
        [("Name".to_owned(), "1394ohci".to_owned())]
    );
}

#[test]
fn empty_result_is_not_an_error() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-empty")]);
    let found = client(&w)
        .enumerate(
            &target(),
            "Win32_Process",
            Some("SELECT * FROM Win32_Process WHERE 0=1"),
        )
        .unwrap();
    assert!(found.is_empty());
    assert_eq!(kinds(&w), ["Enumerate"]);
}

#[test]
fn a_failing_pull_sends_release_and_keeps_the_partial_objects() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(&w, "/Pull", None, vec![fault("fault-pull-parameters")]);
    script(&w, "/Release", None, vec![ok("release")]);
    let error = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap_err();
    assert_eq!(
        failure(error.clone()),
        expect(0x80041001, "Generic failure", "query")
    );
    assert!(!error.extended.is_empty());
    let Some(partial) = error.partial.as_deref() else {
        panic!("no partial result")
    };
    let ManagementResult::Objects(set) = partial else {
        panic!("partial is not objects")
    };
    assert_eq!(set.objects.len(), 1);
    assert_eq!(kinds(&w), ["Enumerate", "Pull", "Release"]);
    assert_eq!(
        sent(&w)[2].texts("EnumerationContext"),
        ["uuid:1AE55BF9-E8C1-4BF1-B354-DCA14D1042FC"]
    );
}

#[test]
fn a_stale_context_restarts_the_enumeration_once_without_release() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![ok("enumerate-driver-page1"), ok("enumerate-driver-page1")],
    );
    script(
        &w,
        "/Pull",
        None,
        vec![
            fault("fault-stale-context"),
            ok("pull-driver-page2"),
            ok("pull-driver-page3"),
        ],
    );
    let found = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap();
    assert_eq!(found.len(), 3);
    assert_eq!(
        kinds(&w),
        ["Enumerate", "Pull", "Enumerate", "Pull", "Pull"]
    );

    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![ok("enumerate-driver-page1"), ok("enumerate-driver-page1")],
    );
    script(
        &w,
        "/Pull",
        None,
        vec![fault("fault-stale-context"), fault("fault-stale-context")],
    );
    let error = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap_err();
    assert_eq!(error.code, 0x80041001);
    assert_eq!(kinds(&w), ["Enumerate", "Pull", "Enumerate", "Pull"]);
}

static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[test]
fn an_interrupt_between_pulls_releases_and_returns_the_partial_objects() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(&w, "/Release", None, vec![ok("release")]);
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = client(&w)
        .with_interrupt(|| STOP.load(std::sync::atomic::Ordering::SeqCst))
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap_err();
    assert_eq!(error.code, 0xC000013A);
    assert_eq!(error.operation, wide("cancel"));
    assert!(matches!(
        error.partial.as_deref(),
        Some(ManagementResult::Objects(set)) if set.objects.len() == 1
    ));
    assert_eq!(kinds(&w), ["Enumerate", "Release"]);
}

#[test]
fn a_call_whose_selection_fails_carries_no_partial_objects() {
    // The session renders partial Objects of a failed node as a table; a CALL has no table.
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(&w, "/Pull", None, vec![fault("fault-pull-parameters")]);
    script(&w, "/Release", None, vec![ok("release")]);
    let error = management(&w)
        .execute(&remote("process where \"Handle='4'\" call getowner"))
        .unwrap_err();
    assert_eq!(error.code, 0x80041001);
    assert!(error.partial.is_none());
    assert_eq!(kinds(&w), ["Enumerate", "Pull", "Release"]);
}

#[test]
fn get_by_selector_builds_the_path_from_the_selectors() {
    let w = wire();
    script(&w, "/Get", None, vec![ok("get-process-4")]);
    let records = objects(
        management(&w)
            .execute(&remote("path Win32_Process.Handle=\"4\" get Name"))
            .unwrap(),
    );
    let message = &sent(&w)[0];
    assert!(message.resource.ends_with("/root/cimv2/Win32_Process"));
    assert_eq!(message.selectors(), [("Handle".to_owned(), "4".to_owned())]);
    assert_eq!(plain_names(&records[0]), ["Name"]);
    assert_eq!(text(&records[0], "Name"), "System");
    assert_eq!(text(&records[0], "__RELPATH"), "Win32_Process.Handle=\"4\"");
}

#[test]
fn a_path_projection_counts_only_named_properties_and_keys() {
    let w = wire();
    script(&w, "/Get", None, vec![ok("get-process-4")]);
    let records = objects(
        management(&w)
            .execute(&remote("path Win32_Process.Handle=\"4\" get nAmE"))
            .unwrap(),
    );
    assert_eq!(plain_names(&records[0]), ["Name"]);
    assert_eq!(
        property(&records[0], "__PROPERTY_COUNT").value,
        CimValue::Sint32(2)
    );
    assert_eq!(text(&records[0], "__RELPATH"), "Win32_Process.Handle=\"4\"");
}

#[test]
fn a_path_wildcard_keeps_all_properties_and_full_class_metadata() {
    let w = wire();
    script(&w, "/Get", None, vec![ok("get-process-4")]);
    let records = objects(
        management(&w)
            .execute(&remote("path Win32_Process.Handle=\"4\" get *"))
            .unwrap(),
    );
    assert_eq!(plain_names(&records[0]).len(), 45);
    assert_eq!(
        property(&records[0], "__PROPERTY_COUNT").value,
        CimValue::Sint32(45)
    );
    assert_eq!(text(&records[0], "__DYNASTY"), "CIM_ManagedSystemElement");
}

#[test]
fn an_unknown_class_projection_counts_its_path_keys_without_displaying_them() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![enumerate_reply(&item(
            "Test_Widget",
            "<p:Id>42</p:Id><p:Label>example</p:Label><p:Other>unused</p:Other>",
            &[("Id", "42")],
        ))],
    );
    let records = objects(
        management(&w)
            .execute(&remote("path Test_Widget get Label"))
            .unwrap(),
    );
    assert_eq!(plain_names(&records[0]), ["Label"]);
    assert_eq!(
        property(&records[0], "__PROPERTY_COUNT").value,
        CimValue::Sint32(2)
    );
    assert_eq!(text(&records[0], "__RELPATH"), "Test_Widget.Id=\"42\"");
}

#[test]
fn key_values_with_backslashes_keep_one_backslash_on_the_wire_and_two_in_the_path() {
    let w = wire();
    script(&w, "/Get", None, vec![ok("get-directory")]);
    let records = objects(
        management(&w)
            .execute(&remote(
                "path Win32_Directory.Name=\"C:\\\\Windows\" get Name",
            ))
            .unwrap(),
    );
    assert_eq!(
        sent(&w)[0].selectors(),
        [("Name".to_owned(), "C:\\Windows".to_owned())]
    );
    assert_eq!(
        records[0].path,
        Some(wide(
            "\\\\AYW\\ROOT\\CIMV2:Win32_Directory.Name=\"C:\\\\Windows\""
        ))
    );
}

fn enumerate_reply(items: &str) -> Reply {
    Reply::Xml(
        200,
        format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:n="http://schemas.xmlsoap.org/ws/2004/09/enumeration" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:cim="http://schemas.dmtf.org/wbem/wscim/1/common"><s:Body><n:EnumerateResponse><n:EnumerationContext></n:EnumerationContext><w:Items>{items}</w:Items><w:EndOfSequence/></n:EnumerateResponse></s:Body></s:Envelope>"#
        ),
    )
}

fn item(class: &str, body: &str, selectors: &[(&str, &str)]) -> String {
    let uri = format!("http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/{class}");
    let selector_set: String = if selectors.is_empty() {
        String::new()
    } else {
        let inner: String = selectors
            .iter()
            .map(|(n, v)| format!(r#"<w:Selector Name="{n}">{v}</w:Selector>"#))
            .collect();
        format!("<w:SelectorSet>{inner}</w:SelectorSet>")
    };
    format!(
        r#"<w:Item><p:{class} xmlns:p="{uri}" xsi:type="p:{class}_Type">{body}</p:{class}><a:EndpointReference><a:Address>http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address><a:ReferenceParameters><w:ResourceURI>{uri}</w:ResourceURI>{selector_set}</a:ReferenceParameters></a:EndpointReference></w:Item>"#
    )
}

#[test]
fn datetimes_keep_their_offset_and_intervals_become_cim_intervals() {
    let w = wire();
    let body = concat!(
        "<p:East><cim:Datetime>2026-03-01T08:30:00.351+05:30</cim:Datetime></p:East>",
        "<p:West><cim:Datetime>2026-10-13T15:47:08.097097-04:00</cim:Datetime></p:West>",
        "<p:Zulu><cim:Datetime>2026-01-29T00:00:00Z</cim:Datetime></p:Zulu>",
        "<p:Span><cim:Interval>P1DT2H3M4.5S</cim:Interval></p:Span>",
        "<p:Gone xsi:nil=\"true\"/>"
    );
    script(
        &w,
        "/Enumerate",
        None,
        vec![enumerate_reply(&item("Vendor_Clock", body, &[("Id", "1")]))],
    );
    let found = client(&w)
        .enumerate(&target(), "Vendor_Clock", None)
        .unwrap();
    let clock = &found[0].record;
    let cim = |name: &str| (text(clock, name), property(clock, name).cim_type);
    assert_eq!(cim("East"), ("20260301083000.351000+330".into(), 101));
    assert_eq!(cim("West"), ("20261013154708.097097-240".into(), 101));
    assert_eq!(cim("Zulu"), ("20260129000000.000000+000".into(), 101));
    assert_eq!(cim("Span"), ("00000001020304.500000:000".into(), 101));
    assert_eq!(property(clock, "Gone").value, CimValue::Null);
}

#[test]
fn endpoint_reference_keys_are_quoted_and_escaped_in_paths() {
    let w = wire();
    let odd = r#"O'Brien &amp; "Co" \ x"#;
    let items = [
        item("Vendor_Thing", "<p:Name>t</p:Name>", &[("Name", odd)]),
        item(
            "Win32_Process",
            "<p:Handle>4</p:Handle>",
            &[("Handle", "4")],
        ),
        item(
            "Vendor_Pair",
            "<p:A>1</p:A>",
            &[("Zed", "z"), ("alpha", "a")],
        ),
    ]
    .concat();
    script(&w, "/Enumerate", None, vec![enumerate_reply(&items)]);
    let found = client(&w)
        .enumerate(&target(), "Win32_Process", Some("SELECT * FROM X"))
        .unwrap();
    let path = |i: usize| found[i].record.path.clone().unwrap().to_string_lossy();
    assert_eq!(
        path(0),
        r#"\\AYW\ROOT\CIMV2:Vendor_Thing.Name="O'Brien & \"Co\" \\ x""#
    );
    assert_eq!(path(1), r#"\\AYW\ROOT\CIMV2:Win32_Process.Handle="4""#);
    assert_eq!(path(2), r#"\\AYW\ROOT\CIMV2:Vendor_Pair.alpha="a",Zed="z""#);
    assert_eq!(found[0].selectors[0].1, r#"O'Brien & "Co" \ x"#);
}

#[test]
fn reference_properties_become_object_paths() {
    let w = wire();
    let link = r#"<p:Antecedent><a:Address>http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address><a:ReferenceParameters><w:ResourceURI>http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Win32_DiskPartition</w:ResourceURI><w:SelectorSet><w:Selector Name="DeviceID">Disk #0, Partition #1</w:Selector></w:SelectorSet></a:ReferenceParameters></p:Antecedent>"#;
    script(
        &w,
        "/Enumerate",
        None,
        vec![enumerate_reply(&item("Vendor_Link", link, &[("Id", "1")]))],
    );
    let found = client(&w)
        .enumerate(&target(), "Vendor_Link", None)
        .unwrap();
    let antecedent = property(&found[0].record, "Antecedent");
    assert_eq!(antecedent.cim_type, 102);
    assert_eq!(
        antecedent.value,
        CimValue::Reference(wide(
            "\\\\AYW\\ROOT\\CIMV2:Win32_DiskPartition.DeviceID=\"Disk #0, Partition #1\""
        ))
    );
}

#[test]
fn unknown_classes_type_every_plain_property_as_string() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(&w, "/Release", None, vec![ok("release")]);
    let result = management(&w)
        .execute(&remote("class Win32_SystemDriver"))
        .unwrap();
    let ManagementResult::Schema(schema) = result else {
        panic!("not a schema")
    };
    assert_eq!(schema.name, wide("Win32_SystemDriver"));
    assert!(schema.methods.is_empty());
    let names: Vec<_> = schema
        .properties
        .iter()
        .map(|p| p.name.to_string_lossy())
        .collect();
    assert!(names.contains(&"AcceptPause".to_owned()) && names.contains(&"TagId".to_owned()));
    assert!(schema.properties.iter().all(|p| p.cim_type == 8));
    assert!(
        schema
            .properties
            .iter()
            .all(|p| { p.qualifiers.iter().any(|q| q.name.eq_ignore_case("read")) })
    );
    assert_eq!(kinds(&w), ["Enumerate", "Release"]);
    assert_eq!(sent(&w)[0].texts("MaxElements"), ["1"]);
    assert_eq!(sent(&w)[1].texts("MaxElements"), Vec::<String>::new());
}

#[test]
fn known_classes_answer_schema_requests_without_the_network() {
    let w = wire();
    let result = management(&w)
        .execute(&remote("class Win32_Process"))
        .unwrap();
    let ManagementResult::Schema(schema) = result else {
        panic!("not a schema")
    };
    assert!(
        schema
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_case("Create") && m.is_static)
    );
    assert!(sent(&w).is_empty());
}

#[test]
fn unknown_class_properties_read_as_text_with_class_names_as_the_dynasty() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(
        &w,
        "/Pull",
        None,
        vec![ok("pull-driver-page2"), ok("pull-driver-page3")],
    );
    let found = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap();
    let driver = &found[0].record;
    assert_eq!(
        property(driver, "Started").value,
        CimValue::String(wide("false"))
    );
    assert_eq!(property(driver, "Started").cim_type, 8);
    assert_eq!(text(driver, "__DYNASTY"), "Win32_SystemDriver");
    assert_eq!(property(driver, "__SUPERCLASS").value, CimValue::Null);
}

#[test]
fn create_returns_the_process_id_out_parameter() {
    let w = wire();
    script(&w, "/Win32_Process/Create", None, vec![ok("invoke-create")]);
    let result = management(&w)
        .execute(&remote("process call create \"cmd.exe /c exit 0\""))
        .unwrap();
    let ManagementResult::Methods(results) = result else {
        panic!("not a method result")
    };
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].return_value, Some(CimValue::Uint32(0)));
    assert_eq!(results[0].out_parameters.len(), 1);
    assert_eq!(results[0].out_parameters[0].name, wide("ProcessId"));
    assert_eq!(results[0].out_parameters[0].value, CimValue::Uint32(36840));
    let message = &sent(&w)[0];
    assert_eq!(kinds(&w), ["Create"]);
    assert!(message.selectors().is_empty());
    assert!(message.resource.ends_with("/root/cimv2/Win32_Process"));
    assert!(
        message
            .xml
            .contains("<p:CommandLine>cmd.exe /c exit 0</p:CommandLine>")
    );
    assert!(message.xml.contains("<p:Create_INPUT"));
}

#[test]
fn a_failed_create_is_a_normal_reply_with_a_null_process_id() {
    let w = wire();
    script(
        &w,
        "/Win32_Process/Create",
        None,
        vec![ok("invoke-create-failed")],
    );
    let result = management(&w)
        .execute(&remote("process call create \"C:\\nonexistent\\nope.exe\""))
        .unwrap();
    let ManagementResult::Methods(results) = result else {
        panic!("not a method result")
    };
    assert_eq!(results[0].return_value, Some(CimValue::Uint32(9)));
    assert_eq!(results[0].out_parameters[0].value, CimValue::Null);
}

#[test]
fn instance_methods_find_targets_then_invoke_with_selectors() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
    script(
        &w,
        "/Win32_Process/GetOwner",
        None,
        vec![ok("invoke-getowner")],
    );
    let result = management(&w)
        .execute(&remote("process where \"Handle='4'\" call getowner"))
        .unwrap();
    let ManagementResult::Methods(results) = result else {
        panic!("not a method result")
    };
    assert_eq!(
        results[0].path,
        wide("\\\\AYW\\ROOT\\CIMV2:Win32_Process.Handle=\"4\"")
    );
    let names: Vec<_> = results[0]
        .out_parameters
        .iter()
        .map(|p| p.name.to_string_lossy())
        .collect();
    assert_eq!(names, ["User", "Domain"]);
    assert_eq!(
        results[0].out_parameters[1].value,
        CimValue::String(wide("AYW"))
    );
    assert_eq!(results[0].return_value, Some(CimValue::Uint32(0)));
    assert_eq!(kinds(&w), ["Enumerate", "GetOwner"]);
    let invoke = &sent(&w)[1];
    assert_eq!(invoke.selectors(), [("Handle".to_owned(), "4".to_owned())]);
    assert!(invoke.xml.contains("<p:GetOwner_INPUT"));
}

struct Answer {
    reply: bool,
    prompts: Vec<String>,
}

impl Confirmation for Answer {
    fn confirm(&mut self, prompt: &WideText) -> Result<bool, ManagementError> {
        self.prompts.push(prompt.to_string_lossy());
        Ok(self.reply)
    }
}

fn interactive(mut r: ResolvedRequest) -> ResolvedRequest {
    r.modifiers.push(Modifier {
        prefix: None,
        raw_value: None,
        name: wide("interactive"),
        value: SwitchValue::Missing,
        scope: ModifierScope::Global,
    });
    r
}

#[test]
fn interactive_calls_prompt_per_instance_and_a_no_skips_the_invoke() {
    for (reply, invoked) in [(false, 0), (true, 1)] {
        let w = wire();
        script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
        script(
            &w,
            "/Win32_Process/GetOwner",
            None,
            vec![ok("invoke-getowner")],
        );
        let mut answer = Answer {
            reply,
            prompts: vec![],
        };
        let result = management(&w)
            .execute_with_confirmation(
                &interactive(remote("process where \"Handle='4'\" call getowner")),
                &mut answer,
            )
            .unwrap();
        assert_eq!(
            answer.prompts,
            ["Execute (\\\\AYW\\ROOT\\CIMV2:Win32_Process.Handle=\"4\")->getowner() (Y/N/?)? "]
        );
        assert_eq!(
            kinds(&w).iter().filter(|k| *k == "GetOwner").count(),
            invoked
        );
        if !reply {
            assert!(matches!(result, ManagementResult::Operation(_)));
        }
    }
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
    let error = management(&w)
        .execute(&interactive(remote(
            "process where \"Handle='4'\" call getowner",
        )))
        .unwrap_err();
    assert_eq!(error.operation, wide("input"));
}

#[test]
fn an_undeclared_method_reports_the_verb_for_aliases_and_the_class_method_otherwise() {
    for (command, code, text) in [
        (
            "process call nosuchmethod",
            0xABFD,
            "nosuchmethod - Invalid alias verb.",
        ),
        (
            "path Win32_Process call nosuchmethod",
            0xABFC,
            "nosuchmethod - Invalid class method.",
        ),
        (
            "class Win32_Process call nosuchmethod",
            0xABFC,
            "nosuchmethod - Invalid class method.",
        ),
    ] {
        let w = wire();
        let error = management(&w).execute(&remote(command)).unwrap_err();
        assert_eq!(
            (
                error.code,
                error.operation.to_string_lossy(),
                error.message.to_string_lossy()
            ),
            (code, "input".to_owned(), text.to_owned()),
            "{command}"
        );
        assert!(sent(&w).is_empty(), "{command}");
    }
}

#[test]
fn methods_of_classes_the_schema_lacks_take_named_arguments_and_read_text() {
    let w = wire();
    script(
        &w,
        "/StdRegProv/GetStringValue",
        None,
        vec![ok("invoke-stdregprov")],
    );
    let mut r = remote("path StdRegProv call GetStringValue");
    r.connection.namespace = wide("root\\default");
    r.arguments = vec![
        wide("hDefKey=2147483650"),
        wide("sSubKeyName=\"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\""),
        wide("sValueName=\"ProductName\""),
    ];
    let result = management(&w).execute(&r).unwrap();
    let ManagementResult::Methods(results) = result else {
        panic!("not a method result")
    };
    assert_eq!(results[0].out_parameters[0].name, wide("sValue"));
    assert_eq!(
        results[0].out_parameters[0].value,
        CimValue::String(wide("Windows 10 Pro"))
    );
    assert_eq!(results[0].return_value, Some(CimValue::Uint32(0)));
    let message = &sent(&w)[0];
    assert!(message.resource.ends_with("/root/default/StdRegProv"));
    assert!(message.xml.contains("<p:hDefKey>2147483650</p:hDefKey>"));
    assert!(message.xml.contains(
        "<p:sSubKeyName>SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion</p:sSubKeyName>"
    ));
}

#[test]
fn embedded_objects_in_outputs_become_nested_records() {
    let w = wire();
    script(
        &w,
        "/GetSecurityDescriptor",
        None,
        vec![ok("invoke-getsecuritydescriptor")],
    );
    let outcome = client(&w)
        .invoke(
            &target(),
            "Win32_LogicalFileSecuritySetting",
            "GetSecurityDescriptor",
            &[("Path".into(), "C:\\x".into())],
            None,
            &[],
        )
        .unwrap();
    assert_eq!(outcome.return_value, Some(CimValue::Uint32(0)));
    let descriptor = &outcome.out_parameters[0];
    assert_eq!(descriptor.cim_type, 13);
    let CimValue::Object(record) = &descriptor.value else {
        panic!("not an object")
    };
    assert_eq!(record.class, wide("Win32_SecurityDescriptor"));
    let dacl = property(record, "DACL");
    assert_eq!(dacl.cim_type, 13 | 0x2000);
    let CimValue::Array(aces) = &dacl.value else {
        panic!("DACL is not an array")
    };
    assert!(aces.len() >= 2);
    let CimValue::Object(ace) = &aces[0] else {
        panic!("ACE is not an object")
    };
    assert_eq!(ace.class, wide("Win32_ACE"));
}

#[test]
fn array_outputs_with_one_element_stay_arrays_when_the_schema_says_so() {
    let w = wire();
    script(&w, "/EnumKey", None, vec![ok("invoke-enumkey")]);
    let schema = MethodSchema {
        name: wide("EnumKey"),
        input: vec![],
        output: vec![
            ParameterSchema {
                cim_type_name: None,
                name: wide("sNames"),
                cim_type: 8 | 0x2000,
                id: Some(0),
                optional: false,
                default_value: None,
            },
            ParameterSchema {
                cim_type_name: None,
                name: wide("ReturnValue"),
                cim_type: 19,
                id: None,
                optional: false,
                default_value: None,
            },
        ],
        is_static: true,
        qualifiers: vec![],
    };
    let outcome = client(&w)
        .invoke(&target(), "StdRegProv", "EnumKey", &[], Some(&schema), &[])
        .unwrap();
    assert_eq!(
        outcome.out_parameters[0].value,
        CimValue::Array(vec![CimValue::String(wide("CurrentVersion"))])
    );
    assert_eq!(outcome.return_value, Some(CimValue::Uint32(0)));
}

#[test]
fn fault_rows_map_to_wmi_status_message_and_operation() {
    enum Call {
        Enumerate,
        Get,
        Invoke,
    }
    let rows = [
        (
            "fault-unknown-class-enumerate",
            Call::Enumerate,
            expect(0x80041010, "Invalid class", "query"),
        ),
        (
            "fault-unknown-namespace",
            Call::Enumerate,
            expect(0x8004100e, "Invalid namespace", "connect"),
        ),
        (
            "fault-wql-syntax",
            Call::Enumerate,
            expect(0x80041017, "Invalid query", "query"),
        ),
        (
            "fault-wql-unknown-class",
            Call::Enumerate,
            expect(0x80041010, "Invalid class", "query"),
        ),
        (
            "fault-wql-unknown-property",
            Call::Enumerate,
            expect(0x80041017, "Invalid query", "query"),
        ),
        (
            "fault-wql-bad-where",
            Call::Enumerate,
            expect(0x80041017, "Invalid query", "query"),
        ),
        (
            "fault-wql-empty",
            Call::Enumerate,
            expect(0x80041017, "Invalid query", "query"),
        ),
        (
            "fault-wql-class-uri",
            Call::Enumerate,
            expect(0x80041008, "Invalid parameter", "query"),
        ),
        (
            "fault-provider-shadowcopy",
            Call::Enumerate,
            expect(0x80041014, "Initialization failure", "query"),
        ),
        (
            "fault-access-denied",
            Call::Enumerate,
            expect(0x80041003, "Access denied", "query"),
        ),
        (
            "fault-envelope-too-small",
            Call::Enumerate,
            expect(0x80041001, "Generic failure", "query"),
        ),
        (
            "fault-bad-timeout",
            Call::Enumerate,
            expect(0x80041001, "Generic failure", "query"),
        ),
        (
            "fault-unknown-class-get",
            Call::Get,
            expect(0x80041002, "Not found", "class"),
        ),
        (
            "fault-no-selectors",
            Call::Get,
            expect(0x80041002, "Not found", "query"),
        ),
        (
            "fault-wrong-selector",
            Call::Get,
            expect(0x80041008, "Invalid parameter", "query"),
        ),
        (
            "fault-invalid-method",
            Call::Invoke,
            expect(0x8004102e, "Invalid method", "method"),
        ),
        (
            "fault-method-instance-missing",
            Call::Invoke,
            expect(0x80041002, "Not found", "method"),
        ),
        (
            "fault-access-denied",
            Call::Invoke,
            expect(0x80041003, "Access denied", "method"),
        ),
    ];
    for (name, call, expected) in rows {
        let w = wire();
        let action = match call {
            Call::Enumerate => "/Enumerate",
            Call::Get => "/Get",
            Call::Invoke => "/Create",
        };
        script(&w, action, None, vec![fault(name)]);
        let mut c = client(&w);
        let error = match call {
            Call::Enumerate => c
                .enumerate(
                    &target(),
                    "Win32_NoSuchClass",
                    Some("SELECT * FROM Win32_NoSuchClass"),
                )
                .map(|_| ()),
            Call::Get => c
                .get(
                    &target(),
                    "Win32_NoSuchClass",
                    &[("Handle".into(), "1".into())],
                )
                .map(|_| ()),
            Call::Invoke => c
                .invoke(&target(), "Win32_Process", "Create", &[], None, &[])
                .map(|_| ()),
        }
        .unwrap_err();
        assert_eq!(failure(error.clone()), expected, "{name}");
        assert_eq!(error.target, wide("AYW"), "{name}");
    }
}

#[test]
fn a_missing_instance_is_an_empty_get_not_an_error() {
    let w = wire();
    script(&w, "/Get", None, vec![fault("fault-instance-not-found")]);
    let records = objects(
        management(&w)
            .execute(&remote("path Win32_Process.Handle=\"999999\" get Name"))
            .unwrap(),
    );
    assert!(records.is_empty());
}

#[test]
fn alias_query_unknown_class_and_path_unknown_class_report_differently() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![fault("fault-wql-unknown-class")],
    );
    let error = management(&w)
        .execute(&remote("os get Caption"))
        .unwrap_err();
    assert_eq!(failure(error), expect(0x80041010, "Invalid class", "query"));

    let w = wire();
    script(&w, "/Get", None, vec![fault("fault-unknown-class-get")]);
    let error = management(&w)
        .execute(&remote("path Win32_NoSuchClass.Id=\"1\" get Name"))
        .unwrap_err();
    assert_eq!(failure(error), expect(0x80041002, "Not found", "class"));
}

#[test]
fn a_node_list_skips_the_class_lookup_rewrite() {
    for (command, fault_name, action) in [
        (
            "path Win32_NoSuchClass.Id=\"1\" get Name",
            "fault-unknown-class-get",
            "/Get",
        ),
        (
            "path Win32_NoSuchClass get Name",
            "fault-unknown-class-enumerate",
            "/Enumerate",
        ),
    ] {
        let w = wire();
        script(&w, action, None, vec![fault(fault_name)]);
        let mut request = remote(command);
        request.multiple_nodes = true;
        let error = management(&w).execute(&request).unwrap_err();
        assert_eq!(
            failure(error),
            expect(0x80041010, "Invalid class", "query"),
            "{command}"
        );
        let w = wire();
        script(&w, action, None, vec![fault(fault_name)]);
        let error = management(&w).execute(&remote(command)).unwrap_err();
        assert_eq!(
            failure(error),
            expect(0x80041002, "Not found", "class"),
            "{command}"
        );
    }
}

#[test]
fn unsupported_verbs_report_the_operation_they_would_have_run() {
    let w = wire();
    let mut backend = management(&w);
    for (verb, operation) in [
        (Verb::Set, "update"),
        (Verb::Delete, "delete"),
        (Verb::Create, "write"),
        (Verb::Assoc, "query"),
    ] {
        let mut r = remote("os get Caption");
        r.verb = verb;
        let error = backend.execute(&r).unwrap_err();
        assert_eq!(
            failure(error),
            expect(0x8004100c, "Not supported", operation),
            "{verb:?}"
        );
    }
    assert!(sent(&w).is_empty());
}

#[test]
fn transport_errors_map_to_rpc_unavailable_access_denied_and_generic_failure() {
    let cases = [
        (
            Reply::Connect,
            expect(0x800706ba, "The RPC server is unavailable.", "query"),
        ),
        (
            Reply::Authentication,
            expect(0x80070005, "Access is denied.", "query"),
        ),
        (Reply::Io, expect(0x80041001, "Generic failure", "query")),
        (
            Reply::Xml(200, "<html>not soap</html>".into()),
            expect(0x80041001, "Generic failure", "query"),
        ),
        (
            Reply::Xml(404, "<html>missing</html>".into()),
            expect(0x80041001, "Generic failure", "query"),
        ),
        (
            Reply::Xml(401, String::new()),
            expect(0x80070005, "Access is denied.", "query"),
        ),
    ];
    for (reply, expected) in cases {
        let w = wire();
        script(&w, "/Enumerate", None, vec![reply]);
        let error = management(&w)
            .execute(&remote("os get Caption"))
            .unwrap_err();
        assert_eq!(failure(error.clone()), expected);
        if expected.0 == 0x80041001 {
            assert!(!error.extended.is_empty());
        }
    }
}

#[test]
fn a_refused_connection_is_rpc_unavailable() {
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    assert!(!listener_reachable("127.0.0.1", port, 500));
    let connector: wmic_compat::backend::wsman::client::Connector = Box::new(move |_| {
        std::net::TcpStream::connect(("127.0.0.1", port))
            .map(|_| unreachable!("port is closed"))
            .map_err(TransportError::Connect)
    });
    let mut backend = WsmanManagement::new(connector);
    let error = backend.execute(&remote("os get Caption")).unwrap_err();
    assert_eq!(
        failure(error),
        expect(0x800706ba, "The RPC server is unavailable.", "query")
    );
}

#[test]
fn the_default_transport_reports_a_refused_connection_as_connect() {
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let mut real = Target {
        endpoint: endpoint(port),
        ..target()
    };
    real.endpoint.host = "127.0.0.1".into();
    let error = WsmanClient::new(default_connector())
        .enumerate(
            &real,
            "Win32_OperatingSystem",
            Some("SELECT * FROM Win32_OperatingSystem"),
        )
        .unwrap_err();
    assert_eq!(
        failure(error),
        expect(0x800706ba, "The RPC server is unavailable.", "query")
    );
}

#[test]
fn an_open_listener_is_reachable() {
    let open = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = open.local_addr().unwrap().port();
    assert!(listener_reachable("127.0.0.1", port, 1000));
}

#[test]
fn connections_are_cached_per_node_and_idempotent_requests_retry_once() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![ok("enumerate-empty"), Reply::Io, ok("enumerate-empty")],
    );
    let mut c = client(&w);
    c.enumerate(&target(), "A", Some("SELECT * FROM A"))
        .unwrap();
    assert_eq!(w.borrow().connections, 1);
    c.enumerate(&target(), "A", Some("SELECT * FROM A"))
        .unwrap();
    assert_eq!(
        w.borrow().connections,
        2,
        "the broken connection is replaced"
    );
    assert_eq!(kinds(&w).len(), 3);

    let w = wire();
    script(
        &w,
        "/Create",
        None,
        vec![ok("invoke-create"), Reply::Io, ok("invoke-create")],
    );
    let mut c = client(&w);
    c.invoke(&target(), "Win32_Process", "Create", &[], None, &[])
        .unwrap();
    let error = c
        .invoke(&target(), "Win32_Process", "Create", &[], None, &[])
        .unwrap_err();
    assert_eq!(error.code, 0x80041001);
    assert_eq!(kinds(&w).len(), 2, "a sent Invoke is not repeated");
}

#[test]
fn requests_carry_the_required_headers() {
    let w = wire();
    script(&w, "/Get", None, vec![ok("get-process-4")]);
    client(&w)
        .get(&target(), "Win32_Process", &[("Handle".into(), "4".into())])
        .unwrap();
    let message = &sent(&w)[0];
    assert_eq!(message.texts("To"), ["http://AYW:5985/wsman"]);
    assert_eq!(
        message.texts("Address"),
        ["http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous"]
    );
    assert_eq!(message.texts("MaxEnvelopeSize"), ["512000"]);
    assert_eq!(message.texts("OperationTimeout"), ["PT60.000S"]);
    assert!(message.texts("MessageID")[0].starts_with("uuid:"));
    assert_eq!(
        message.action,
        "http://schemas.xmlsoap.org/ws/2004/09/transfer/Get"
    );
    assert!(message.xml.contains("<w:Locale xml:lang=\"en-US\""));
    assert!(message.xml.contains("<p:DataLocale xml:lang=\"en-US\""));
    assert!(message.resource.contains(CIMV2));
}

fn deep_reply(depth: usize) -> Reply {
    enumerate_reply(&format!(
        "<w:Item>{}{}</w:Item>",
        "<a>".repeat(depth),
        "</a>".repeat(depth)
    ))
}

fn detail_mentions(error: &ManagementError, needle: &str) -> bool {
    error
        .extended
        .iter()
        .any(|p| matches!(&p.value, CimValue::String(s) if s.to_string_lossy().contains(needle)))
}

#[test]
fn deeply_nested_and_oversized_replies_fail_as_malformed_instead_of_aborting() {
    for depth in [300, 20_000] {
        let w = wire();
        script(&w, "/Enumerate", None, vec![deep_reply(depth)]);
        let error = client(&w)
            .enumerate(&target(), "A", Some("SELECT * FROM A"))
            .unwrap_err();
        assert_eq!(
            failure(error.clone()),
            expect(0x80041001, "Generic failure", "query"),
            "depth {depth}"
        );
        assert!(detail_mentions(&error, "too deeply"), "depth {depth}");
    }

    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![enumerate_reply(&format!(
            "<w:Item><a>{}</a></w:Item>",
            "<b/>".repeat(250_000)
        ))],
    );
    let error = client(&w)
        .enumerate(&target(), "A", Some("SELECT * FROM A"))
        .unwrap_err();
    assert_eq!(error.code, 0x80041001);

    let w = wire();
    script(&w, "/Enumerate", None, vec![deep_reply(100)]);
    assert!(
        client(&w)
            .enumerate(&target(), "A", Some("SELECT * FROM A"))
            .is_ok()
    );
}

#[test]
fn a_pull_that_times_out_is_pulled_again_with_the_same_context() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(
        &w,
        "/Pull",
        None,
        vec![
            fault("fault-timed-out"),
            fault("fault-timed-out"),
            ok("pull-driver-page2"),
            ok("pull-driver-page3"),
        ],
    );
    let found = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap();
    assert_eq!(found.len(), 3);
    assert_eq!(
        kinds(&w),
        ["Enumerate", "Pull", "Pull", "Pull", "Pull"],
        "no Release and no restart"
    );
    let messages = sent(&w);
    let first = messages[1].texts("EnumerationContext");
    assert_eq!(messages[2].texts("EnumerationContext"), first);
    assert_eq!(messages[3].texts("EnumerationContext"), first);
}

static CHECKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[test]
fn an_interrupt_ends_the_retries_of_a_pull_that_keeps_timing_out() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(
        &w,
        "/Pull",
        None,
        (0..10).map(|_| fault("fault-timed-out")).collect(),
    );
    script(&w, "/Release", None, vec![ok("release")]);
    let error = client(&w)
        .with_interrupt(|| CHECKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap_err();
    assert_eq!(error.code, 0xC000013A);
    assert_eq!(kinds(&w), ["Enumerate", "Pull", "Pull", "Pull", "Release"]);
}

#[test]
fn a_transport_error_during_a_pull_releases_the_context_on_a_new_connection() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-driver-page1")]);
    script(&w, "/Pull", None, vec![Reply::Io]);
    script(&w, "/Release", None, vec![ok("release")]);
    let error = client(&w)
        .enumerate(&target(), "Win32_SystemDriver", None)
        .unwrap_err();
    assert_eq!(error.code, 0x80041001);
    assert!(matches!(
        error.partial.as_deref(),
        Some(ManagementResult::Objects(set)) if set.objects.len() == 1
    ));
    assert_eq!(kinds(&w), ["Enumerate", "Pull", "Release"]);
    assert_eq!(w.borrow().connections, 2);
    assert_eq!(
        sent(&w)[2].texts("EnumerationContext"),
        sent(&w)[1].texts("EnumerationContext")
    );
}

#[test]
fn server_is_the_computer_name_the_service_reports_for_every_kind_of_host() {
    let cases = [
        ("10.1.2.3", Some("LABHOST"), "LABHOST"),
        ("labhost.corp.example.test", Some("LABHOST"), "LABHOST"),
        ("localhost", Some("LABHOST"), "LABHOST"),
        (".", Some("LABHOST"), "LABHOST"),
        ("\\\\labhost", Some("LABHOST"), "LABHOST"),
        (
            "labhost.corp.example.test",
            None,
            "LABHOST.CORP.EXAMPLE.TEST",
        ),
        ("10.1.2.3", None, "10.1.2.3"),
        ("localhost", None, "LOCALHOST"),
    ];
    for (host, reported, expected) in cases {
        let w = wire();
        w.borrow_mut().server_name = reported.map(str::to_owned);
        script(&w, "/Enumerate", None, vec![ok("enumerate-os")]);
        let mut r = remote("os get");
        r.connection.node = wide(host);
        let records = objects(management(&w).execute(&r).unwrap());
        assert_eq!(text(&records[0], "__SERVER"), expected, "{host}");
        assert_eq!(records[0].server, Some(wide(expected)), "{host}");
        assert_eq!(
            records[0].path,
            Some(wide(&format!(
                "\\\\{expected}\\ROOT\\CIMV2:Win32_OperatingSystem=@"
            ))),
            "{host}"
        );
    }
}

const COMPUTER_SYSTEM_QUERY: &str = "SELECT Name FROM Win32_ComputerSystem";

fn computer_system(name: &str) -> Reply {
    Reply::Xml(
        200,
        fixture("wql-process-fragment")
            .replace("<Name>System</Name>", &format!("<Name>{name}</Name>")),
    )
}

fn computer_system_queries(wire: &Shared) -> usize {
    sent(wire)
        .iter()
        .filter(|s| s.xml.contains(COMPUTER_SYSTEM_QUERY))
        .count()
}

#[test]
fn a_kerberos_connection_reads_the_computer_name_once() {
    let w = wire();
    w.borrow_mut().mechanism = Some("Kerberos");
    script(
        &w,
        "/Enumerate",
        None,
        vec![
            ok("enumerate-os"),
            computer_system("CM01"),
            ok("enumerate-os"),
        ],
    );
    let mut backend = management(&w);
    let mut r = remote("os get");
    r.connection.node = wide("10.1.2.3");
    for _ in 0..2 {
        let records = objects(backend.execute(&r).unwrap());
        assert_eq!(text(&records[0], "__SERVER"), "CM01");
        assert_eq!(
            records[0].path,
            Some(wide("\\\\CM01\\ROOT\\CIMV2:Win32_OperatingSystem=@"))
        );
    }
    assert_eq!(kinds(&w), ["Enumerate", "Enumerate", "Enumerate"]);
    assert_eq!(computer_system_queries(&w), 1);
    assert!(sent(&w)[1].xml.contains(COMPUTER_SYSTEM_QUERY));
    assert_eq!(w.borrow().connections, 1);
}

#[test]
fn a_kerberos_connection_per_endpoint_reads_its_own_computer_name() {
    let w = wire();
    w.borrow_mut().mechanism = Some("Kerberos");
    script(
        &w,
        "/Enumerate",
        None,
        vec![
            ok("enumerate-os"),
            computer_system("CM01"),
            ok("enumerate-os"),
            computer_system("CM02"),
        ],
    );
    let mut backend = management(&w);
    let mut first = remote("os get");
    first.connection.node = wide("10.1.2.3");
    let mut second = remote("os get");
    second.connection.node = wide("10.1.2.4");
    let first = objects(backend.execute(&first).unwrap());
    let second = objects(backend.execute(&second).unwrap());
    assert_eq!(text(&first[0], "__SERVER"), "CM01");
    assert_eq!(text(&second[0], "__SERVER"), "CM02");
    assert_eq!(computer_system_queries(&w), 2);
}

#[test]
fn ntlm_and_unreported_authentication_send_no_computer_name_query() {
    for mechanism in [Some("NTLM"), None] {
        let w = wire();
        w.borrow_mut().mechanism = mechanism;
        script(&w, "/Enumerate", None, vec![ok("enumerate-os")]);
        let mut r = remote("os get");
        r.connection.node = wide("10.1.2.3");
        let records = objects(management(&w).execute(&r).unwrap());
        assert_eq!(text(&records[0], "__SERVER"), "10.1.2.3", "{mechanism:?}");
        assert_eq!(kinds(&w), ["Enumerate"], "{mechanism:?}");
        assert_eq!(computer_system_queries(&w), 0, "{mechanism:?}");
    }
}

#[test]
fn a_kerberos_connection_does_not_ask_again_when_the_service_reported_a_name() {
    let w = wire();
    w.borrow_mut().mechanism = Some("Kerberos");
    w.borrow_mut().server_name = Some("LABHOST".into());
    script(
        &w,
        "/Enumerate",
        None,
        vec![ok("enumerate-os"), ok("enumerate-os")],
    );
    let mut backend = management(&w);
    let mut r = remote("os get");
    r.connection.node = wide("10.1.2.3");
    objects(backend.execute(&r).unwrap());
    let records = objects(backend.execute(&r).unwrap());
    assert_eq!(text(&records[0], "__SERVER"), "LABHOST");
    assert_eq!(computer_system_queries(&w), 0);
}

#[test]
fn a_failed_computer_name_read_leaves_the_typed_host() {
    let w = wire();
    w.borrow_mut().mechanism = Some("Kerberos");
    script(
        &w,
        "/Enumerate",
        None,
        vec![ok("enumerate-os"), fault("fault-access-denied")],
    );
    let mut r = remote("os get");
    r.connection.node = wide("10.1.2.3");
    let records = objects(management(&w).execute(&r).unwrap());
    assert_eq!(text(&records[0], "__SERVER"), "10.1.2.3");
    assert_eq!(computer_system_queries(&w), 1);
}

#[test]
fn get_and_invoke_on_a_kerberos_connection_read_the_computer_name() {
    let w = wire();
    w.borrow_mut().mechanism = Some("Kerberos");
    script(&w, "/Get", None, vec![ok("get-process-4")]);
    script(&w, "/Enumerate", None, vec![computer_system("CM01")]);
    let mut r = remote("path Win32_Process.Handle=\"4\" get Name");
    r.connection.node = wide("10.1.2.3");
    let records = objects(management(&w).execute(&r).unwrap());
    assert_eq!(
        records[0].path,
        Some(wide("\\\\CM01\\ROOT\\CIMV2:Win32_Process.Handle=\"4\""))
    );
    assert_eq!(text(&records[0], "__SERVER"), "CM01");
    assert_eq!(kinds(&w), ["Get", "Enumerate"]);

    let w = wire();
    w.borrow_mut().mechanism = Some("Kerberos");
    script(
        &w,
        "/Win32_Process/GetOwner",
        None,
        vec![ok("invoke-getowner")],
    );
    script(&w, "/Enumerate", None, vec![computer_system("CM01")]);
    let selectors = [("Handle".to_owned(), "4".to_owned())];
    client(&w)
        .invoke(
            &target(),
            "Win32_Process",
            "GetOwner",
            &selectors,
            None,
            &[],
        )
        .unwrap();
    assert_eq!(kinds(&w), ["GetOwner", "Enumerate"]);
    assert_eq!(computer_system_queries(&w), 1);
}

#[test]
fn get_and_method_targets_use_the_reported_computer_name() {
    let w = wire();
    w.borrow_mut().server_name = Some("LABHOST".into());
    script(&w, "/Get", None, vec![ok("get-process-4")]);
    let mut r = remote("path Win32_Process.Handle=\"4\" get Name");
    r.connection.node = wide("10.1.2.3");
    let records = objects(management(&w).execute(&r).unwrap());
    assert_eq!(
        records[0].path,
        Some(wide("\\\\LABHOST\\ROOT\\CIMV2:Win32_Process.Handle=\"4\""))
    );
    assert_eq!(text(&records[0], "__SERVER"), "LABHOST");

    let w = wire();
    w.borrow_mut().server_name = Some("LABHOST".into());
    script(&w, "/Enumerate", None, vec![ok("wql-process-fragment")]);
    script(
        &w,
        "/Win32_Process/GetOwner",
        None,
        vec![ok("invoke-getowner")],
    );
    let mut r = remote("process where \"Handle='4'\" call getowner");
    r.connection.node = wide("10.1.2.3");
    let ManagementResult::Methods(results) = management(&w).execute(&r).unwrap() else {
        panic!("not a method result")
    };
    assert_eq!(
        results[0].path,
        wide("\\\\LABHOST\\ROOT\\CIMV2:Win32_Process.Handle=\"4\"")
    );
}

#[test]
fn association_instances_render_nested_reference_keys_as_quoted_paths() {
    let w = wire();
    script(&w, "/Enumerate", None, vec![ok("enumerate-association")]);
    let found = client(&w)
        .enumerate(&target(), "Win32_LogicalDiskToPartition", None)
        .unwrap();
    assert_eq!(found.len(), 1);
    let relative = concat!(
        r##"Win32_LogicalDiskToPartition.Antecedent="\\\\AYW\\ROOT\\CIMV2:Win32_DiskPartition.DeviceID=\"Disk #0, Partition #1\"","##,
        r##"Dependent="\\\\AYW\\ROOT\\CIMV2:Win32_LogicalDisk.DeviceID=\"C:\"""##
    );
    let record = &found[0].record;
    assert_eq!(
        record.path,
        Some(wide(&format!("\\\\AYW\\ROOT\\CIMV2:{relative}")))
    );
    assert_eq!(text(record, "__RELPATH"), relative);
    assert!(
        found[0].selectors.is_empty(),
        "reference keys are not flattened into plain selectors"
    );
    assert_eq!(
        property(record, "Antecedent").value,
        CimValue::Reference(wide(
            "\\\\AYW\\ROOT\\CIMV2:Win32_DiskPartition.DeviceID=\"Disk #0, Partition #1\""
        ))
    );
}

#[test]
fn only_the_direct_selectors_of_the_outer_set_are_keys() {
    let xml = enumerate_reply(
        r#"<w:Item><p:Vendor_Thing xmlns:p="http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Vendor_Thing"><p:Id>1</p:Id></p:Vendor_Thing><a:EndpointReference><a:Address>x</a:Address><a:ReferenceParameters><w:ResourceURI>http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Vendor_Thing</w:ResourceURI><w:SelectorSet><w:Selector Name="Id">1</w:Selector><w:Selector Name="Owner"><a:EndpointReference><a:Address>x</a:Address><a:ReferenceParameters><w:ResourceURI>http://schemas.microsoft.com/wbem/wsman/1/wmi/root/cimv2/Win32_Process</w:ResourceURI><w:SelectorSet><w:Selector Name="Handle">4</w:Selector></w:SelectorSet></a:ReferenceParameters></a:EndpointReference></w:Selector></w:SelectorSet></a:ReferenceParameters></a:EndpointReference></w:Item>"#,
    );
    let w = wire();
    script(&w, "/Enumerate", None, vec![xml]);
    let found = client(&w)
        .enumerate(&target(), "Vendor_Thing", None)
        .unwrap();
    assert_eq!(found[0].selectors, [("Id".to_owned(), "1".to_owned())]);
    assert_eq!(
        found[0].record.path,
        Some(wide(
            r#"\\AYW\ROOT\CIMV2:Vendor_Thing.Id="1",Owner="\\\\AYW\\ROOT\\CIMV2:Win32_Process.Handle=\"4\"""#
        ))
    );
}

#[test]
fn a_connection_the_peer_closed_is_replaced_before_a_request_that_changes_state() {
    let w = wire();
    script(
        &w,
        "/Create",
        None,
        vec![ok("invoke-create"), ok("invoke-create")],
    );
    let mut c = client(&w);
    c.invoke(&target(), "Win32_Process", "Create", &[], None, &[])
        .unwrap();
    w.borrow_mut().stale = 1;
    c.invoke(&target(), "Win32_Process", "Create", &[], None, &[])
        .unwrap();
    assert_eq!(kinds(&w), ["Create", "Create"], "each Invoke is sent once");
    assert_eq!(w.borrow().connections, 2);
}

#[test]
fn a_401_reply_evicts_the_connection() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        vec![Reply::Xml(401, String::new()), ok("enumerate-empty")],
    );
    let mut c = client(&w);
    let error = c
        .enumerate(&target(), "A", Some("SELECT * FROM A"))
        .unwrap_err();
    assert_eq!(
        failure(error),
        expect(0x80070005, "Access is denied.", "query")
    );
    assert_eq!(w.borrow().connections, 1);
    c.enumerate(&target(), "A", Some("SELECT * FROM A"))
        .unwrap();
    assert_eq!(
        w.borrow().connections,
        2,
        "the next request authenticates again"
    );
}

#[test]
fn a_different_password_opens_a_new_connection() {
    let w = wire();
    script(
        &w,
        "/Enumerate",
        None,
        (0..4).map(|_| ok("enumerate-empty")).collect(),
    );
    let mut c = client(&w);
    let mut other = target();
    other.endpoint.password = SecretWide::new(wide("another secret"));
    let query = |c: &mut WsmanClient, t: &Target| {
        c.enumerate(t, "A", Some("SELECT * FROM A")).unwrap();
    };
    query(&mut c, &target());
    query(&mut c, &target());
    assert_eq!(w.borrow().connections, 1, "same account and password share");
    query(&mut c, &other);
    assert_eq!(w.borrow().connections, 2);
    query(&mut c, &other);
    assert_eq!(w.borrow().connections, 2);
}

#[test]
fn unknown_classes_of_paths_are_not_found_but_queries_keep_invalid_class() {
    for name in ["fault-unknown-class-enumerate", "fault-wql-unknown-class"] {
        let w = wire();
        script(&w, "/Enumerate", None, vec![fault(name)]);
        let error = management(&w)
            .execute(&remote("path Win32_NoSuchClass get Name"))
            .unwrap_err();
        assert_eq!(
            failure(error),
            expect(0x80041002, "Not found", "class"),
            "{name}"
        );

        let w = wire();
        script(&w, "/Enumerate", None, vec![fault(name)]);
        let error = client(&w)
            .enumerate_class(&target(), "Win32_NoSuchClass", Some("SELECT * FROM X"))
            .unwrap_err();
        assert_eq!(
            failure(error),
            expect(0x80041002, "Not found", "class"),
            "{name}"
        );

        let w = wire();
        script(&w, "/Enumerate", None, vec![fault(name)]);
        let error = management(&w)
            .execute(&remote("os get Caption"))
            .unwrap_err();
        assert_eq!(
            failure(error),
            expect(0x80041010, "Invalid class", "query"),
            "{name}"
        );
    }
}

#[test]
fn a_port_the_environment_names_wrongly_is_an_invalid_parameter() {
    let w = wire();
    let mut backend = management(&w).with_listener_source(|| {
        Err(wmic_compat::backend::wsman::InvalidSetting::Port(
            "0".into(),
        ))
    });
    let error = backend.execute(&remote("os get Caption")).unwrap_err();
    assert_eq!(
        failure(error.clone()),
        expect(0x80041008, "Invalid parameter", "query")
    );
    assert_eq!(error.target, wide("AYW"));
    assert!(detail_mentions(&error, "WMIC_WSMAN_PORT"));
    assert!(!backend.reachable(&wide("AYW"), 100));
    assert!(sent(&w).is_empty());
}

#[cfg(target_os = "macos")]
mod routing {
    use super::*;
    use wmic_compat::backend::dispatch::{Dispatcher, is_remote};

    fn connection(node: &str, user: Option<&str>) -> wmic_compat::backend::ConnectionOptions {
        wmic_compat::backend::ConnectionOptions {
            node: wide(node),
            user: user.map(wide),
            ..Default::default()
        }
    }

    #[test]
    fn another_node_or_an_account_selects_the_remote_backend() {
        assert!(!is_remote(&connection(".", None)));
        assert!(!is_remote(&connection("localhost", None)));
        assert!(is_remote(&connection("other.example.test", None)));
        assert!(is_remote(&connection(".", Some("user"))));
        assert!(is_remote(&connection("localhost", Some("DOM\\user"))));
    }

    fn dispatcher(wire: &Shared, tunnel: bool) -> Dispatcher {
        let wire = wire.clone();
        Dispatcher::with_connector_and_tunnel(
            Box::new(move |_| {
                wire.borrow_mut().connections += 1;
                Ok(Box::new(FakeTransport(wire.clone())) as Box<dyn Transport>)
            }),
            tunnel,
        )
        .unwrap()
    }

    #[test]
    fn requests_route_by_node_and_account() {
        let w = wire();
        script(
            &w,
            "/Enumerate",
            None,
            vec![ok("enumerate-os"), ok("enumerate-os")],
        );
        w.borrow_mut().server_name = Some("LABHOST".into());
        let mut backend = dispatcher(&w, true);

        let mut other = request("os get Caption");
        other.connection.node = wide("other.example.test");
        let records = objects(backend.execute(&other).unwrap());
        assert_eq!(text(&records[0], "Caption"), "Microsoft Windows 11 Pro");
        assert_eq!(kinds(&w), ["Enumerate"]);

        let mut with_user = request("os get Caption");
        with_user.connection.user = Some(wide("admin"));
        let records = objects(backend.execute(&with_user).unwrap());
        assert_eq!(text(&records[0], "__SERVER"), "LABHOST");
        assert_eq!(kinds(&w), ["Enumerate", "Enumerate"]);

        let local = objects(backend.execute(&request("os get Caption")).unwrap());
        assert!(text(&local[0], "Caption").starts_with("macOS "));
        assert_eq!(kinds(&w).len(), 2, "a local request never reaches the wire");
    }

    #[test]
    fn credentials_reach_the_endpoint() {
        let w = wire();
        let seen: Rc<RefCell<Vec<Endpoint>>> = Rc::default();
        let log = seen.clone();
        let wire2 = w.clone();
        let mut backend = Dispatcher::with_connector(Box::new(move |endpoint| {
            log.borrow_mut().push(endpoint.clone());
            Ok(Box::new(FakeTransport(wire2.clone())) as Box<dyn Transport>)
        }))
        .unwrap();
        script(&w, "/Enumerate", None, vec![ok("enumerate-os")]);
        backend.execute(&remote("os get Caption")).unwrap();
        let seen = seen.borrow();
        assert_eq!(seen[0].host, "AYW");
        assert_eq!(seen[0].user, "admin");
        assert_eq!(seen[0].domain, "AYW");
        assert_eq!(seen[0].password.expose(), &wide("secret"));
        assert_eq!(
            seen[0].port,
            wmic_compat::backend::wsman::listener().unwrap().port
        );
    }

    #[test]
    fn a_local_node_with_an_account_is_refused_unless_the_port_is_named() {
        let w = wire();
        script(&w, "/Enumerate", None, vec![ok("enumerate-os")]);
        let mut with_user = request("os get Caption");
        with_user.connection.user = Some(wide("admin"));
        for node in [".", "localhost"] {
            with_user.connection.node = wide(node);
            let mut backend = dispatcher(&w, false);
            let refused = expect(
                0x80041064,
                "User credentials cannot be used for local connections",
                "query",
            );
            let error = backend.execute(&with_user).unwrap_err();
            assert_eq!(failure(error.clone()), refused, "{node}");
            assert_eq!(error.target, wide(node));
            struct Never;
            impl Confirmation for Never {
                fn confirm(&mut self, _: &WideText) -> Result<bool, ManagementError> {
                    panic!("no prompt")
                }
            }
            let error = backend
                .execute_with_confirmation(&with_user, &mut Never)
                .unwrap_err();
            assert_eq!(failure(error), refused, "{node}");
        }
        assert!(sent(&w).is_empty(), "nothing reaches the wire");

        let mut backend = dispatcher(&w, true);
        with_user.connection.node = wide(".");
        assert!(backend.execute(&with_user).is_ok());
        assert_eq!(kinds(&w), ["Enumerate"], "a named port routes remote");
    }

    #[test]
    fn failfast_probes_the_backend_that_serves_the_request() {
        let w = wire();
        let mut tunnel = dispatcher(&w, true);
        let mut plain = dispatcher(&w, false);
        let local = connection(".", None);
        assert!(plain.reachable_for(&local, 200));
        assert!(tunnel.reachable_for(&local, 200));

        let with_user = connection(".", Some("admin"));
        assert!(
            plain.reachable_for(&with_user, 200),
            "the request is refused before it needs a listener"
        );
        let port = wmic_compat::backend::wsman::listener().unwrap().port;
        assert_eq!(
            tunnel.reachable_for(&with_user, 200),
            listener_reachable("localhost", port, 200),
            "an account on the tunnel probes the listener"
        );
    }
}
