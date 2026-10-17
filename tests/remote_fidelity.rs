//! Output of failing nodes, node lists and method names, pinned to bytes measured on classic WMIC.
//! Node names, the account and the password stand in for the lab values of the measured runs.
mod support;
use std::{fs, path::PathBuf};
use support::wide;
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::OutputError,
    session::{OutputSink, OutputStream, RoutedOutput, Session},
};

const CAPTION: &str = "Microsoft Windows Server 2025 Datacenter Evaluation";
const PATH: &str = r#"\\NODE1\ROOT\CIMV2:Win32_Process.Handle="892""#;
const RPC: &str = "The RPC server is unavailable.";
const RPC_BLOCK: &str = "ERROR:\r\r\nDescription = The RPC server is unavailable.\r\r\n";
const FAILFAST: &str = "RPC Server not available.\r\r\n";
const CREDENTIALS: &str = "User credentials cannot be used for local connections";
const BLANK: &str = "\r\r\n\r\r\n";
const OWNER_BLOCK: &str = concat!(
    "Method execution successful.\r\r\n",
    "Out Parameters:\r\n",
    "instance of __PARAMETERS\r\n{\r\n",
    "\tDomain = \"NT AUTHORITY\";\r\n",
    "\tReturnValue = 0;\r\n",
    "\tUser = \"SYSTEM\";\r\n",
    "};\r\n"
);
const ACCOUNT: &str = r"/user:example\user /password:pw";

fn table() -> String {
    format!("{:<53}\r\r\n{CAPTION}  \r\r\n\r\r\n", "Caption")
}

fn owner(method: &str) -> String {
    format!("Executing ({PATH})->{method}()\r\r\n{OWNER_BLOCK}")
}

struct Failure {
    node: &'static str,
    code: u32,
    message: &'static str,
    operation: &'static str,
}

#[derive(Default)]
struct Fake {
    failing: Vec<Failure>,
    down: Vec<&'static str>,
    seen: Vec<(String, bool)>,
}

impl Fake {
    fn rpc(nodes: &[&'static str]) -> Self {
        Self {
            failing: nodes
                .iter()
                .map(|node| Failure {
                    node,
                    code: 0x800706ba,
                    message: RPC,
                    operation: "query",
                })
                .collect(),
            ..Default::default()
        }
    }

    fn down(nodes: &[&'static str]) -> Self {
        Self {
            down: nodes.to_vec(),
            ..Default::default()
        }
    }
}

impl Management for Fake {
    fn reachable(&mut self, node: &WideText, _: u32) -> bool {
        !self.down.iter().any(|d| *node == wide(d))
    }
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        self.seen
            .push((r.connection.node.to_string_lossy(), r.multiple_nodes));
        if let Some(f) = self
            .failing
            .iter()
            .find(|f| r.connection.node == wide(f.node))
        {
            return Err(ManagementError {
                code: f.code,
                target: r.connection.node.clone(),
                message: f.message.into(),
                operation: f.operation.into(),
                extended: vec![],
                partial: None,
            });
        }
        if r.verb == Verb::Call {
            let property = |name: &str, value: CimValue| Property {
                name: wide(name),
                value,
                cim_type: 8,
                system: false,
                qualifiers: vec![],
            };
            return Ok(ManagementResult::Methods(vec![MethodResult {
                path: wide(if r.class_name == wide("Win32_Service") {
                    "Win32_Service"
                } else {
                    PATH
                }),
                return_value: Some(CimValue::Uint32(0)),
                out_parameters: vec![
                    property("Domain", CimValue::String(wide("NT AUTHORITY"))),
                    property("User", CimValue::String(wide("SYSTEM"))),
                ],
                text: None,
            }]));
        }
        Ok(ManagementResult::Objects(ObjectSet {
            objects: vec![Record {
                class: wide("Win32_OperatingSystem"),
                properties: vec![Property {
                    name: wide("Caption"),
                    value: CimValue::String(wide(CAPTION)),
                    cim_type: 8,
                    system: false,
                    qualifiers: vec![],
                }],
                ..Default::default()
            }],
            schema: None,
        }))
    }
}

/// Both streams, and one stream with the writes in order, as a merged pipe would show them.
#[derive(Default)]
struct Sink {
    stdout: String,
    stderr: String,
    merged: String,
}

impl OutputSink for Sink {
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        let text = text.to_string_lossy();
        match stream {
            OutputStream::Stdout => self.stdout.push_str(&text),
            OutputStream::Stderr => self.stderr.push_str(&text),
        }
        self.merged.push_str(&text);
        Ok(())
    }
    fn record_user(&self) -> WideText {
        wide(r"CORP\admin")
    }
}

