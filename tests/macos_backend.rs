#![cfg(target_os = "macos")]
mod support;
use support::{request, wide};
use wmic_compat::{
    backend::{
        Confirmation, Management, ResolvedRequest,
        macos::{MacManagement, provider::registry},
    },
    model::*,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream, Session},
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

fn failure(request: &ResolvedRequest) -> ManagementError {
    backend().execute(request).unwrap_err()
}

fn operation(error: &ManagementError) -> String {
    error.operation.to_string_lossy()
}

fn text(record: &Record, name: &str) -> Option<String> {
    record
        .properties
        .iter()
        .find(|p| p.name.eq_ignore_case(name))
        .and_then(|p| match &p.value {
            CimValue::String(s) | CimValue::DateTime(s) => Some(s.to_string_lossy()),
            _ => None,
        })
}

/// Whether the variant is one the CIM type allows.
fn consistent(value: &CimValue, cim_type: u32) -> bool {
    if let CimValue::Null | CimValue::Empty = value {
        return true;
    }
    if let CimValue::Array(items) = value {
        return cim_type & 0x2000 != 0 && items.iter().all(|i| consistent(i, cim_type & !0x2000));
    }
    cim_type & 0x2000 == 0
        && matches!(
            (cim_type & 0xfff, value),
            (2, CimValue::Sint16(_))
                | (3, CimValue::Sint32(_))
                | (4, CimValue::Real32(_))
                | (5, CimValue::Real64(_))
                | (8, CimValue::String(_))
                | (11, CimValue::Boolean(_))
                | (13, CimValue::Object(_))
                | (16, CimValue::Sint8(_))
                | (17, CimValue::Uint8(_))
                | (18, CimValue::Uint16(_))
                | (19, CimValue::Uint32(_))
                | (20, CimValue::Sint64(_))
                | (21, CimValue::Uint64(_))
                | (101, CimValue::DateTime(_))
                | (102, CimValue::Reference(_))
                | (103, CimValue::Char16(_))
        )
}

#[derive(Default)]
struct Sink {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
impl OutputSink for Sink {
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        let bytes = encode_output(text, &DestinationProfile::pipe(1252))?;
        match stream {
            OutputStream::Stdout => self.stdout.extend(bytes),
            OutputStream::Stderr => self.stderr.extend(bytes),
        }
        Ok(())
    }
}

fn session(node: &str, command: &str) -> (u32, Sink) {
    let mut sink = Sink::default();
    let outcome = Session::new(wide(node)).unwrap().execute(
        &wide(command),
        EntryMode::Interactive,
        &mut backend(),
        &mut sink,
    );
    (outcome.exit_status, sink)
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

#[test]
fn a_projection_reduces_the_class_metadata() {
    let metadata = |command: &str| {
        let record = objects(command).remove(0);
        let value = |name: &str| {
            record
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_case(name))
                .unwrap()
                .value
                .clone()
        };
        (
            value("__DERIVATION"),
            value("__PROPERTY_COUNT"),
            value("__DYNASTY"),
            value("__SUPERCLASS"),
            text(&record, "__PATH").unwrap(),
            text(&record, "__CLASS").unwrap(),
            record.properties.iter().filter(|p| !p.system).count(),
        )
    };
    for (command, count) in [
        ("os get Caption", 1),
        ("path Win32_OperatingSystem get Caption", 1),
        ("os get __SERVER,__DERIVATION", 0),
    ] {
        let (derivation, counted, dynasty, superclass, path, class, _) = metadata(command);
        let named = command.contains("__DERIVATION");
        assert_eq!(
            matches!(&derivation, CimValue::Array(items) if !items.is_empty()),
            named,
            "{command}"
        );
        assert_eq!(counted, CimValue::Sint32(count), "{command}");
        assert_eq!(dynasty, CimValue::String(wide("")), "{command}");
        assert_eq!(
            superclass,
            CimValue::String(wide("")),
            "{command}: only a named __SUPERCLASS is filled"
        );
        assert!(
            path.ends_with(":Win32_OperatingSystem=@"),
            "{command}: {path}"
        );
        assert_eq!(class, "Win32_OperatingSystem");
    }
    let (_, count, _, superclass, ..) = metadata("os get Caption,__SUPERCLASS");
    assert_eq!(count, CimValue::Sint32(1));
    assert_eq!(superclass, CimValue::String(wide("CIM_OperatingSystem")));
    let (_, brief, ..) = metadata("os list brief");
    assert_ne!(brief, CimValue::Sint32(0));
    let (derivation, count, dynasty, superclass, _, _, plain) =
        metadata("process where \"ProcessId=1\" get Name");
    assert_eq!(derivation, CimValue::Array(vec![]));
    assert_eq!(dynasty, CimValue::String(wide("")));
    assert_eq!(superclass, CimValue::String(wide("")));
    assert_eq!(plain, 1, "the projection keeps only the named properties");
    assert_eq!(
        count,
        CimValue::Sint32(2),
        "the key property Handle counts although it is not shown"
    );
    for command in ["os get", "os get *"] {
        let (derivation, count, dynasty, superclass, ..) = metadata(command);
        assert!(
            matches!(derivation, CimValue::Array(items) if !items.is_empty()),
            "{command}"
        );
        assert_ne!(count, CimValue::Sint32(0), "{command}");
        assert_ne!(dynasty, CimValue::String(wide("")), "{command}");
        assert_ne!(superclass, CimValue::String(wide("")), "{command}");
    }
}

