mod support;
use std::{fs, path::PathBuf};
use wmic_compat::{model::*, render::OutputError, session::*};
#[derive(Default)]
struct Capture(Vec<(OutputStream, WideText)>);
impl OutputSink for Capture {
    fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
        self.0.push((s, t.clone()));
        Ok(())
    }
}
fn directory() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "wmic-output-{}-{}-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&p).unwrap();
    p
}
#[test]
fn output_and_append_are_independent_and_preserve_unrelated_file() {
    let dir = directory();
    let out = dir.join("out.txt");
    let append = dir.join("append.txt");
    let sentinel = dir.join("unrelated");
    fs::write(&append, b"SEED\r\n").unwrap();
    fs::write(&sentinel, [0, 255, 42]).unwrap();
    let settings = SessionSettings {
        output: out.to_str().unwrap().into(),
        append: append.to_str().unwrap().into(),
        ..Default::default()
    };
    let mut sink = RoutedOutput::new(Capture::default());
    sink.configure(&settings).unwrap();
    sink.write(OutputStream::Stdout, &"A\r\r\n\r\r\n".into())
        .unwrap();
    sink.write(OutputStream::Stderr, &"Error\r\r\n".into())
        .unwrap();
    sink.finish().unwrap();
    assert_eq!(
        fs::read(&out).unwrap(),
        [
            vec![255, 254],
            "A\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect()
        ]
        .concat()
    );
    assert_eq!(fs::read(&append).unwrap(), b"SEED\r\nA\r\n\r\n");
    assert_eq!(fs::read(&sentinel).unwrap(), [0, 255, 42]);
    assert_eq!(
        sink.inner.0,
        vec![(OutputStream::Stderr, "Error\r\r\n".into())]
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn new_append_uses_utf16_bom_and_duplicates_to_stdout() {
    let dir = directory();
    let append = dir.join("append.txt");
    let mut sink = RoutedOutput::new(Capture::default());
    sink.configure(&SessionSettings {
        append: append.to_str().unwrap().into(),
        ..Default::default()
    })
    .unwrap();
    sink.write(OutputStream::Stdout, &"A\r\r\n\r\r\n".into())
        .unwrap();
    sink.finish().unwrap();
    assert_eq!(
        fs::read(append).unwrap(),
        [
            vec![255, 254],
            "A\r\n\r\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect()
        ]
        .concat()
    );
    assert_eq!(sink.inner.0[0].1, WideText::from("A\r\r\n\r\r\n"));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_destination_fails_before_execution() {
    let dir = directory();
    let mut sink = RoutedOutput::new(Capture::default());
    assert!(
        sink.configure(&SessionSettings {
            output: dir.to_str().unwrap().into(),
            ..Default::default()
        })
        .is_err()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recording_uses_utf8_and_keeps_command_text_quotes() {
    let dir = directory();
    let file = dir.join("record.xml");
    let mut sink = RoutedOutput::new(Capture::default());
    sink.configure(&SessionSettings {
        record: Some(file.to_str().unwrap().into()),
        ..Default::default()
    })
    .unwrap();
    let context = RenderContext {
        sequence: 1,
        every_count: 0,
        issued_from: "HOST".into(),
        started_at: "fixed".into(),
        command_line: " /record:\"owned file\" path Sample where \"A='<x>'\" get A".into(),
        nodes: vec![],
        settings: vec![],
        components_xml: WideText::default(),
    };
    sink.command_context(&context).unwrap();
    sink.context(&context, &"HOST".into()).unwrap();
    sink.write(OutputStream::Stdout, &"é字\r\r\n\r\r\n".into())
        .unwrap();
    sink.finish().unwrap();
    let text = String::from_utf8(fs::read(file).unwrap()).unwrap();
    assert!(
        text.contains("<COMMANDLINE>  path Sample where \"A='&lt;x&gt;'\" get A</COMMANDLINE>")
    );
    assert!(text.contains("<![CDATA[é字\r\n]]>"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn parser_error_is_recorded_once_and_quit_preserves_the_output_file() {
    use std::collections::VecDeque;
    use wmic_compat::backend::{Management, ResolvedRequest};
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Ok(ManagementResult::Objects(ObjectSet {
                objects: vec![Record {
                    class: "Win32_OperatingSystem".into(),
                    properties: vec![Property {
                        name: "Caption".into(),
                        cim_type: 8,
                        value: CimValue::String("Test".into()),
                        system: false,
                        qualifiers: vec![],
                    }],
                    ..Default::default()
                }],
                schema: None,
            }))
        }
    }
    struct Input(VecDeque<InputEvent>);
    impl InputSource for Input {
        fn next_input(&mut self) -> Result<InputEvent, InputError> {
            Ok(self.0.pop_front().unwrap_or(InputEvent::Eof))
        }
    }
    let dir = directory();
    let output = dir.join("out.txt");
    let record = dir.join("record.xml");
    let mut session = Session::new("HOST".into()).unwrap();
    session.settings.output = output.to_str().unwrap().into();
    session.settings.record = Some(record.to_str().unwrap().into());
    let mut input = Input(
        ["os get Caption", "/nonsense", "quit"]
            .into_iter()
            .map(|s| InputEvent::Line {
                line: s.into(),
                console: false,
            })
            .collect(),
    );
    let mut sink = RoutedOutput::new(Capture::default());
    assert_eq!(
        run_interactive(&mut session, &mut Provider, &mut input, &mut sink).exit_status,
        0
    );
    let out = fs::read(output).unwrap();
    assert!(out.windows(8).any(|s| s == [84, 0, 101, 0, 115, 0, 116, 0]));
    let xml = String::from_utf8(fs::read(record).unwrap()).unwrap();
    assert_eq!(xml.matches("<RECORD SEQUENCENUM=").count(), 2);
    assert!(xml.contains("<COMMANDLINE>/nonsense</COMMANDLINE>"));
    assert_eq!(xml.matches("Invalid Global Switch.").count(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_password_never_reaches_stderr_or_the_complete_record() {
    use wmic_compat::backend::{Management, ResolvedRequest};
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("invalid input must not execute")
        }
    }
    for command in [
        "/password:\"DO_NOT_LOG_TEST_SECRET",
        "/password: \"DO_NOT_LOG_TEST_SECRET",
        "/PASSWORD:\"DO_NOT_LOG_TEST_SECRET",
        "/password: get \"DO_NOT_LOG_TEST_SECRET",
        "/password：DO_NOT_LOG_TEST_SECRET context",
        "/password_typo:DO_NOT_LOG_TEST_SECRET context",
        "/pa\u{00ad}ssword：DO_NOT_LOG_TEST_SECRET context",
        "/pa\u{00ad}ssword_typo:DO_NOT_LOG_TEST_SECRET context",
        r#"/node:"a,b" /password:DO_NOT_LOG_TEST_SECRET context"#,
        r#"/password:first /node:"a,b" /password:DO_NOT_LOG_TEST_SECRET context"#,
    ] {
        let dir = directory();
        let record = dir.join("record.xml");
        let mut session = Session::new("HOST".into()).unwrap();
        session.settings.record = Some(record.to_str().unwrap().into());
        let mut sink = RoutedOutput::new(Capture::default());
        let out = session.execute(
            &command.into(),
            EntryMode::Interactive,
            &mut Provider,
            &mut sink,
        );
        let expected = if command.contains('：') || command.contains("_typo") {
            0xAC5D
        } else {
            0xABFD
        };
        assert_eq!(out.exit_status, expected);
        let output: Vec<_> = sink.inner.0.iter().flat_map(|(_, s)| s.0.clone()).collect();
        assert!(
            !String::from_utf16_lossy(&output).contains("DO_NOT_LOG_TEST_SECRET"),
            "diagnostic leaked a password"
        );
        let xml = fs::read_to_string(record).unwrap();
        assert!(
            !xml.contains("DO_NOT_LOG_TEST_SECRET"),
            "record leaked a password"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn credential_looking_method_parameter_names_and_values_are_not_recorded() {
    use wmic_compat::backend::{Management, ResolvedRequest};
    struct NoProvider;
    impl Management for NoProvider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("invalid method parameters must never execute a provider")
        }
    }
    for args in [
        r#""/password:DO_NOT_LOG_TEST_SECRET"=1"#,
        r#""-Ｐａｓｓｗｏｒｄ:DO_NOT_LOG_TEST_SECRET"=1"#,
        r#"Unknown="/password:DO_NOT_LOG_TEST_SECRET""#,
    ] {
        let dir = directory();
        let record = dir.join("record.xml");
        let mut session = Session::new("HOST".into()).unwrap();
        session.settings.record = Some(record.to_str().unwrap().into());
        let mut sink = RoutedOutput::new(Capture::default());
        let command = format!("process call GetOwner {args}");
        let outcome = session.execute(
            &command.as_str().into(),
            EntryMode::Interactive,
            &mut NoProvider,
            &mut sink,
        );
        assert_eq!(outcome.exit_status, 0xB0AB, "{command}");
        let text: Vec<_> = sink.inner.0.iter().flat_map(|(_, s)| s.0.clone()).collect();
        assert!(!String::from_utf16_lossy(&text).contains("DO_NOT_LOG_TEST_SECRET"));
        assert!(
            !fs::read_to_string(record)
                .unwrap()
                .contains("DO_NOT_LOG_TEST_SECRET")
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