struct Run {
    stdout: String,
    stderr: String,
    merged: String,
    status: u32,
}

fn run(backend: &mut Fake, line: &str) -> Run {
    let mut sink = Sink::default();
    let outcome = Session::new(wide("LOCALPC")).unwrap().execute(
        &wide(&format!(" {line}")),
        EntryMode::Interactive,
        backend,
        &mut sink,
    );
    Run {
        stdout: sink.stdout,
        stderr: sink.stderr,
        merged: sink.merged,
        status: outcome.exit_status,
    }
}

fn directory() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "wmic-remote-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

// Case 10: one node, no host, DCOM timeout.
#[test]
fn a_lone_unreachable_node_prints_blank_output_and_the_error_block() {
    let r = run(&mut Fake::rpc(&["NODE2"]), "/node:NODE2 os get Caption");
    assert_eq!(r.stdout, BLANK);
    assert_eq!(r.stderr, format!("Node - NODE2\r\r\n{RPC_BLOCK}"));
    assert_eq!(r.status, 0x800706ba);
}

// Case 11.
#[test]
fn failfast_on_one_node_prints_the_raw_text_without_an_error_block() {
    let r = run(
        &mut Fake::down(&["NODE2"]),
        "/node:NODE2 /failfast:on os get Caption",
    );
    assert_eq!(r.stdout, BLANK);
    assert_eq!(r.stderr, format!("Node - NODE2\r\r\n{FAILFAST}"));
    assert_eq!(r.status, 0xABE3);
}

// Case 12 and E3: a failing node beside a good node prints no blank output of its own.
#[test]
fn a_failing_node_beside_a_good_node_adds_nothing_to_stdout() {
    let r = run(
        &mut Fake::rpc(&["NODE2"]),
        "/node:NODE1,NODE2 os get Caption",
    );
    assert_eq!(r.stdout, table());
    assert_eq!(r.stderr, format!("Node - NODE2\r\r\n{RPC_BLOCK}"));
    assert_eq!(r.status, 0x800706ba);
}

// E3, E3_rev, E3_merge.
#[test]
fn a_credential_error_beside_a_good_node_adds_nothing_to_stdout() {
    let error = format!("Node - LOCALPC\r\r\nERROR:\r\r\nDescription = {CREDENTIALS}\r\r\n");
    for nodes in ["NODE1,LOCALPC", "LOCALPC,NODE1"] {
        let mut backend = Fake::default();
        backend.failing.push(Failure {
            node: "LOCALPC",
            code: 0x80041064,
            message: CREDENTIALS,
            operation: "query",
        });
        let r = run(
            &mut backend,
            &format!("/node:{nodes} {ACCOUNT} os get Caption"),
        );
        assert_eq!(r.stdout, table(), "{nodes}");
        assert_eq!(r.stderr, error, "{nodes}");
        assert_eq!(r.merged, format!("{error}{}", table()), "{nodes}");
        assert_eq!(r.status, 0x80041064, "{nodes}");
    }
}

// E1: two failing nodes with fail-fast. One blank pair for the command, not one per node.
#[test]
fn two_failing_nodes_print_one_blank_pair_after_both_messages() {
    let one = format!("Node - NODE2\r\r\n{FAILFAST}Node - NODE3\r\r\n{FAILFAST}");
    let r = run(
        &mut Fake::down(&["NODE2", "NODE3"]),
        "/node:NODE2,NODE3 /failfast:on os get Caption",
    );
    assert_eq!(r.stdout, BLANK);
    assert_eq!(r.stderr, one);
    assert_eq!(r.merged, format!("{one}{BLANK}"));
    assert_eq!(r.status, 0xABE3);
}

