//! Exact CLI comparisons for unknown direct methods and unchanged alias diagnostics.
#![cfg(windows)]
use std::{collections::BTreeMap, path::PathBuf};

/// Keep native pipe bytes intact, including OEM best-fit conversion and CRCRLF.
fn stream(case: &serde_json::Value, name: &str) -> Vec<u8> {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/path-method-reference");
    std::fs::read(root.join(case[name]["file"].as_str().unwrap())).unwrap()
}

/// Launch every captured command and compare its full DWORD status and both streams.
#[test]
fn windows_cli_matches_unknown_path_method_diagnostics() {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-method-cases.json")).unwrap();
    let capture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/path-method-reference/capture.json")).unwrap();
    let reference: BTreeMap<_, _> = capture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c))
        .collect();
    let requests = manifest["cases"].as_array().unwrap();
    assert_eq!(requests.len(), 62);
    assert_eq!(reference.len(), requests.len());
    let cases: Vec<_> = requests
        .iter()
        .map(|c| {
            let id = c["id"].as_str().unwrap();
            let expected = reference[id];
            assert_eq!(expected["state"], "completed", "{id}");
            (id, c["arguments"].as_str().unwrap(), expected)
        })
        .collect();
    let node = capture["profile"]["computer"].as_str().unwrap();
    let binary =
        std::env::var_os("WMIC_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_wmic").into());
    for (id, arguments, expected) in cases {
        let mut child = Command::new(&binary)
            .raw_arg(arguments)
            // Match the native capture launcher. An inherited console can have
            // a shell-selected code page that changes the diagnostic bytes.
            .creation_flags(0x08000000) // CREATE_NO_WINDOW: native pipes use the OEM code page.
            .env("COMPUTERNAME", node)
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
        assert_eq!(stdout, stream(expected, "stdout"), "{id}: stdout");
        assert_eq!(stderr, stream(expected, "stderr"), "{id}: stderr");
    }
}
