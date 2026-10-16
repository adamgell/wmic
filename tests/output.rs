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