// E2: two failing nodes without fail-fast.
#[test]
fn two_failing_nodes_without_failfast_print_one_blank_pair() {
    let both = format!("Node - NODE2\r\r\n{RPC_BLOCK}Node - NODE3\r\r\n{RPC_BLOCK}");
    let r = run(
        &mut Fake::rpc(&["NODE2", "NODE3"]),
        "/node:NODE2,NODE3 os get Caption",
    );
    assert_eq!(r.stdout, BLANK);
    assert_eq!(r.stderr, both);
    assert_eq!(r.merged, format!("{both}{BLANK}"));
    assert_eq!(r.status, 0x800706ba);
}

// E1_file: the blank pair is one write, so a file holds a byte order mark and one line.
#[test]
fn two_failing_nodes_leave_one_line_in_an_output_file() {
    let dir = directory();
    let file = dir.join("out.txt");
    let mut sink = RoutedOutput::new(Sink::default());
    Session::new(wide("LOCALPC")).unwrap().execute(
        &wide(&format!(
            "/output:\"{}\" /node:NODE2,NODE3 /failfast:on os get Caption",
            file.display()
        )),
        EntryMode::Interactive,
        &mut Fake::down(&["NODE2", "NODE3"]),
        &mut sink,
    );
    assert_eq!(fs::read(&file).unwrap(), [0xff, 0xfe, 0x0d, 0, 0x0a, 0]);
    assert_eq!(sink.inner.stdout, "");
    fs::remove_dir_all(dir).unwrap();
}

// E12b: a failing list view prints the three blank lines of the list format.
#[test]
fn a_failing_list_view_prints_the_list_blank_lines() {
    let mut backend = Fake::default();
    backend.failing.push(Failure {
        node: "NODE1",
        code: 0x80041017,
        message: "Invalid query",
        operation: "query",
    });
    let r = run(&mut backend, "/node:NODE1 os list full");
    assert_eq!(r.stdout, "\r\r\n\r\r\n\r\r\n");
    assert_eq!(
        r.stderr,
        "Node - NODE1\r\r\nERROR:\r\r\nDescription = Invalid query\r\r\n"
    );
    assert_eq!(r.status, 0x80041017);
}

// Case 07: a namespace error names no node and prints no blank output.
#[test]
fn an_invalid_namespace_prints_no_node_line_and_no_blank_output() {
    let mut backend = Fake::default();
    backend.failing.push(Failure {
        node: "NODE1",
        code: 0x8004100e,
        message: "Invalid namespace",
        operation: "connect",
    });
    let r = run(
        &mut backend,
        "/node:NODE1 path Win32_OperatingSystem get Caption",
    );
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ERROR:\r\r\nDescription = Invalid namespace\r\r\n"
    );
    assert_eq!(r.status, 0x8004100e);
}

// E13: a node list prints a Node line on stdout before the output of each node.
#[test]
fn call_on_a_node_list_names_each_node_on_stdout() {
    let call = r#"process where "Name='lsass.exe'" call getowner"#;
    let r = run(
        &mut Fake::down(&["NODE2"]),
        &format!("/node:NODE1,NODE2 /failfast:on {call}"),
    );
    assert_eq!(
        r.stdout,
        format!("Node - NODE1\r\r\n{}Node - NODE2\r\r\n", owner("getowner"))
    );
    assert_eq!(r.stderr, format!("\r\n{FAILFAST}"));
    assert_eq!(
        r.merged,
        format!(
            "Node - NODE1\r\r\n{}\r\nNode - NODE2\r\r\n{FAILFAST}",
            owner("getowner")
        )
    );
    assert_eq!(r.status, 0xABE3);
}

// E13b: the node order sets the order of the lines.
#[test]
fn call_on_a_node_list_keeps_the_node_order() {
    let call = r#"process where "Name='lsass.exe'" call getowner"#;
    let r = run(
        &mut Fake::down(&["NODE2"]),
        &format!("/node:NODE2,NODE1 /failfast:on {call}"),
    );
    assert_eq!(
        r.stdout,
        format!("Node - NODE2\r\r\nNode - NODE1\r\r\n{}", owner("getowner"))
    );
    assert_eq!(r.stderr, format!("{FAILFAST}\r\n"));
    assert_eq!(r.status, 0xABE3);
}