#[test]
fn an_alias_keeps_its_own_namespace_beside_the_namespace_switch() {
    let (status, sink) = session("HOST", r"/namespace:\\root\nosuchns os get Caption");
    assert_eq!(status, 0);
    assert!(sink.stdout.starts_with(b"Caption"));
    assert!(sink.stderr.is_empty());
    let (status, sink) = session(
        "HOST",
        r"/namespace:\\root\nosuchns path Win32_OperatingSystem get Caption",
    );
    assert_eq!(status, 0x8004100e);
    assert!(sink.stdout.is_empty());
    assert_eq!(
        sink.stderr,
        b"ERROR:\r\r\nDescription = Invalid namespace\r\r\n"
    );
}

#[test]
fn every_registered_class_returns_typed_flagged_records() {
    let system = [
        "__PATH",
        "__NAMESPACE",
        "__SERVER",
        "__DERIVATION",
        "__PROPERTY_COUNT",
        "__RELPATH",
        "__DYNASTY",
        "__SUPERCLASS",
        "__CLASS",
        "__GENUS",
    ];
    assert!(registry().entries().len() >= 30);
    for entry in registry().entries() {
        let class = &entry.info.name;
        let records = objects(&format!("path {class} get"));
        if wmic_compat::backend::macos::providers::EMPTY_CLASSES.contains(&class.as_str()) {
            assert!(records.is_empty(), "{class} is explicitly unpopulated");
        }
        if [
            "Win32_OperatingSystem",
            "Win32_Processor",
            "Win32_ComputerSystem",
            "Win32_Process",
            "Win32_LogicalDisk",
            "Win32_Volume",
            "Win32_DiskDrive",
            "Win32_DiskPartition",
            "Win32_NetworkAdapter",
            "Win32_NetworkAdapterConfiguration",
        ]
        .contains(&class.as_str())
        {
            assert!(
                !records.is_empty(),
                "required provider {class} returned no instances"
            );
        }
        for record in records {
            assert_eq!(&record.class.to_string_lossy(), class);
            let path = record.path.as_ref().unwrap().to_string_lossy();
            assert!(
                path.starts_with("\\\\") && path.contains(&format!("\\root\\cimv2:{class}")),
                "{path}"
            );
            assert!(record.server.is_some());
            let flagged: Vec<_> = record.properties.iter().filter(|p| p.system).collect();
            assert_eq!(flagged.len(), 10, "{class}");
            for name in system {
                assert!(
                    flagged.iter().any(|p| p.name.eq_ignore_case(name)),
                    "{class} lacks {name}"
                );
            }
            let plain: Vec<_> = record.properties.iter().filter(|p| !p.system).collect();
            assert_eq!(plain.len(), entry.info.schema.properties.len(), "{class}");
            for (property, schema) in plain.iter().zip(&entry.info.schema.properties) {
                assert_eq!(property.name, schema.name);
                assert_eq!(property.cim_type, schema.cim_type);
            }
            for property in &record.properties {
                assert!(
                    consistent(&property.value, property.cim_type),
                    "{class}.{} holds {:?} for type {}",
                    property.name,
                    property.value,
                    property.cim_type
                );
            }
        }
    }
}

