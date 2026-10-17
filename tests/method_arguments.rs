//! Captured method-argument diagnostics. Control probes must never execute a provider.
use std::{collections::BTreeMap, path::PathBuf};
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream, Session},
};

struct NoProvider;
impl Management for NoProvider {
    /// Fail immediately if a controller-only case reaches management execution.
    fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        panic!("method argument control oracle must never reach a provider")
    }
}

#[derive(Default)]
struct Sink {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
impl OutputSink for Sink {
    /// Encode both streams for exact comparison with the ASCII control captures.
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        let bytes = encode_output(text, &DestinationProfile::pipe(1252))?;
        match stream {
            OutputStream::Stdout => self.stdout.extend(bytes),
            OutputStream::Stderr => self.stderr.extend(bytes),
        }
        Ok(())
    }
}

/// Locate raw captures, allowing the native harness to supply a staged fixture directory.
fn fixtures() -> PathBuf {
    std::env::var_os("WMIC_METHOD_ARGUMENT_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/method-arguments-reference")
        })
}
/// Pair captured results with invocations and guard the selected test inventory.
fn cases(controls_only: bool) -> (String, Vec<(String, String, serde_json::Value)>) {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/method-arguments-cases.json")).unwrap();
    let capture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/method-arguments-reference/capture.json"
    ))
    .unwrap();
    let reference: BTreeMap<_, _> = capture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c))
        .collect();
    let requests = manifest["cases"].as_array().unwrap();
    assert_eq!(requests.len(), 106);
    assert_eq!(reference.len(), requests.len());
    let cases: Vec<_> = requests
        .iter()
        .filter(|c| !controls_only || c["controller_oracle"] == true)
        .map(|c| {
            let id = c["id"].as_str().unwrap();
            let expected = reference[id];
            assert_eq!(expected["state"], "completed", "{id}");
            assert!(
                c.get("stdin_base64").is_none(),
                "{id}: control probes use no interactive input"
            );
            (
                id.to_owned(),
                c["arguments"].as_str().unwrap().to_owned(),
                expected.clone(),
            )
        })
        .collect();
    assert_eq!(cases.len(), if controls_only { 64 } else { 106 });
    (
        capture["profile"]["computer"].as_str().unwrap().to_owned(),
        cases,
    )
}
/// Read the captured bytes directly so encoding and line endings remain significant.
fn stream(case: &serde_json::Value, name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(case[name]["file"].as_str().unwrap())).unwrap()
}

#[test]
/// Prove the 64 controller failures occur before any management operation.
fn control_oracles_match_status_and_both_streams_without_a_provider() {
    let (node, cases) = cases(true);
    for (id, arguments, expected) in cases {
        let mut sink = Sink::default();
        let actual = Session::new(node.as_str().into()).unwrap().execute(
            &format!("wmic {arguments}").into(),
            EntryMode::Process,
            &mut NoProvider,
            &mut sink,
        );
        assert_eq!(
            actual.exit_status,
            expected["exit_status"].as_u64().unwrap() as u32,
            "{id}: status"
        );
        assert_eq!(sink.stdout, stream(&expected, "stdout"), "{id}: stdout");
        assert_eq!(sink.stderr, stream(&expected, "stderr"), "{id}: stderr");
    }
}

#[test]
#[cfg(windows)]
/// Compare all 106 commands through a real child process with bounded execution.
fn windows_cli_matches_all_method_argument_oracles() {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let (node, cases) = cases(false);
    let binary =
        std::env::var_os("WMIC_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_wmic").into());
    for (id, arguments, expected) in cases {
        let mut child = Command::new(&binary)
            .raw_arg(&arguments)
            // Match the native capture launcher. An inherited console can have
            // a shell-selected code page that changes the diagnostic bytes.
            .creation_flags(0x08000000) // CREATE_NO_WINDOW: native pipes use the OEM code page.
            .env("COMPUTERNAME", &node)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Drain both pipes while the child runs: help output exceeds pipe capacity.
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut data = vec![];
            stdout.read_to_end(&mut data).unwrap();
            data
        });
        let err = std::thread::spawn(move || {
            let mut data = vec![];
            stderr.read_to_end(&mut data).unwrap();
            data
        });
        let deadline = Instant::now() + Duration::from_secs(10);
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
            "{id}: status"
        );
        assert_eq!(stdout, stream(&expected, "stdout"), "{id}: stdout");
        assert_eq!(stderr, stream(&expected, "stderr"), "{id}: stderr");
    }
}
