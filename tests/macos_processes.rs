#![cfg(target_os = "macos")]
mod support;
use std::{collections::HashMap, io::Cursor, path::Path};
use support::request;
use wmic_compat::{
    backend::{
        Management,
        macos::{
            MacManagement,
            provider::registry,
            sys::{
                accounts, cmdline,
                launchd::{self, Domain, Outcome, RuntimeEntry},
                proc,
            },
        },
    },
    model::*,
};

fn backend() -> MacManagement {
    MacManagement::new().unwrap()
}

fn objects(text: &str) -> Vec<Record> {
    match backend().execute(&request(text)).unwrap() {
        ManagementResult::Objects(set) => set.objects,
        other => panic!("{text} returned {other:?}"),
    }
}

fn invoke(text: &str) -> Vec<MethodResult> {
    match backend().execute(&request(text)).unwrap() {
        ManagementResult::Methods(results) => results,
        other => panic!("{text} returned {other:?}"),
    }
}

fn value<'a>(record: &'a Record, name: &str) -> &'a CimValue {
    &record
        .properties
        .iter()
        .find(|p| !p.system && p.name.eq_ignore_case(name))
        .unwrap_or_else(|| panic!("record lacks {name}"))
        .value
}

fn text(record: &Record, name: &str) -> Option<String> {
    match value(record, name) {
        CimValue::String(s) | CimValue::DateTime(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn number(record: &Record, name: &str) -> Option<u64> {
    match value(record, name) {
        CimValue::Uint32(v) => Some(*v as u64),
        CimValue::Uint64(v) => Some(*v),
        CimValue::Uint16(v) => Some(*v as u64),
        CimValue::Uint8(v) => Some(*v as u64),
        CimValue::Sint16(v) => u64::try_from(*v).ok(),
        _ => None,
    }
}

fn return_value(result: &MethodResult) -> u32 {
    match result.return_value {
        Some(CimValue::Uint32(code)) => code,
        ref other => panic!("unexpected return value {other:?}"),
    }
}

fn out_text(result: &MethodResult, name: &str) -> Option<String> {
    result
        .out_parameters
        .iter()
        .find(|p| p.name.eq_ignore_case(name))
        .and_then(|p| match &p.value {
            CimValue::String(s) => Some(s.to_string_lossy()),
            _ => None,
        })
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Starts `sleep 60` through the provider and returns its pid.
fn spawn_sleeper() -> i32 {
    let results = invoke("process call create \"/bin/sleep 60\"");
    assert_eq!(results.len(), 1);
    assert_eq!(return_value(&results[0]), 0);
    match results[0]
        .out_parameters
        .iter()
        .find(|p| p.name.eq_ignore_case("ProcessId"))
        .map(|p| &p.value)
    {
        Some(CimValue::Uint32(pid)) => *pid as i32,
        other => panic!("missing ProcessId: {other:?}"),
    }
}

/// Kills and reaps a child of this test process; returns the signal that ended it.
fn reap(pid: i32) -> Option<i32> {
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    libc::WIFSIGNALED(status).then(|| libc::WTERMSIG(status))
}

#[test]
fn command_lines_split_like_windows() {
    assert_eq!(cmdline::split("/bin/echo a b"), ["/bin/echo", "a", "b"]);
    assert_eq!(
        cmdline::split("  \"/Applications/My App/run\"   --flag  \"two words\" "),
        ["/Applications/My App/run", "--flag", "two words"]
    );
    assert_eq!(cmdline::split(r#"a "" b"#), ["a", "", "b"]);
    assert_eq!(cmdline::split(r#"say \"hi\""#), ["say", "\"hi\""]);
    assert_eq!(cmdline::split(r#""C:\dir\\" next"#), ["C:\\dir\\", "next"]);
    assert_eq!(cmdline::split(r"C:\dir\file x"), ["C:\\dir\\file", "x"]);
    assert_eq!(cmdline::split(r#"a\\\"b c"#), ["a\\\"b", "c"]);
    assert!(cmdline::split("   ").is_empty());
}

#[test]
fn quoting_round_trips_through_split() {
    let arguments = [
        "plain",
        "two words",
        "",
        "has \"quote\"",
        "trailing\\",
        "trailing space\\",
        "a\\\"b",
        "tab\there",
    ];
    let line = cmdline::join(&arguments);
    assert_eq!(cmdline::split(&line), arguments);
    assert_eq!(cmdline::quote("plain"), "plain");
    assert_eq!(cmdline::quote("a b"), "\"a b\"");
}

#[test]
fn process_argument_buffers_skip_the_path_and_environment() {
    let mut buffer = 3i32.to_ne_bytes().to_vec();
    buffer.extend(b"/usr/bin/tool\0\0\0\0tool\0-v\0two words\0HOME=/Users/x\0PATH=/bin\0");
    assert_eq!(
        proc::parse_arguments(&buffer).unwrap(),
        ["tool", "-v", "two words"]
    );
    assert_eq!(proc::parse_arguments(&buffer[..2]), None);
    let mut truncated = 2i32.to_ne_bytes().to_vec();
    truncated.extend(b"/bin/x\0\0x\0");
    assert_eq!(proc::parse_arguments(&truncated).unwrap(), ["x", ""]);
}

const PRINT: &str = "system = {\n\ttype = system\n\thandle = 0\n\tactive count = 1058\n\tservice count = 6\n\tservices = {\n\t\t       0      - \tcom.apple.backgroundassets.managed.relay.service\n\t\t     536      - \tcom.apple.runningboardd\n\t\t       0   (pe) \tcom.apple.kernelmanager_helper\n\t\t       0      0 \tcom.apple.wifiFirmwareLoader\n\t\t   16229    -15 \tcom.apple.XprotectFramework.PluginService\n\t\t     453      - \tcom.apple.fseventsd\n\t}\n\n\tattractive services = {\n\t\t       0      - \tcom.example.not.a.service\n\t}\n\n\tdisabled services = {\n\t\t\"com.apple.ftpd\" => disabled\n\t}\n}\n";

const DISABLED: &str = "\n\tdisabled services = {\n\t\t\"com.apple.AEServer\" => enabled\n\t\t\"com.apple.CSCSupportd\" => disabled\n\t\t\"com.openssh.sshd\" => enabled\n\t\t\"odd\" => 1\n\t}\n";

#[test]
fn launchctl_service_rows_parse() {
    let rows = launchd::parse_services(PRINT);
    assert_eq!(rows.len(), 6);
    assert_eq!(
        rows[0],
        RuntimeEntry {
            pid: 0,
            status: None,
            label: "com.apple.backgroundassets.managed.relay.service".into()
        }
    );
    assert_eq!(rows[1].pid, 536);
    assert_eq!(rows[2].status, None);
    assert_eq!(rows[3].status, Some(0));
    assert_eq!(rows[4].status, Some(-15));
    assert_eq!(rows[5].label, "com.apple.fseventsd");
    assert!(launchd::parse_services("Bad request.\n").is_empty());
    let listing = launchd::parse_services("\tservices = {\n\t\t-\t0\tcom.example.idle\n\t}\n");
    assert_eq!(listing[0].pid, 0);
}

#[test]
fn launchctl_disabled_overrides_parse() {
    let overrides = launchd::parse_disabled(DISABLED);
    assert_eq!(overrides.len(), 3);
    assert!(overrides["com.apple.CSCSupportd"]);
    assert!(!overrides["com.apple.AEServer"]);
    assert!(!overrides["com.openssh.sshd"]);
    assert!(launchd::parse_disabled("Could not find domain\n").is_empty());
}

const DAEMON: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.example.daemon</string>
<key>Program</key><string>/usr/local/bin/daemon tool</string>
<key>ProgramArguments</key><array><string>daemon</string><string>--serve</string><string>a b</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>UserName</key><string>_example</string>
</dict></plist>"#;

const AGENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.example.agent</string>
<key>ProgramArguments</key><array><string>/usr/bin/true</string></array>
<key>Disabled</key><true/>
</dict></plist>"#;

fn definition(xml: &str) -> launchd::Definition {
    let value = plist::Value::from_reader(Cursor::new(xml)).unwrap();
    launchd::parse_definition(&value, Path::new("/x/y.plist")).unwrap()
}

#[test]
fn job_definitions_read_launchd_keys() {
    let daemon = definition(DAEMON);
    assert_eq!(daemon.label, "com.example.daemon");
    assert!(daemon.run_at_load && daemon.keep_alive && !daemon.disabled);
    assert_eq!(daemon.user_name.as_deref(), Some("_example"));
    assert_eq!(
        daemon.command_line().unwrap(),
        "\"/usr/local/bin/daemon tool\" --serve \"a b\""
    );
    let agent = definition(AGENT);
    assert!(agent.disabled && !agent.run_at_load && !agent.keep_alive);
    assert_eq!(agent.command_line().unwrap(), "/usr/bin/true");
    let no_label = plist::Value::from_reader(Cursor::new(
        "<plist version=\"1.0\"><dict><key>Program</key><string>/bin/x</string></dict></plist>",
    ))
    .unwrap();
    let unnamed = launchd::parse_definition(&no_label, Path::new("/x/com.example.file.plist"));
    assert_eq!(unnamed.unwrap().label, "com.example.file");
}

#[test]
fn definitions_load_every_plist_in_a_directory() {
    let directory = std::env::temp_dir().join(format!("wmic-launchd-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("b.plist"), AGENT).unwrap();
    std::fs::write(directory.join("a.plist"), DAEMON).unwrap();
    std::fs::write(directory.join("broken.plist"), "not a plist").unwrap();
    std::fs::write(directory.join("note.txt"), AGENT).unwrap();
    let found = launchd::definitions(&directory);
    std::fs::remove_dir_all(&directory).unwrap();
    let labels: Vec<_> = found.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels, ["com.example.daemon", "com.example.agent"]);
    assert!(launchd::definitions(Path::new("/nonexistent-wmic-dir")).is_empty());
}

fn row(pid: u32, status: Option<i32>, label: &str) -> RuntimeEntry {
    RuntimeEntry {
        pid,
        status,
        label: label.into(),
    }
}

#[test]
fn service_inventory_joins_runtime_overrides_and_plists() {
    let overrides = HashMap::from([("com.example.agent".to_owned(), false)]);
    let services = launchd::merge(
        &Domain::System,
        Some(vec![
            row(77, Some(0), "com.example.daemon"),
            row(0, None, "com.example.submitted"),
        ]),
        &overrides,
        vec![definition(DAEMON), definition(AGENT)],
    );
    let find = |label: &str| services.iter().find(|s| s.label == label).unwrap();
    let daemon = find("com.example.daemon");
    assert!(daemon.running() && daemon.loaded() && !daemon.disabled);
    let agent = find("com.example.agent");
    assert!(!agent.running() && !agent.loaded());
    assert!(!agent.disabled, "the override database wins over the plist");
    let submitted = find("com.example.submitted");
    assert!(submitted.definition.is_none() && submitted.loaded() && !submitted.running());

    let unknown = launchd::merge(
        &Domain::Gui(501),
        None,
        &HashMap::new(),
        vec![definition(AGENT)],
    );
    assert!(unknown[0].runtime.is_none());
    assert!(unknown[0].disabled);
}

#[test]
fn a_label_in_two_domains_keeps_the_running_copy() {
    let system = launchd::merge(
        &Domain::System,
        Some(vec![row(0, None, "com.example.both")]),
        &HashMap::new(),
        vec![],
    );
    let gui = launchd::merge(
        &Domain::Gui(501),
        Some(vec![
            row(9, None, "com.example.both"),
            row(0, None, "com.example.a"),
        ]),
        &HashMap::new(),
        vec![],
    );
    let combined = launchd::combine(vec![system, gui]);
    assert_eq!(combined.len(), 2);
    assert_eq!(combined[0].label, "com.example.a");
    assert_eq!(combined[1].domain, Domain::Gui(501));
    let reversed = launchd::combine(vec![
        launchd::merge(
            &Domain::System,
            Some(vec![row(0, None, "x")]),
            &HashMap::new(),
            vec![],
        ),
        launchd::merge(
            &Domain::Gui(501),
            Some(vec![row(0, None, "x")]),
            &HashMap::new(),
            vec![],
        ),
    ]);
    assert_eq!(reversed[0].domain, Domain::System);
}

#[test]
fn service_commands_address_the_owning_domain() {
    let system = launchd::merge(
        &Domain::System,
        Some(vec![]),
        &HashMap::new(),
        vec![definition(DAEMON)],
    )
    .remove(0);
    assert_eq!(
        system.kickstart_arguments(),
        ["kickstart", "system/com.example.daemon"]
    );
    assert_eq!(
        system.bootstrap_arguments().unwrap(),
        ["bootstrap", "system", "/x/y.plist"]
    );
    assert_eq!(
        system.stop_arguments(),
        ["bootout", "system/com.example.daemon"]
    );
    assert_eq!(
        system.enable_arguments(false),
        ["disable", "system/com.example.daemon"]
    );
    assert_eq!(
        system.enable_arguments(true),
        ["enable", "system/com.example.daemon"]
    );
    let agent = launchd::merge(
        &Domain::Gui(501),
        Some(vec![]),
        &HashMap::new(),
        vec![definition(AGENT)],
    )
    .remove(0);
    assert_eq!(
        agent.stop_arguments(),
        ["kill", "SIGTERM", "gui/501/com.example.agent"]
    );
    let submitted = launchd::merge(
        &Domain::System,
        Some(vec![row(5, None, "com.example.submitted")]),
        &HashMap::new(),
        vec![],
    )
    .remove(0);
    assert!(submitted.bootstrap_arguments().is_none());
}

#[test]
fn launchctl_failures_map_to_service_return_codes() {
    let failed = |stderr: &str| Outcome {
        success: false,
        stdout: String::new(),
        stderr: stderr.into(),
    };
    let ok = Outcome {
        success: true,
        ..Default::default()
    };
    assert_eq!(launchd::return_value(&ok), 0);
    assert_eq!(
        launchd::return_value(&failed(
            "Could not signal service: 1: Operation not permitted"
        )),
        2
    );
    assert_eq!(
        launchd::return_value(&failed("Not privileged to signal service.")),
        2
    );
    assert_eq!(
        launchd::return_value(&failed(
            "Bootstrap failed: 1: Operation not permitted while System Integrity Protection is engaged"
        )),
        2
    );
    assert_eq!(
        launchd::return_value(&failed("Could not find service \"x\" in domain for system")),
        6
    );
    assert_eq!(
        launchd::return_value(&failed("Could not signal service: 3: No such process")),
        6
    );
    assert_eq!(
        launchd::return_value(&failed("Bootstrap failed: 5: Input/output error")),
        8
    );
    assert_eq!(launchd::return_value(&Outcome::default()), 8);
}

#[test]
fn process_table_includes_this_process_with_details() {
    let pid = std::process::id();
    let records = objects(&format!(
        "process where \"ProcessId={pid}\" get Handle,ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine,CreationDate,Priority,ThreadCount,WorkingSetSize,VirtualSize,UserModeTime,KernelModeTime,PageFaults,CSName,OSName,WindowsVersion,Caption,Description"
    ));
    assert_eq!(records.len(), 1);
    let me = &records[0];
    assert_eq!(text(me, "Handle"), Some(pid.to_string()));
    assert_eq!(
        number(me, "ParentProcessId"),
        Some(unsafe { libc::getppid() } as u64)
    );
    let executable = std::env::current_exe().unwrap();
    let file = executable
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(text(me, "Name"), Some(file.clone()));
    assert_eq!(text(me, "Caption"), Some(file.clone()));
    assert_eq!(text(me, "Description"), Some(file.clone()));
    assert!(text(me, "ExecutablePath").unwrap().ends_with(&file));
    let line = text(me, "CommandLine").unwrap();
    assert!(line.contains(&file), "{line}");
    let created = text(me, "CreationDate").unwrap();
    assert_eq!(created.len(), 25, "{created}");
    assert_eq!(number(me, "Priority"), Some(8));
    assert!(number(me, "ThreadCount").unwrap() >= 1);
    assert!(number(me, "WorkingSetSize").unwrap() > 0);
    assert!(number(me, "VirtualSize").unwrap() > 0);
    assert!(number(me, "UserModeTime").is_some() && number(me, "KernelModeTime").is_some());
    assert!(number(me, "PageFaults").is_some());
    assert!(text(me, "OSName").unwrap().starts_with("macOS "));
    assert!(text(me, "OSName").unwrap().ends_with("|/|/System"));
    assert!(text(me, "WindowsVersion").unwrap().contains('.'));
    let server = text(&objects("computersystem get Name")[0], "Name").unwrap();
    assert!(text(me, "CSName").unwrap().eq_ignore_ascii_case(&server));
}

#[test]
fn process_table_lists_launchd_and_unreadable_values_are_null() {
    let all = objects(
        "process get ProcessId,Name,ExecutablePath,CommandLine,HandleCount,SessionId,Status",
    );
    assert!(all.len() > 20);
    let launchd = all
        .iter()
        .find(|r| number(r, "ProcessId") == Some(1))
        .expect("launchd is listed");
    assert_eq!(text(launchd, "Name").as_deref(), Some("launchd"));
    assert_eq!(
        text(launchd, "ExecutablePath").as_deref(),
        Some("/sbin/launchd")
    );
    if !is_root() {
        assert_eq!(*value(launchd, "CommandLine"), CimValue::Null);
    }
    for record in &all {
        assert_eq!(*value(record, "HandleCount"), CimValue::Null);
        assert_eq!(*value(record, "SessionId"), CimValue::Null);
        assert_eq!(*value(record, "Status"), CimValue::Null);
    }
    let by_name = objects("process where \"Name='launchd'\" get ProcessId,ExecutablePath");
    assert!(by_name.iter().any(|r| number(r, "ProcessId") == Some(1)));
}

#[test]
fn process_instance_path_selects_one_handle() {
    let pid = std::process::id();
    let hit = objects(&format!(
        "path Win32_Process.Handle=\"{pid}\" get ProcessId"
    ));
    assert_eq!(hit.len(), 1);
    assert_eq!(number(&hit[0], "ProcessId"), Some(pid as u64));
    assert!(objects("path Win32_Process.Handle=\"999999999\" get ProcessId").is_empty());
}

#[test]
fn methods_are_reported_implemented() {
    for (class, methods) in [
        (
            "Win32_Process",
            &["Create", "Terminate", "GetOwner", "SetPriority"][..],
        ),
        (
            "Win32_Service",
            &["StartService", "StopService", "ChangeStartMode"][..],
        ),
        (
            "Win32_OperatingSystem",
            &[
                "Reboot",
                "Shutdown",
                "Win32Shutdown",
                "Win32ShutdownTracker",
            ][..],
        ),
    ] {
        let schema = &registry().get(class).unwrap().info.schema;
        for name in methods {
            let method = schema
                .methods
                .iter()
                .find(|m| m.name.eq_ignore_case(name))
                .unwrap();
            let implemented = method
                .qualifiers
                .iter()
                .find(|q| q.name.eq_ignore_case("implemented"))
                .unwrap();
            assert_eq!(implemented.value, CimValue::Boolean(true), "{class}.{name}");
        }
    }
}

#[test]
fn create_runs_a_program_and_terminate_kills_it() {
    let pid = spawn_sleeper();
    assert!(pid > 1);
    let listed = objects(&format!(
        "path Win32_Process.Handle=\"{pid}\" get Name,CommandLine,ParentProcessId"
    ));
    assert_eq!(listed.len(), 1);
    assert_eq!(text(&listed[0], "Name").as_deref(), Some("sleep"));
    assert_eq!(
        text(&listed[0], "CommandLine").as_deref(),
        Some("/bin/sleep 60")
    );
    let results = invoke(&format!("process where \"Handle='{pid}'\" call terminate"));
    assert_eq!(results.len(), 1);
    assert_eq!(return_value(&results[0]), 0);
    assert!(
        results[0]
            .path
            .to_string_lossy()
            .ends_with(&format!("Win32_Process.Handle=\"{pid}\""))
    );
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    assert!(libc::WIFSIGNALED(status));
    assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
}

#[test]
fn terminate_through_an_object_path() {
    let pid = spawn_sleeper();
    let results = invoke(&format!(
        "path Win32_Process.Handle=\"{pid}\" call terminate"
    ));
    assert_eq!(return_value(&results[0]), 0);
    assert_eq!(reap(pid), Some(libc::SIGKILL));
}

#[test]
fn create_detaches_the_child_from_the_session_and_standard_streams() {
    let pid = spawn_sleeper();
    assert_eq!(unsafe { libc::getsid(pid) }, pid);
    let mut output = vec![0u8; 0];
    let listing = std::process::Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-Fn", "-d", "0,1,2"])
        .output();
    if let Ok(listing) = listing {
        output = listing.stdout;
    }
    reap(pid);
    let names = String::from_utf8_lossy(&output);
    for line in names.lines().filter(|l| l.starts_with('n')) {
        assert_eq!(line, "n/dev/null", "{names}");
    }
}

#[test]
fn create_honors_the_working_directory() {
    let marker = std::env::temp_dir().join(format!("wmic-cwd-{}", std::process::id()));
    std::fs::create_dir_all(&marker).unwrap();
    let results = invoke(&format!(
        "process call create \"/usr/bin/touch created.txt\",\"{}\"",
        marker.display()
    ));
    assert_eq!(return_value(&results[0]), 0);
    let pid = match &results[0].out_parameters[0].value {
        CimValue::Uint32(pid) => *pid as i32,
        other => panic!("{other:?}"),
    };
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    let created = marker.join("created.txt").exists();
    std::fs::remove_dir_all(&marker).unwrap();
    assert!(created);
}

#[test]
fn create_reports_failures_as_win32_codes() {
    let code = |line: &str| return_value(&invoke(&format!("process call create {line}"))[0]);
    assert_eq!(code("\"/nonexistent/wmic-program\""), 9);
    assert_eq!(code("\"wmic-no-such-program-anywhere\""), 9);
    assert_eq!(code("\"/bin/sleep 1\",\"/nonexistent/directory\""), 9);
    assert_eq!(code("\"/etc/hosts\""), 2);
    assert_eq!(code("\"   \""), 21);
}

#[test]
fn create_has_no_shell() {
    let marker = std::env::temp_dir().join(format!("wmic-shell-{}", std::process::id()));
    let results = invoke(&format!(
        "process call create \"/bin/echo hi > {}\"",
        marker.display()
    ));
    assert_eq!(return_value(&results[0]), 0);
    let pid = match &results[0].out_parameters[0].value {
        CimValue::Uint32(pid) => *pid as i32,
        other => panic!("{other:?}"),
    };
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    assert!(!marker.exists());
}

#[test]
fn terminate_refuses_launchd_and_unknown_targets() {
    let results = invoke("path Win32_Process.Handle=\"1\" call terminate");
    assert_eq!(return_value(&results[0]), 3);
    let none = backend()
        .execute(&request(
            "path Win32_Process.Handle=\"999999999\" call terminate",
        ))
        .unwrap();
    assert!(matches!(
        none,
        ManagementResult::Operation(_) | ManagementResult::Methods(_)
    ));
}

#[test]
fn get_owner_reports_the_account_and_the_server() {
    let pid = std::process::id();
    let results = invoke(&format!("process where \"ProcessId={pid}\" call getowner"));
    assert_eq!(return_value(&results[0]), 0);
    let uid = unsafe { libc::getuid() };
    assert_eq!(out_text(&results[0], "User"), accounts::user_name(uid));
    let server = text(&objects("computersystem get Name")[0], "Name").unwrap();
    assert!(
        out_text(&results[0], "Domain")
            .unwrap()
            .eq_ignore_ascii_case(&server)
    );
    let launchd = invoke("process where \"ProcessId=1\" call getowner");
    assert_eq!(return_value(&launchd[0]), 0);
    assert_eq!(out_text(&launchd[0], "User").as_deref(), Some("root"));
}

#[test]
fn set_priority_maps_windows_classes_to_nice_values() {
    let pid = spawn_sleeper();
    let set = |class: i32| {
        return_value(
            &invoke(&format!(
                "process where \"Handle='{pid}'\" call setpriority {class}"
            ))[0],
        )
    };
    let nice = || unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
    assert_eq!(set(16384), 0);
    assert_eq!(nice(), 10);
    assert_eq!(set(64), 0);
    assert_eq!(nice(), 19);
    assert_eq!(set(5), 21);
    assert_eq!(nice(), 19);
    if !is_root() {
        assert_eq!(set(32), 2, "lowering the nice value needs privilege");
        assert_eq!(nice(), 19);
    }
    reap(pid);
}

#[test]
fn shutdown_methods_validate_flags_and_privilege() {
    let call = |text: &str| return_value(&invoke(&format!("os call {text}"))[0]);
    assert_eq!(call("win32shutdown 3"), 87);
    assert_eq!(call("win32shutdown 16"), 87);
    assert_eq!(call("win32shutdown 0"), 87);
    if !is_root() {
        assert_eq!(call("reboot"), 5);
        assert_eq!(call("shutdown"), 5);
        assert_eq!(call("win32shutdown 1"), 5);
        assert_eq!(call("win32shutdown 6"), 5);
        assert_eq!(call("win32shutdown 8"), 5);
    }
}

#[test]
fn services_are_listed_with_launchd_state() {
    let all = objects(
        "service get Name,DisplayName,Caption,State,Started,ProcessId,StartMode,StartName,Status,PathName,ServiceType,SystemName,ExitCode",
    );
    assert!(all.len() > 50, "{} services", all.len());
    let mut names: Vec<_> = all.iter().filter_map(|r| text(r, "Name")).collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total, "service names are unique");
    for record in &all {
        let status = text(record, "Status").unwrap();
        assert!(status == "OK" || status == "Unknown");
        assert_eq!(text(record, "DisplayName"), text(record, "Name"));
        assert_eq!(text(record, "ServiceType").as_deref(), Some("Own Process"));
        assert!(matches!(
            text(record, "StartMode").as_deref(),
            Some("Auto" | "Manual" | "Disabled")
        ));
        match text(record, "State").as_deref() {
            Some("Running") => {
                assert_eq!(*value(record, "Started"), CimValue::Boolean(true));
                assert!(number(record, "ProcessId").unwrap() > 0);
            }
            Some("Stopped") => {
                assert_eq!(*value(record, "Started"), CimValue::Boolean(false));
                assert_eq!(number(record, "ProcessId"), Some(0));
            }
            None => assert_eq!(status, "Unknown"),
            other => panic!("state {other:?}"),
        }
    }
    let running = objects("service where \"State='Running'\" get Name");
    assert!(!running.is_empty());
}

#[test]
fn a_running_system_service_cannot_start_again_or_change_without_privilege() {
    let label = "com.apple.fseventsd";
    let selector = format!("service where \"Name='{label}'\"");
    if objects(&format!("{selector} get State"))
        .first()
        .and_then(|r| text(r, "State"))
        .as_deref()
        != Some("Running")
    {
        return;
    }
    assert_eq!(
        return_value(&invoke(&format!("{selector} call startservice"))[0]),
        10
    );
    assert_eq!(
        return_value(&invoke(&format!("{selector} call changestartmode Bogus"))[0]),
        21
    );
    if !is_root() {
        assert_eq!(
            return_value(&invoke(&format!("{selector} call stopservice"))[0]),
            2
        );
        assert_eq!(
            return_value(&invoke(&format!("{selector} call changestartmode Disabled"))[0]),
            2
        );
    }
}

#[test]
fn stopping_a_stopped_service_is_not_active() {
    let stopped = objects("service where \"State='Stopped'\" get Name");
    let Some(first) = stopped.first() else {
        return;
    };
    let name = text(first, "Name").unwrap();
    let results = invoke(&format!("service where \"Name='{name}'\" call stopservice"));
    assert_eq!(return_value(&results[0]), 6);
}

#[test]
fn user_accounts_list_regular_accounts_only() {
    let users = objects(
        "useraccount get Name,Domain,Caption,FullName,AccountType,LocalAccount,SID,Status,Disabled,Lockout,SIDType",
    );
    assert!(!users.is_empty());
    let uid = unsafe { libc::getuid() };
    let me = accounts::user_name(uid).unwrap();
    let server = text(&objects("computersystem get Name")[0], "Name").unwrap();
    for user in &users {
        let name = text(user, "Name").unwrap();
        assert!(!name.starts_with('_'), "{name}");
        let domain = text(user, "Domain").unwrap();
        assert!(domain.eq_ignore_ascii_case(&server));
        assert_eq!(text(user, "Caption").unwrap(), format!("{domain}\\{name}"));
        assert_eq!(number(user, "AccountType"), Some(512));
        assert_eq!(*value(user, "LocalAccount"), CimValue::Boolean(true));
        assert_eq!(*value(user, "SID"), CimValue::Null);
        assert_eq!(*value(user, "Disabled"), CimValue::Null);
        assert_eq!(*value(user, "Lockout"), CimValue::Boolean(false));
        assert_eq!(text(user, "Status").as_deref(), Some("OK"));
    }
    assert!(
        users
            .iter()
            .any(|u| text(u, "Name").as_deref() == Some("root"))
    );
    if uid >= 500 {
        let mine = users
            .iter()
            .find(|u| text(u, "Name").as_deref() == Some(&me))
            .unwrap();
        assert!(text(mine, "FullName").is_some());
    }
    assert!(
        users
            .iter()
            .all(|u| text(u, "Name").as_deref() != Some("daemon"))
    );
}

#[test]
fn groups_exclude_directory_service_groups() {
    let groups = objects("group get Name,Domain,Caption,LocalAccount,SID,Status");
    assert!(!groups.is_empty());
    for group in &groups {
        let name = text(group, "Name").unwrap();
        assert!(
            !name.starts_with('_') && !name.starts_with("com.apple."),
            "{name}"
        );
        assert_eq!(
            text(group, "Caption").unwrap(),
            format!("{}\\{name}", text(group, "Domain").unwrap())
        );
        assert_eq!(*value(group, "SID"), CimValue::Null);
        assert_eq!(*value(group, "LocalAccount"), CimValue::Boolean(true));
    }
    assert!(
        groups
            .iter()
            .any(|g| text(g, "Name").as_deref() == Some("staff"))
    );
}

#[test]
fn startup_commands_come_from_run_at_load_agents() {
    for record in
        objects("startup get Name,Command,Location,User,Caption,Description,UserSID,SettingID")
    {
        let location = text(&record, "Location").unwrap();
        assert!(location.ends_with("/Library/LaunchAgents"), "{location}");
        let user = text(&record, "User").unwrap();
        assert!(user == "Public" || !user.is_empty());
        assert!(!text(&record, "Command").unwrap().is_empty());
        assert_eq!(text(&record, "Name"), text(&record, "Caption"));
        assert_eq!(*value(&record, "UserSID"), CimValue::Null);
    }
}

#[test]
fn products_come_from_application_bundles() {
    let products = objects(
        "product get Name,Version,IdentifyingNumber,InstallLocation,InstallDate,PackageName,InstallState,Vendor",
    );
    for product in &products {
        let location = text(product, "InstallLocation").unwrap();
        assert!(location.ends_with(".app"), "{location}");
        assert!(Path::new(&location).is_dir());
        assert!(!text(product, "Name").unwrap().is_empty());
        let date = text(product, "InstallDate").unwrap();
        assert!(
            date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()),
            "{date}"
        );
        assert_eq!(number(product, "InstallState"), Some(5));
        assert_eq!(*value(product, "Vendor"), CimValue::Null);
        let package = text(product, "PackageName").unwrap();
        assert!(location.ends_with(&package));
    }
    if Path::new("/Applications/Safari.app/Contents/Info.plist").exists() {
        let safari = products
            .iter()
            .find(|p| text(p, "IdentifyingNumber").as_deref() == Some("com.apple.Safari"))
            .unwrap();
        assert_eq!(text(safari, "Name").as_deref(), Some("Safari"));
        assert!(text(safari, "Version").is_some());
    }
}

#[test]
fn quick_fixes_come_from_the_install_history() {
    let fixes = objects("qfe get HotFixID,Description,InstalledOn,CSName,Caption,InstalledBy");
    if !Path::new("/Library/Receipts/InstallHistory.plist").exists() {
        assert!(fixes.is_empty());
        return;
    }
    for fix in &fixes {
        assert!(!text(fix, "HotFixID").unwrap().is_empty());
        assert_eq!(text(fix, "Description").as_deref(), Some("Update"));
        assert_eq!(*value(fix, "Caption"), CimValue::Null);
        assert_eq!(*value(fix, "InstalledBy"), CimValue::Null);
        if let Some(date) = text(fix, "InstalledOn") {
            let parts: Vec<_> = date.split('/').collect();
            assert_eq!(parts.len(), 3, "{date}");
            assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{date}");
            assert!(
                !parts[0].starts_with('0') && !parts[1].starts_with('0'),
                "{date}"
            );
            assert_eq!(parts[2].len(), 4);
        }
    }
}
