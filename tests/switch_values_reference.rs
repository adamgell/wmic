//! Exact CONTEXT/status oracles captured from Microsoft's hash-pinned WMIC.
mod support;
use std::{collections::BTreeMap, path::PathBuf};
use support::wide;
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream, Session},
};

struct NoProvider;
impl Management for NoProvider {
    fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        panic!("CONTEXT probes must never call a management provider")
    }
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

fn fixtures() -> PathBuf {
    std::env::var_os("WMIC_TEST_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/switch-values-reference")
        })
}

fn cases() -> (String, Vec<(String, String, serde_json::Value)>) {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/switch-values-cases.json")).unwrap();
    let capture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/switch-values-reference/capture.json"
    ))
    .unwrap();
    let reference: BTreeMap<_, _> = capture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c))
        .collect();
    let requests = manifest["cases"].as_array().unwrap();
    assert_eq!(reference.len(), requests.len());
    let cases = requests
        .iter()
        .map(|c| {
            let id = c["id"].as_str().unwrap();
            let expected = reference[id];
            assert_eq!(expected["state"], "completed", "{id}");
            (
                id.to_owned(),
                c["arguments"].as_str().unwrap().to_owned(),
                expected.clone(),
            )
        })
        .collect();
    (
        capture["profile"]["computer"].as_str().unwrap().to_owned(),
        cases,
    )
}

fn stream(case: &serde_json::Value, name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(case[name]["file"].as_str().unwrap())).unwrap()
}

#[test]
fn controller_matches_every_captured_switch_value() {
    let (node, cases) = cases();
    for (id, arguments, expected) in cases {
        let mut session = Session::new(wide(&node)).unwrap();
        let mut sink = Sink::default();
        let actual = session.execute(
            &wide(&arguments),
            EntryMode::Interactive,
            &mut NoProvider,
            &mut sink,
        );
        assert_eq!(
            actual.exit_status,
            expected["exit_status"].as_u64().unwrap() as u32,
            "{id}: {arguments:?}"
        );
        assert_eq!(sink.stdout, stream(&expected, "stdout"), "{id}: stdout");
        assert_eq!(sink.stderr, stream(&expected, "stderr"), "{id}: stderr");
    }
}

#[test]
#[cfg(windows)]
fn windows_cli_matches_every_captured_switch_value() {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let (node, cases) = cases();
    let binary =
        std::env::var_os("WMIC_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_wmic").into());
    for (id, arguments, expected) in cases {
        let mut child = Command::new(&binary)
            .raw_arg(&arguments)
            .env("COMPUTERNAME", &node)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{id}: CLI timed out: {arguments:?}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let actual = child.wait_with_output().unwrap();
        assert_eq!(
            actual.status.code().unwrap() as u32,
            expected["exit_status"].as_u64().unwrap() as u32,
            "{id}: status"
        );
        assert_eq!(actual.stdout, stream(&expected, "stdout"), "{id}: stdout");
        assert_eq!(actual.stderr, stream(&expected, "stderr"), "{id}: stderr");
    }
}
