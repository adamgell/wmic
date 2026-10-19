mod support;
use support::{request, wide};
use wmic_compat::backend::build_query;

#[test]
fn single_key_path_get_matches_microsoft_query_text() {
    for (command, expected) in [
        (
            r#"path Win32_Process.Handle="4294967295" get Caption"#,
            r#"SELECT Caption FROM Win32_Process WHERE Handle="4294967295""#,
        ),
        (
            "path Win32_Process.Handle='4294967295' get Caption",
            "SELECT Caption FROM Win32_Process WHERE Handle='4294967295'",
        ),
        (
            "path Win32_Process.Handle=4294967295 get Caption",
            "SELECT Caption FROM Win32_Process WHERE Handle=4294967295",
        ),
        (
            "path Win32_Process.Handle='4294967295' get",
            "SELECT * FROM Win32_Process WHERE Handle='4294967295'",
        ),
        (
            "path Win32_Process.Handle='4294967295'",
            "SELECT * FROM Win32_Process WHERE Handle='4294967295'",
        ),
        (
            "path Win32_Environment.Name='WmicCompat.Missing' get Name",
            "SELECT Name FROM Win32_Environment WHERE Name='WmicCompat.Missing'",
        ),
        (
            "path Win32_Environment.Name='Wmic WHERE Missing' get Name",
            "SELECT Name FROM Win32_Environment WHERE Name='Wmic WHERE Missing'",
        ),
    ] {
        assert_eq!(build_query(&request(command)), wide(expected), "{command}");
    }
}

#[test]
fn path_key_conversion_preserves_raw_utf16_literals() {
    let mut request = request("path Win32_Process.Handle='placeholder' get Caption");
    let mut path = wide("Win32_Process.Handle='");
    path.0.extend([0xd800, 0xd83e, 0xdd80]);
    path.0.extend("'".encode_utf16());
    request.instance_path = Some(path);
    let mut expected = wide("SELECT Caption FROM Win32_Process WHERE Handle='");
    expected.0.extend([0xd800, 0xd83e, 0xdd80]);
    expected.0.extend("'".encode_utf16());
    assert_eq!(build_query(&request), expected);
}

#[test]
fn native_rejected_and_unverified_forms_keep_their_existing_query_path() {
    for (command, expected) in [
        (
            "path Win32_OperatingSystem=@ get Caption",
            "SELECT Caption FROM Win32_OperatingSystem=@",
        ),
        (
            "path Win32_Process.Handle='4294967295' where \"Name='missing'\" get Caption",
            "SELECT Caption FROM Win32_Process.Handle='4294967295' WHERE Name='missing'",
        ),
        (
            "path Win32_Environment.Name='WmicMissing',UserName='<SYSTEM>' get Name",
            "SELECT Name FROM Win32_Environment.Name='WmicMissing',UserName='<SYSTEM>'",
        ),
        (
            r#"alias MSFT_CliAlias.FriendlyName="OS" get FriendlyName"#,
            r#"SELECT FriendlyName FROM MSFT_CliAlias.FriendlyName="OS""#,
        ),
        (
            "path Win32_Process.Handle='4294967295' call GetOwner",
            "SELECT * FROM Win32_Process.Handle='4294967295'",
        ),
    ] {
        assert_eq!(build_query(&request(command)), wide(expected), "{command}");
    }
}

#[test]
fn rejected_comma_and_escaped_quote_literals_are_not_promoted_to_valid_queries() {
    for path in [
        "Win32_Environment.Name='WmicCompat,Missing'",
        r#"Win32_Environment.Name="WmicCompat\\Missing\"Value""#,
    ] {
        let mut resolved = request("path Win32_Environment.Name='placeholder' get Name");
        resolved.instance_path = Some(wide(path));
        assert_eq!(
            build_query(&resolved),
            wide(&format!("SELECT Name FROM {path}")),
            "native-rejected literal must retain its prior query path"
        );
    }
}