#[test]
fn class_lookup_ignores_case() {
    assert_eq!(objects("path WIN32_PROCESSOR get Name").len(), 1);
    assert_eq!(objects("path win32_tapedrive get DeviceID").len(), 0);
}

#[test]
fn schema_has_no_system_properties_and_boolean_qualifiers() {
    let ManagementResult::Schema(schema) =
        backend().execute(&request("class Win32_Process")).unwrap()
    else {
        panic!("class request did not return a schema")
    };
    assert_eq!(schema.name, wide("Win32_Process"));
    assert!(!schema.properties.is_empty());
    for property in &schema.properties {
        assert!(!property.name.to_string_lossy().starts_with("__"));
        let read = property
            .qualifiers
            .iter()
            .find(|q| q.name.eq_ignore_case("read"))
            .unwrap();
        assert_eq!(read.value, CimValue::Boolean(true));
        for q in &property.qualifiers {
            assert!(matches!(q.value, CimValue::Boolean(_)));
        }
    }
    let create = schema
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_case("Create"))
        .unwrap();
    assert!(create.is_static);
    let implemented = create
        .qualifiers
        .iter()
        .find(|q| q.name.eq_ignore_case("implemented"))
        .unwrap();
    assert!(matches!(implemented.value, CimValue::Boolean(_)));
    let startup = create
        .input
        .iter()
        .find(|p| p.name.eq_ignore_case("ProcessStartupInformation"))
        .unwrap();
    assert_eq!(startup.cim_type, 13);
    assert_eq!(
        startup.cim_type_name,
        Some(wide("object:Win32_ProcessStartup"))
    );
    assert_eq!(startup.id, Some(2));
    assert!(
        create
            .output
            .iter()
            .any(|p| p.name.eq_ignore_case("ReturnValue"))
    );
    assert!(
        create
            .output
            .iter()
            .any(|p| p.name.eq_ignore_case("ProcessId"))
    );
}

#[test]
fn projection_keeps_requested_properties_and_system_properties() {
    let records = objects("os get Caption,Version");
    assert_eq!(records.len(), 1);
    let names: Vec<_> = records[0]
        .properties
        .iter()
        .filter(|p| !p.system)
        .map(|p| p.name.to_string_lossy())
        .collect();
    assert_eq!(names, ["Caption", "Version"]);
    assert!(text(&records[0], "Caption").unwrap().starts_with("macOS "));
    assert_eq!(
        records[0].properties.iter().filter(|p| p.system).count(),
        10
    );
}

#[test]
fn wildcard_projection_keeps_every_column() {
    let all = objects("os get *");
    let schema = &registry().get("Win32_OperatingSystem").unwrap().info;
    assert_eq!(
        all[0].properties.iter().filter(|p| !p.system).count(),
        schema.schema.properties.len()
    );
}

#[test]
fn where_filters_numeric_and_string_properties() {
    assert_eq!(objects("os where \"ProductType = 1\" get Caption").len(), 1);
    assert_eq!(objects("os where \"ProductType = 2\" get Caption").len(), 0);
    assert_eq!(objects("os where \"OSType = 2\" get Caption").len(), 1);
    assert_eq!(objects("os where \"OSType > 2\" get Caption").len(), 0);
    assert_eq!(
        objects("os where \"NumberOfProcesses > 0 AND NOT Primary = FALSE\" get Caption").len(),
        1
    );
    assert_eq!(
        objects("os where \"Caption like 'macOS%'\" get Caption").len(),
        1
    );
    assert_eq!(
        objects("os where \"Manufacturer = 'apple inc.'\" get Caption").len(),
        1
    );
    assert_eq!(
        objects("os where \"Version = '__WMIC_RESEARCH_NO_MATCH__'\" get Caption").len(),
        0
    );
    assert_eq!(
        objects("os where \"Locale IS NULL OR Locale IS NOT NULL\" get Caption").len(),
        1
    );
}

