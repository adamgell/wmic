//! Exercise the built executable through the actual macOS entry point.
#![cfg(target_os = "macos")]
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_wmic"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    process.0.stdin.take().unwrap().write_all(input).unwrap();
    let mut stdout = process.0.stdout.take().unwrap();
    let mut stderr = process.0.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = vec![];
        std::io::Read::read_to_end(&mut stdout, &mut bytes).unwrap();
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = vec![];
        std::io::Read::read_to_end(&mut stderr, &mut bytes).unwrap();
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "wmic timed out: {args:?}");
        std::thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

fn success(output: &Output) -> &str {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    std::str::from_utf8(&output.stdout).unwrap()
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wmic-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn real_os_and_cpu_queries_emit_utf8_lf_output() {
    let os = run(&["os", "get", "Caption,Version"], b"");
    let text = success(&os);
    assert!(text.contains("Caption") && text.contains("Version") && text.contains("macOS "));
    assert!(!text.contains('\r'));
    let cpu = run(&["cpu", "get", "Name", "/value"], b"");
    assert!(
        success(&cpu)
            .lines()
            .any(|line| line.starts_with("Name=") && line.len() > 5)
    );
}

#[test]
fn errors_reach_stderr_with_actual_posix_exit_statuses() {
    for (args, status, message) in [
        (
            vec!["path", "WmicCompatMissing", "get", "Name"],
            2,
            "Not found",
        ),
        (vec!["os", "get", "WmicCompatMissing"], 23, "Invalid query"),
        (
            vec![
                r#"/node:"wmic-no-such-host.invalid""#,
                "os",
                "get",
                "Caption",
            ],
            186,
            "RPC server",
        ),
        (
            vec!["os", "set", "Description=wmic-test-must-be-refused"],
            12,
            "Not supported",
        ),
    ] {
        let result = run(&args, b"");
        assert_eq!(result.status.code(), Some(status), "{result:?}");
        assert!(
            std::str::from_utf8(&result.stderr)
                .unwrap()
                .contains(message),
            "{result:?}"
        );
        assert!(!result.stderr.contains(&b'\r'));
        assert!(
            !result
                .stdout
                .windows(message.len())
                .any(|bytes| bytes == message.as_bytes())
        );
    }
}

#[test]
fn shell_quoted_where_clause_produces_an_empty_success() {
    let result = run(
        &[
            "os",
            "where",
            "Version='wmic_compat_no_match'",
            "get",
            "Caption",
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    assert!(!String::from_utf8(result.stdout).unwrap().contains("macOS"));
}

#[test]
fn spaced_output_path_and_append_keep_utf16_crlf_and_one_bom() {
    let directory = Directory::new();
    let file = directory.0.join("output with spaces.txt");
    for switch in ["output", "append"] {
        let argument = format!("/{switch}:{}", file.display());
        let result = run(&[&argument, "os", "get", "Caption", "/value"], b"");
        if switch == "output" {
            assert!(success(&result).trim().is_empty());
        } else {
            assert!(success(&result).contains("Caption=macOS "));
        }
    }
    let bytes = fs::read(file).unwrap();
    assert_eq!(&bytes[..2], b"\xff\xfe");
    assert_eq!(bytes.len() % 2, 0);
    let units: Vec<u16> = bytes[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let text = String::from_utf16(&units).unwrap();
    assert_eq!(text.matches("Caption=macOS ").count(), 2);
    assert!(!text.contains('\u{feff}'));
    assert!(text.contains("\r\n"));
    assert!(!text.replace("\r\n", "").contains('\n'));
}

#[test]
fn redirected_interactive_input_recovers_after_an_error_and_handles_eof() {
    let result = run(&[], b"os get WmicCompatMissing\nos get Caption /value\n");
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    assert!(
        std::str::from_utf8(&result.stderr)
            .unwrap()
            .contains("Invalid query")
    );
    let text = std::str::from_utf8(&result.stdout).unwrap();
    assert!(
        text.contains("wmic:") && text.contains("Caption=macOS "),
        "{text}"
    );
    assert!(!text.contains('\r'));
}

#[test]
fn unknown_alias_never_echoes_a_password_from_real_argv() {
    let result = run(
        &[
            "/password:DO_NOT_LOG_SECRET with spaces",
            "wmic-no-such-alias",
            "get",
            "Name",
        ],
        b"",
    );
    assert!(!result.status.success());
    for bytes in [&result.stdout, &result.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains("DO_NOT_LOG_SECRET"));
    }
}

#[test]
fn full_local_object_paths_reach_class_lookup_and_instance_selection() {
    for path in [
        r"\\.\root\cimv2:Win32_OperatingSystem=@".to_owned(),
        r"\\127.0.0.1\root\cimv2:Win32_OperatingSystem=@".to_owned(),
        r"\\::1\root\cimv2:Win32_OperatingSystem=@".to_owned(),
        format!(
            r#"\\{}\root\cimv2:Win32_Process.Handle="{}""#,
            wmic_compat::backend::macos::sys::host::host_name().unwrap(),
            std::process::id()
        ),
    ] {
        let result = run(&["path", &path, "get", "Name", "/value"], b"");
        assert!(!success(&result).trim().is_empty(), "{path}");
    }
}

#[test]
fn full_object_paths_do_not_bypass_remote_or_namespace_restrictions() {
    for (path, code, message) in [
        (
            r"\\wmic-no-such-host.invalid\root\cimv2:Win32_OperatingSystem=@",
            186,
            "RPC server",
        ),
        (
            r"\\.\root\cli:Win32_OperatingSystem=@",
            14,
            "Invalid namespace",
        ),
    ] {
        let result = run(&["path", path, "get", "Name"], b"");
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(message),
            "{result:?}"
        );
    }
}

#[test]
fn namespace_qualified_paths_preserve_colons_inside_key_values() {
    let path = r#"root\cimv2:Win32_LogicalDisk.DeviceID="/Volumes/wmic:missing""#;
    let result = run(&["path", path, "get", "Name"], b"");
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    assert!(!String::from_utf8_lossy(&result.stderr).contains("Invalid object path"));
}

#[test]
fn full_local_process_path_supports_read_only_method_selection() {
    let path = format!(
        r#"\\.\root\cimv2:Win32_Process.Handle="{}""#,
        std::process::id()
    );
    let result = run(&["path", &path, "call", "GetOwner"], b"");
    assert!(result.status.success(), "{result:?}");
    assert!(
        String::from_utf8_lossy(&result.stderr).trim().is_empty(),
        "{result:?}"
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("ReturnValue = 0"),
        "{result:?}"
    );
}

#[test]
fn full_class_targets_preserve_their_prefix_before_backend_lookup() {
    let result = run(
        &[
            "path",
            r"\\.\root\cimv2:Win32_Process",
            "get",
            "Name",
            "/value",
        ],
        b"",
    );
    assert!(
        success(&result)
            .lines()
            .any(|line| line.starts_with("Name="))
    );
    let relative = run(&["class", "Win32_Process"], b"");
    let full = run(&["class", r"\\.\root\cimv2:Win32_Process"], b"");
    assert_eq!(success(&full), success(&relative));
}

#[test]
fn malformed_qualified_instance_suffixes_cannot_become_whole_class_queries() {
    for path in [
        r"root\cimv2:Win32_Process.garbage",
        r"\\.\root\cimv2:Win32_Process.garbage",
        r"\\127.0.0.1\root\cimv2:Win32_Process.Handle",
        r"\\.\root\cimv2:",
    ] {
        let result = run(&["path", path, "get", "Name"], b"");
        assert_eq!(result.status.code(), Some(58), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("Invalid object path"));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("Name="));
    }
}