fn fixtures() -> std::path::PathBuf {
    std::env::var_os("WMIC_PATH_INSTANCE_FIXTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/path-instance-reference")
        })
}
fn captured(id: &str, stream: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(format!("{id}.{stream}.bin"))).unwrap()
}
fn native_query(id: &str) -> wmic_compat::model::WideText {
    let stderr = String::from_utf8(captured(id, "stderr")).unwrap();
    let (_, query) = stderr
        .split_once("IWbemServices::ExecQuery(L\"WQL\", L\"")
        .unwrap();
    wide(query.split_once("\", 0, NULL, -)").unwrap().0)
}

#[test]
fn query_builder_matches_original_trace_queries() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-instance-cases.json")).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        if case["cli_case"] == true {
            continue;
        }
        let id = case["id"].as_str().unwrap();
        assert_eq!(
            build_query(&request(case["arguments"].as_str().unwrap())),
            native_query(id),
            "{id}"
        );
    }
}

#[test]
fn session_dispatches_the_native_query_and_matches_plain_output() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        model::*,
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Backend {
        expected: WideText,
        status: u32,
        calls: usize,
    }
    impl Management for Backend {
        fn execute(
            &mut self,
            request: &ResolvedRequest,
        ) -> Result<ManagementResult, ManagementError> {
            self.calls += 1;
            assert_eq!(build_query(request), self.expected);
            if self.status != 0 {
                return Err(ManagementError {
                    code: self.status,
                    target: request.connection.node.clone(),
                    message: wide("Invalid query"),
                    operation: wide("enumerate"),
                    extended: vec![],
                    partial: None,
                });
            }
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    #[derive(Default)]
    struct Sink {
        stdout: WideText,
        stderr: WideText,
    }
    impl OutputSink for Sink {
        fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
            let destination = if stream == OutputStream::Stdout {
                &mut self.stdout
            } else {
                &mut self.stderr
            };
            destination.0.extend(&text.0);
            Ok(())
        }
    }
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-instance-cases.json")).unwrap();
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/path-instance-reference/capture.json"
    ))
    .unwrap();
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["cli_case"] == true)
    {
        let id = case["id"].as_str().unwrap();
        let expected = oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let status = expected["exit_status"].as_u64().unwrap() as u32;
        let mut backend = Backend {
            expected: native_query(id.strip_suffix("-plain").unwrap()),
            status,
            calls: 0,
        };
        let mut sink = Sink::default();
        let result = Session::new(wide("DESKTOP-N4H9E7R")).unwrap().execute(
            &wide(case["arguments"].as_str().unwrap()),
            EntryMode::Interactive,
            &mut backend,
            &mut sink,
        );
        assert_eq!(result.exit_status, status, "{id}");
        assert_eq!(backend.calls, 1, "{id}: must execute the query");
        assert_eq!(
            sink.stdout.to_string_lossy().as_bytes(),
            captured(id, "stdout"),
            "{id}: stdout"
        );
        assert_eq!(
            sink.stderr.to_string_lossy().as_bytes(),
            captured(id, "stderr"),
            "{id}: stderr"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_cli_matches_keyed_path_get_results() {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-instance-cases.json")).unwrap();
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/path-instance-reference/capture.json"
    ))
    .unwrap();
    let binary =
        std::env::var_os("WMIC_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_wmic").into());
    let mut tested = 0;
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["cli_case"] == true)
    {
        let id = case["id"].as_str().unwrap();
        let expected = oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let mut child = Command::new(&binary)
            .raw_arg(case["arguments"].as_str().unwrap())
            .creation_flags(0x08000000)
            .env("COMPUTERNAME", "DESKTOP-N4H9E7R")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut b = vec![];
            stdout.read_to_end(&mut b).unwrap();
            b
        });
        let err = std::thread::spawn(move || {
            let mut b = vec![];
            stderr.read_to_end(&mut b).unwrap();
            b
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut timeout = false;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                timeout = true;
                let _ = child.kill();
                break child.wait().unwrap();
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let stdout = out.join().unwrap();
        let stderr = err.join().unwrap();
        assert!(!timeout, "{id}: CLI timed out");
        assert_eq!(
            status.code().unwrap() as u32,
            expected["exit_status"].as_u64().unwrap() as u32,
            "{id}: DWORD status"
        );
        assert_eq!(stdout, captured(id, "stdout"), "{id}: stdout");
        assert_eq!(stderr, captured(id, "stderr"), "{id}: stderr");
        tested += 1;
    }
    assert_eq!(tested, 8);
}