#[test]
fn instance_paths_select_by_key() {
    let name = text(&objects("computersystem get Name")[0], "Name").unwrap();
    let hit = objects(&format!(
        "path Win32_ComputerSystem.Name=\"{name}\" get Name"
    ));
    assert_eq!(hit.len(), 1);
    let miss = objects("path Win32_ComputerSystem.Name=\"WmicNoSuchComputer\" get Name");
    assert!(miss.is_empty());
    assert_eq!(
        objects("path Win32_Processor.DeviceID=\"CPU0\" get Name").len(),
        1
    );
    assert_eq!(objects("path Win32_OperatingSystem=@ get Caption").len(), 1);
    let path = objects("os get Caption")[0].path.clone().unwrap();
    assert!(path.to_string_lossy().ends_with(":Win32_OperatingSystem=@"));
    let relative = objects("bios get Name")[0]
        .path
        .clone()
        .unwrap()
        .to_string_lossy();
    assert!(relative.contains(":Win32_BIOS.Name=\""), "{relative}");
}

#[test]
fn unknown_property_is_an_invalid_query() {
    let e = failure(&request("os get WmicResearchPropertyDoesNotExist"));
    assert_eq!((e.code, operation(&e)), (0x80041017, "query".into()));
    assert_eq!(e.message, wide("Invalid query"));
    let e = failure(&request("os where \"WmicNope = 1\" get Caption"));
    assert_eq!(e.code, 0x80041017);
    let e = failure(&request("os where \"Caption = \" get Caption"));
    assert_eq!(e.code, 0x80041017);
    let e = failure(&request("os where \"Caption > 5\" get Caption"));
    assert_eq!(e.code, 0x80041017);
}

#[test]
fn unknown_class_through_an_alias_is_invalid_class() {
    let e = failure(&request("printer get Name"));
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (0x80041010, "Invalid class".into(), "query".into())
    );
    let (status, sink) = session("HOST", "printer get Name");
    assert_eq!(status, 0x80041010);
    assert_eq!(
        sink.stderr,
        b"Node - HOST\r\r\nERROR:\r\r\nDescription = Invalid class\r\r\n"
    );
    assert_eq!(sink.stdout, b"\r\r\n\r\r\n");
}

#[test]
fn unknown_class_through_a_path_matches_the_reference_bytes() {
    let e = failure(&request("path WmicResearchClassDoesNotExist get Name"));
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (0x80041002, "Not found".into(), "class".into())
    );
    let (status, sink) = session("HOST", "path WmicResearchClassDoesNotExist get Name");
    assert_eq!(status, 0x80041002);
    assert_eq!(
        sink.stderr,
        include_bytes!("fixtures/reference/error-class.stderr.bin")
    );
    assert_eq!(
        sink.stdout,
        include_bytes!("fixtures/reference/error-class.stdout.bin")
    );
}

#[test]
fn session_output_matches_reference_bytes_for_property_error_and_empty_set() {
    let (status, sink) = session("DESKTOP-N4H9E7R", "os get WmicResearchPropertyDoesNotExist");
    assert_eq!(status, 0x80041017);
    assert_eq!(
        sink.stderr,
        include_bytes!("fixtures/reference/error-property.stderr.bin")
    );
    assert_eq!(
        sink.stdout,
        include_bytes!("fixtures/reference/error-property.stdout.bin")
    );
    let (status, sink) = session(
        "HOST",
        "os where \"Version='__WMIC_RESEARCH_NO_MATCH__'\" get Caption",
    );
    assert_eq!(status, 0);
    assert_eq!(
        sink.stderr,
        include_bytes!("fixtures/reference/os-empty.stderr.bin")
    );
    assert_eq!(
        sink.stdout,
        include_bytes!("fixtures/reference/os-empty.stdout.bin")
    );
}

#[test]
fn namespace_must_be_root_cimv2() {
    for accepted in ["root\\cimv2", "ROOT\\CIMV2", "\\\\root\\cimv2"] {
        let mut r = request("os get Caption");
        r.connection.namespace = accepted.into();
        assert!(backend().execute(&r).is_ok(), "{accepted}");
    }
    let mut r = request("os get Caption");
    r.connection.namespace = "root\\cli".into();
    let e = failure(&r);
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (0x8004100e, "Invalid namespace".into(), "connect".into())
    );
}