// E13c: without fail-fast the failing node prints the error block on stderr.
#[test]
fn call_on_a_node_list_without_failfast_prints_the_error_block_on_stderr() {
    let call = r#"process where "Name='lsass.exe'" call getowner"#;
    let r = run(
        &mut Fake::rpc(&["NODE2"]),
        &format!("/node:NODE1,NODE2 {call}"),
    );
    assert_eq!(
        r.stdout,
        format!("Node - NODE1\r\r\n{}Node - NODE2\r\r\n", owner("getowner"))
    );
    assert_eq!(r.stderr, format!("\r\n{RPC_BLOCK}"));
    assert_eq!(r.status, 0x800706ba);
}

// E14.
#[test]
fn call_on_one_failfast_node_prints_the_raw_text_on_stderr_only() {
    let r = run(
        &mut Fake::down(&["NODE2"]),
        r#"/node:NODE2 /failfast:on process call create "cmd.exe /c exit 0""#,
    );
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, FAILFAST);
    assert_eq!(r.status, 0xABE3);
}

// E14b.
#[test]
fn call_on_two_failfast_nodes_names_both_on_stdout() {
    let r = run(
        &mut Fake::down(&["NODE2", "NODE3"]),
        r#"/node:NODE2,NODE3 /failfast:on process call create "cmd.exe /c exit 0""#,
    );
    assert_eq!(r.stdout, "Node - NODE2\r\r\nNode - NODE3\r\r\n");
    assert_eq!(r.stderr, format!("{FAILFAST}{FAILFAST}"));
    assert_eq!(r.status, 0xABE3);
}

// E14c.
#[test]
fn call_on_one_unreachable_node_prints_the_error_block_without_a_node_line() {
    let r = run(
        &mut Fake::rpc(&["NODE2"]),
        r#"/node:NODE2 process call create "cmd.exe /c exit 0""#,
    );
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, RPC_BLOCK);
    assert_eq!(r.status, 0x800706ba);
}

/// Replaces the first date and time of the form `MM-DD-YYYYTHH:MM:SS` with the measured one.
fn pin_start_time(text: &str, measured: &str) -> String {
    let bytes = text.as_bytes();
    let shape = b"dd-dd-ddddTdd:dd:dd";
    let at = (0..bytes.len().saturating_sub(shape.len()))
        .find(|&i| {
            shape.iter().enumerate().all(|(k, s)| match s {
                b'd' => bytes[i + k].is_ascii_digit(),
                other => bytes[i + k] == *other,
            })
        })
        .expect("a start time");
    format!("{}{measured}{}", &text[..at], &text[at + shape.len()..])
}

/// The measured document names the password the lab used; this build redacts it.
fn redacted(measured: &str) -> String {
    measured.replace("/password:<pw>", "/password:&lt;redacted&gt;")
}

// E4 and E5: the failing node is a result of the one command document, after the good node.
#[test]
fn rawxml_and_xml_show_the_failing_node_in_the_command_document() {
    for (format, golden, time) in [
        (
            "rawxml",
            include_str!("fixtures/remote-reference/rawxml-failing-node.stdout.bin"),
            "10-17-2026T08:42:37",
        ),
        (
            "xml",
            include_str!("fixtures/remote-reference/xml-failing-node.stdout.bin"),
            "10-17-2026T08:42:42",
        ),
    ] {
        let r = run(
            &mut Fake::down(&["NODE2"]),
            &format!("/node:NODE1,NODE2 /failfast:on {ACCOUNT} os get Caption /format:{format}"),
        );
        let expected = redacted(golden);
        assert_eq!(pin_start_time(&r.stdout, time), expected, "{format}");
        assert_eq!(
            r.stderr,
            format!("Node - NODE2\r\r\n{FAILFAST}"),
            "{format}"
        );
        assert_eq!(r.status, 0xABE3, "{format}");
    }
}