#[test]
fn only_the_local_machine_is_reachable() {
    let mut r = request("os get Caption");
    r.connection.node = "wmic-no-such-host.invalid".into();
    let e = failure(&r);
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (
            0x800706ba,
            "The RPC server is unavailable.".into(),
            "query".into()
        )
    );
    for local in [".", "localhost", "LOCALHOST", "127.0.0.1", "::1"] {
        let mut r = request("os get Caption");
        r.connection.node = local.into();
        assert!(backend().execute(&r).is_ok(), "{local}");
    }
    let mut host = [0 as libc::c_char; 256];
    assert_eq!(unsafe { libc::gethostname(host.as_mut_ptr(), 255) }, 0);
    let full: String = host
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8 as char)
        .collect();
    let label = full.split('.').next().unwrap().to_uppercase();
    for local in [full.as_str(), label.as_str()] {
        let mut r = request("os get Caption");
        r.connection.node = local.into();
        assert!(backend().execute(&r).is_ok(), "{local}");
        assert!(backend().reachable(&wide(local), 1000));
    }
    assert!(!backend().reachable(&wide("wmic-no-such-host.invalid"), 1000));
}

#[test]
fn association_queries_are_not_supported() {
    let e = failure(&request("path Win32_OperatingSystem assoc"));
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (0x8004100c, "Not supported".into(), "query".into())
    );
}

#[test]
fn writes_are_not_supported() {
    let e = failure(&request("os set Description=\"x\""));
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (0x8004100c, "Not supported".into(), "update".into())
    );
    assert!(
        e.target
            .to_string_lossy()
            .ends_with(":Win32_OperatingSystem=@")
    );
    let e = failure(&request("os delete"));
    assert_eq!((e.code, operation(&e)), (0x8004100c, "delete".into()));
    let e = failure(&request("path Win32_Environment create Name=\"A\""));
    assert_eq!((e.code, operation(&e)), (0x8004100c, "write".into()));
    let e = failure(&request("os set Missing=1"));
    assert_eq!((e.code, operation(&e)), (0xADE5, "input".into()));
}

#[test]
/// Keep unknown-method, binding and unsupported-provider errors distinct.
fn methods_bind_by_schema_and_report_shapes() {
    for command in [
        "path Win32_OperatingSystem call WmicNoSuchMethod",
        "class Win32_OperatingSystem call WmicNoSuchMethod",
    ] {
        let e = failure(&request(command));
        assert_eq!(
            (e.code, operation(&e)),
            (0xABFC, "input".into()),
            "{command}"
        );
        assert_eq!(e.message, wide("WmicNoSuchMethod - Invalid class method."));
    }
    let e = failure(&request("os call WmicNoSuchMethod"));
    assert_eq!((e.code, operation(&e)), (0xABFD, "input".into()));
    assert_eq!(e.message, wide("WmicNoSuchMethod - Invalid alias verb."));
    if unsafe { libc::geteuid() } != 0 {
        let e = failure(&request("path Win32_OperatingSystem call Reboot 1"));
        assert_eq!((e.code, operation(&e)), (0xADE6, "input".into()));
    }
    let e = failure(&request(
        "path Win32_OperatingSystem call SetDateTime 20260101000000.000000+000",
    ));
    assert_eq!(
        (e.code, e.message.to_string_lossy(), operation(&e)),
        (0x8004100c, "Not supported".into(), "method".into())
    );
    assert!(
        e.target
            .to_string_lossy()
            .ends_with(":Win32_OperatingSystem=@")
    );
}

#[test]
fn interactive_prompts_use_the_windows_text_and_declines_skip_the_target() {
    let mut answer = Answer {
        reply: false,
        prompts: vec![],
    };
    let r = request("/interactive os set Description=\"x\"");
    let result = backend()
        .execute_with_confirmation(&r, &mut answer)
        .unwrap();
    let ManagementResult::Operation(done) = result else {
        panic!("expected an operation result")
    };
    assert!(done.paths.is_empty());
    assert_eq!(done.instances.len(), 1);
    assert_eq!(answer.prompts.len(), 1);
    assert!(answer.prompts[0].starts_with("Update property(s) of '\\\\"));
    assert!(answer.prompts[0].ends_with(":Win32_OperatingSystem=@' (Y/N/?)? "));

    let mut answer = Answer {
        reply: false,
        prompts: vec![],
    };
    if unsafe { libc::geteuid() } != 0 {
        let r = request("/interactive path Win32_OperatingSystem call Reboot");
        let result = backend()
            .execute_with_confirmation(&r, &mut answer)
            .unwrap();
        assert!(matches!(
            result,
            ManagementResult::Operation(OperationResult {
                action: Verb::Call,
                ..
            })
        ));
        assert!(answer.prompts[0].starts_with("Execute (\\\\"));
        assert!(answer.prompts[0].ends_with(":Win32_OperatingSystem=@)->Reboot() (Y/N/?)? "));
    }

    let mut answer = Answer {
        reply: false,
        prompts: vec![],
    };
    let r = request("/interactive path Win32_Environment create Name=\"A\"");
    let result = backend()
        .execute_with_confirmation(&r, &mut answer)
        .unwrap();
    assert!(matches!(
        result,
        ManagementResult::Operation(OperationResult {
            action: Verb::Create,
            ..
        })
    ));
    assert_eq!(
        answer.prompts,
        ["Create instance of 'Win32_Environment' class (Y/N)?"]
    );

    let mut answer = Answer {
        reply: true,
        prompts: vec![],
    };
    let e = backend()
        .execute_with_confirmation(
            &request("/interactive os set Description=\"x\""),
            &mut answer,
        )
        .unwrap_err();
    assert_eq!((e.code, operation(&e)), (0x8004100c, "update".into()));

    let e = failure(&request("/interactive os set Description=\"x\""));
    assert_eq!((e.code, operation(&e)), (0x80041008, "input".into()));
}

#[test]
fn loaders_defer_to_the_bundled_aliases() {
    let mut b = backend();
    assert!(
        b.load_aliases(&wide("root\\cli"), None, &wide("ms_409"))
            .unwrap()
            .is_none()
    );
    assert!(
        b.load_alias_inventory(&wide("root\\cli"), &wide("ms_409"))
            .unwrap()
            .is_none()
    );
    assert!(
        b.load_translation(&wide("root\\cli"), &wide("x"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn zero_instance_and_pending_classes_serve_schema_only() {
    for class in [
        "Win32_TemperatureProbe",
        "Win32_VoltageProbe",
        "Win32_CDROMDrive",
        "Win32_TapeDrive",
    ] {
        assert!(objects(&format!("path {class} get")).is_empty(), "{class}");
        assert!(matches!(
            backend()
                .execute(&request(&format!("class {class}")))
                .unwrap(),
            ManagementResult::Schema(_)
        ));
    }
    for class in [
        "Win32_Process",
        "Win32_Service",
        "Win32_LogicalDisk",
        "Win32_Volume",
        "Win32_DiskDrive",
        "Win32_DiskPartition",
        "Win32_NetworkAdapter",
        "Win32_NetworkAdapterConfiguration",
        "Win32_UserAccount",
        "Win32_Group",
        "Win32_StartupCommand",
        "Win32_Product",
        "Win32_QuickFixEngineering",
    ] {
        assert!(registry().get(class).is_some(), "{class}");
    }
    assert!(registry().get("Win32_Printer").is_none());
}

#[test]
fn time_values_use_cim_datetime_text() {
    let os = &objects("os get LastBootUpTime,LocalDateTime,InstallDate")[0];
    for name in ["LastBootUpTime", "LocalDateTime"] {
        let value = text(os, name).unwrap();
        assert_eq!(value.len(), 25, "{value}");
        assert_eq!(&value[14..15], ".");
        assert!(matches!(&value[21..22], "+" | "-"));
        assert!(value[..14].bytes().all(|b| b.is_ascii_digit()));
        assert!(value[22..].bytes().all(|b| b.is_ascii_digit()));
    }
}

#[test]
fn identity_classes_agree_with_each_other() {
    let product = &objects("csproduct get IdentifyingNumber,Name,UUID")[0];
    let board = &objects("baseboard get Product")[0];
    assert_eq!(text(product, "Name"), text(board, "Product"));
    let bios = &objects("bios get Name,SMBIOSBIOSVersion,Version")[0];
    assert_eq!(text(bios, "Name"), text(bios, "SMBIOSBIOSVersion"));
    let memory = &objects("memorychip get Capacity")[0];
    let system = &objects("computersystem get TotalPhysicalMemory")[0];
    let capacity = |r: &Record, name: &str| {
        r.properties
            .iter()
            .find(|p| p.name.eq_ignore_case(name))
            .map(|p| p.value.clone())
    };
    assert_eq!(
        capacity(memory, "Capacity"),
        capacity(system, "TotalPhysicalMemory")
    );
}

#[test]
fn failed_writes_print_the_operation_line_before_the_diagnostic() {
    let (status, sink) = session("HOST", "os set Description=\"x\"");
    assert_eq!(status, 0x8004100c);
    let stdout = String::from_utf8(sink.stdout).unwrap();
    assert!(
        stdout.starts_with("Updating property(s) of '\\\\"),
        "{stdout:?}"
    );
    assert!(
        stdout.trim_end().ends_with(":Win32_OperatingSystem=@'"),
        "{stdout:?}"
    );
    assert!(!sink.stderr.is_empty());
    let (status, sink) = session("HOST", "os delete");
    assert_eq!(status, 0x8004100c);
    let stdout = String::from_utf8(sink.stdout).unwrap();
    assert!(stdout.starts_with("Deleting instance \\\\"), "{stdout:?}");
    assert!(!sink.stderr.is_empty());
}

#[test]
fn number_of_processes_agrees_with_the_process_table() {
    use wmic_compat::backend::macos::sys::proc;
    let reported = match objects("os get NumberOfProcesses")[0]
        .properties
        .iter()
        .find(|p| p.name.eq_ignore_case("NumberOfProcesses"))
        .map(|p| p.value.clone())
    {
        Some(CimValue::Uint32(n)) => n as f64,
        other => panic!("unexpected value {other:?}"),
    };
    let table = proc::list().unwrap().len() as f64;
    assert!(
        (reported - table).abs() <= table * 0.10,
        "reported {reported}, process table {table}"
    );
}

#[test]
fn where_accepts_system_properties_and_quoted_numbers() {
    assert_eq!(
        objects("os where \"__CLASS='Win32_OperatingSystem'\" get Caption").len(),
        1
    );
    assert_eq!(
        objects("os where \"__CLASS='Win32_Other'\" get Caption").len(),
        0
    );
    assert_eq!(objects("os where \"__GENUS = 2\" get Caption").len(), 1);
    assert_eq!(objects("os where \"OSType='2'\" get Caption").len(), 1);
    assert_eq!(objects("os where \"OSType='3'\" get Caption").len(), 0);
    assert_eq!(objects("os where \"Primary='TRUE'\" get Caption").len(), 1);
    assert_eq!(
        objects("os where \"NumberOfProcesses > 0.5\" get Caption").len(),
        1
    );
    let e = failure(&request("os where \"OSType='two'\" get Caption"));
    assert_eq!(e.code, 0x80041017);
}

#[test]
fn object_paths_need_every_key_and_the_singleton_form_needs_a_singleton() {
    let invalid = |text: &str| {
        let e = failure(&request(text));
        assert_eq!(
            (e.code, e.message.to_string_lossy(), operation(&e)),
            (0x8004103a, "Invalid object path".into(), "query".into()),
            "{text}"
        );
    };
    invalid("path Win32_Environment=@ get Name");
    invalid("path Win32_Environment.Name=\"PATH\" get Name");
    invalid("path Win32_Environment.UserName=\"x\" get Name");
    invalid("path Win32_Environment.Name=\"PATH\",Name=\"PATH\" get Name");
    invalid("path Win32_Environment.Name=\"PATH\",Caption=\"x\" get Name");
    assert_eq!(objects("path Win32_OperatingSystem=@ get Caption").len(), 1);
    objects("path Win32_Environment.Name=\"WMIC_NO_SUCH\",UserName=\"x\" get Name");
}

/// Serialized requests must retain alias versus direct-target diagnostics even with an instance path.
#[test]
fn unknown_method_diagnostics_preserve_call_origin_after_serialization() {
    for (text, code, suffix) in [
        (
            "process call WmicCompatMissingMethod",
            0xABFD,
            "Invalid alias verb.",
        ),
        (
            "process Win32_Process.ProcessId=0 call WmicCompatMissingMethod",
            0xABFD,
            "Invalid alias verb.",
        ),
        (
            "path Win32_Process call WmicCompatMissingMethod",
            0xABFC,
            "Invalid class method.",
        ),
        (
            "path Win32_Process.ProcessId=0 call WmicCompatMissingMethod",
            0xABFC,
            "Invalid class method.",
        ),
        (
            "class Win32_Process call WmicCompatMissingMethod",
            0xABFC,
            "Invalid class method.",
        ),
    ] {
        let original = request(text);
        let copied = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        let error = failure(&copied);
        assert_eq!(
            (error.code, operation(&error)),
            (code, "input".into()),
            "{text}"
        );
        assert_eq!(
            error.message,
            wide(&format!("WmicCompatMissingMethod - {suffix}")),
            "{text}"
        );
    }
}