// The node order, not the success of a node, orders the results of the document.
#[test]
fn rawxml_lists_a_failing_first_node_before_the_good_node() {
    let r = run(
        &mut Fake::down(&["NODE2"]),
        "/node:NODE2,NODE1 /failfast:on os get Caption /format:rawxml",
    );
    let failed = r.stdout.find("<RESULTS NODE=\"NODE2\">").unwrap();
    let good = r.stdout.find("<RESULTS NODE=\"NODE1\">").unwrap();
    assert!(failed < good, "{}", r.stdout);
    assert_eq!(r.stdout.matches("<COMMAND ").count(), 1);
}

// A lone failing node in rawxml keeps its own document.
#[test]
fn rawxml_with_one_failing_node_prints_an_error_result() {
    let r = run(
        &mut Fake::down(&["NODE2"]),
        "/node:NODE2 /failfast:on os get Caption /format:rawxml",
    );
    assert!(
        r.stdout.contains(
            "<RESULTS NODE=\"NODE2\">\r\r\n<ERROR><DESCRIPTION>RPC Server not available.\r\r\n</DESCRIPTION>\r\r\n</ERROR>\r\r\n</RESULTS>\r\r\n</COMMAND>\r\r\n\r\r\n"
        ),
        "{}",
        r.stdout
    );
}

fn record_of(line: &str, mut backend: Fake) -> (Run, String) {
    let dir = directory();
    let file = dir.join("record.xml");
    let mut sink = RoutedOutput::new(Sink::default());
    let outcome = Session::new(wide("LOCALPC")).unwrap().execute(
        &wide(&format!(" /record:\"{}\" {line}", file.display())),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
    );
    let text = String::from_utf8(fs::read(&file).unwrap()).unwrap();
    fs::remove_dir_all(dir).unwrap();
    let inner = sink.inner;
    (
        Run {
            stdout: inner.stdout,
            stderr: inner.stderr,
            merged: inner.merged,
            status: outcome.exit_status,
        },
        text,
    )
}

fn record_start_pinned(text: &str) -> String {
    pin_start_time(text, "10-17-2026T08:42:47")
}

// E6: one record for the command; the target is the node list; the text is the stdout.
#[test]
fn a_record_of_a_node_list_holds_the_combined_output_once() {
    let (r, record) = record_of(
        &format!("/node:NODE2,NODE1 /failfast:on {ACCOUNT} os get Caption"),
        Fake::down(&["NODE2"]),
    );
    assert_eq!(r.stdout, table());
    assert_eq!(r.stderr, format!("Node - NODE2\r\r\n{FAILFAST}"));
    assert_eq!(
        record_start_pinned(&record),
        format!(
            concat!(
                "<WMIRECORD USER=\"CORP\\admin\"><RECORD SEQUENCENUM=\"1\" ISSUEDFROM=\"LOCALPC\" ",
                "STARTTIME=\"10-17-2026T08:42:47\"><REQUEST><COMMANDLINE>  ",
                "/node:NODE2,NODE1 /failfast:on /user:example\\user /password:&lt;redacted&gt; ",
                "os get Caption</COMMANDLINE></REQUEST>",
                "<OUTPUT TARGETNODE=\"NODE2, NODE1\" ITERATION=\"1\"><![CDATA[",
                "{:<53}\r\n{}  \r\n]]></OUTPUT></RECORD></WMIRECORD>\r\n"
            ),
            "Caption", CAPTION
        )
    );
}

// E7: every node failed, so the output is the blank pair, recorded as one line.
#[test]
fn a_record_of_failing_nodes_holds_one_blank_line() {
    let (r, record) = record_of(
        "/node:NODE2,NODE3 /failfast:on os get Caption",
        Fake::down(&["NODE2", "NODE3"]),
    );
    assert_eq!(r.stdout, BLANK);
    assert_eq!(
        r.stderr,
        format!("Node - NODE2\r\r\n{FAILFAST}Node - NODE3\r\r\n{FAILFAST}")
    );
    assert_eq!(record.matches("<RECORD ").count(), 1);
    assert!(
        record.contains(
            "<OUTPUT TARGETNODE=\"NODE2, NODE3\" ITERATION=\"1\"><![CDATA[\r\n]]></OUTPUT>"
        ),
        "{record}"
    );
}

// A record of a CALL on a node list holds the Node lines, as they are stdout.
#[test]
fn a_record_of_a_call_holds_the_node_lines() {
    let (_, record) = record_of(
        r#"/node:NODE1,NODE2 /failfast:on process where "Name='lsass.exe'" call getowner"#,
        Fake::down(&["NODE2"]),
    );
    assert_eq!(record.matches("<OUTPUT ").count(), 1);
    assert!(record.contains("TARGETNODE=\"NODE1, NODE2\""), "{record}");
    assert!(record.contains("<![CDATA[Node - NODE1\r\n"), "{record}");
    assert!(record.contains("};\r\nNode - NODE2"), "{record}");
}

// A single node keeps one record named after the node.
#[test]
fn a_record_of_one_node_names_that_node() {
    let (_, record) = record_of("/node:NODE1 os get Caption", Fake::default());
    assert!(record.contains("TARGETNODE=\"NODE1\""), "{record}");
}

#[test]
fn an_unknown_alias_is_named_even_when_a_password_is_given() {
    let r = run(&mut Fake::default(), "/user:u /password:x nope get Name");
    assert_eq!(r.stderr, "nope - Alias not found.\r\r\n");
    assert_eq!(r.status, 0xAC67);
}

#[test]
fn an_unknown_alias_that_equals_the_password_is_withheld() {
    let r = run(&mut Fake::default(), "/user:u /password:nope nope get Name");
    assert_eq!(r.stderr, "Alias not found.\r\r\n");
    assert_eq!(r.status, 0xAC67);
}

// E8 to E10 and E13d: a VerbType 0 verb echoes the casing of its definition; any other echoes
// the typed text. The method that runs stays the one the alias names.
#[test]
fn the_method_name_in_the_executing_line_follows_the_verb_type() {
    let echo = |command: &str| {
        let request = support::request(command);
        (
            request.method.clone().unwrap().to_string_lossy(),
            request.shown_method().to_string_lossy(),
        )
    };
    let pair = |run: &str, echoed: &str| (run.to_owned(), echoed.to_owned());
    assert_eq!(
        echo(r#"sysdriver where "Name='Beep'" call interrogateservice"#),
        pair("InterrogateService", "InterrogateService")
    );
    assert_eq!(
        echo(r#"sysdriver where "Name='Beep'" call INTERROGATESERVICE"#),
        pair("InterrogateService", "InterrogateService")
    );
    assert_eq!(
        echo(r#"process call create "cmd""#),
        pair("Create", "Create")
    );
    assert_eq!(
        echo(r#"process call CREATE "cmd""#),
        pair("Create", "Create")
    );
    assert_eq!(echo("service call create"), pair("Create", "create"));
    assert_eq!(echo("service call CREATE"), pair("Create", "CREATE"));
    assert_eq!(
        echo(r#"process where "Name='x'" call getowner"#),
        pair("GetOwner", "getowner")
    );
    assert_eq!(
        echo(r#"process where "Name='x'" call GETOWNER"#),
        pair("GetOwner", "GETOWNER")
    );
}

#[test]
fn the_executing_line_echoes_the_typed_name_of_an_instance_method() {
    for (typed, shown) in [("getowner", "getowner"), ("GETOWNER", "GETOWNER")] {
        let r = run(
            &mut Fake::default(),
            &format!(r#"/node:NODE1 process where "Name='lsass.exe'" call {typed}"#),
        );
        assert_eq!(r.stdout, owner(shown));
    }
}

#[test]
fn the_executing_line_of_a_static_call_names_the_class() {
    let r = run(&mut Fake::default(), "/node:NODE1 service call create");
    assert!(
        r.stdout
            .starts_with("Executing (Win32_Service)->create()\r\r\nMethod execution successful."),
        "{}",
        r.stdout
    );
}

// The session tells the backend when the command names several nodes.
#[test]
fn the_session_marks_a_node_list_in_the_request() {
    let mut backend = Fake::default();
    run(&mut backend, "/node:NODE1,NODE2 os get Caption");
    assert_eq!(
        backend.seen,
        [("NODE1".to_owned(), true), ("NODE2".to_owned(), true)]
    );
    let mut backend = Fake::default();
    run(&mut backend, "/node:NODE1 os get Caption");
    assert_eq!(backend.seen, [("NODE1".to_owned(), false)]);
}
