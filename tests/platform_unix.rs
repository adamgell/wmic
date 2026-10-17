#![cfg(unix)]
mod support;
use support::wide;
use wmic_compat::{
    model::{EntryMode, SwitchValue, Target, Verb, WideText},
    parse::{is_single_token, parse_command},
    platform::unix::{
        ExitAction,
        cmdline::{command_line, quote_argument},
        exit_action, node_from_host_name,
        output::native_newlines,
    },
};

fn rebuild(args: &[&str]) -> String {
    let units: Vec<Vec<u16>> = args.iter().map(|a| wide(a).0).collect();
    command_line(&units).to_string_lossy()
}

#[test]
fn where_clause_with_spaces_is_wrapped_in_double_quotes() {
    let line = rebuild(&[
        "wmic",
        "os",
        "where",
        "Caption like '%x%'",
        "get",
        "Caption",
    ]);
    assert_eq!(line, r#""wmic" os where "Caption like '%x%'" get Caption"#);
    let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
    assert_eq!(command.selector, Some(wide("Caption like '%x%'")));
    assert_eq!(command.target, Some(Target::Alias(wide("os"))));
    assert_eq!(command.properties, vec![wide("Caption")]);
}

#[test]
fn argument_without_whitespace_is_not_changed() {
    let line = rebuild(&["wmic", "process", "call", "create", "notepad.exe"]);
    assert_eq!(line, r#""wmic" process call create notepad.exe"#);
    let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
    assert_eq!(command.arguments, vec![wide("notepad.exe")]);
}

#[test]
fn argument_with_whitespace_and_commas_stays_one_argument() {
    let line = rebuild(&["wmic", "process", "call", "create", "cmd /c echo a,b"]);
    assert_eq!(line, r#""wmic" process call create "cmd /c echo a,b""#);
    let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
    assert_eq!(command.arguments, vec![wide("cmd /c echo a,b")]);
}

fn node_values(args: &[&str]) -> Vec<WideText> {
    let line = rebuild(args);
    let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
    match &command.modifiers[0].value {
        SwitchValue::Value(value) => vec![value.clone()],
        other => panic!("unexpected switch value {other:?}"),
    }
}

#[test]
fn quoted_node_commas_preserve_the_native_command_consumption_quirk() {
    let args = ["wmic", r#"/node:"a,b""#, "os", "get", "Caption"];
    assert_eq!(rebuild(&args), r#""wmic" /node:"a,b" os get Caption"#);
    assert_eq!(node_values(&args), vec![wide(r#""a,b" os get Caption"#)]);
    assert_eq!(
        parse_command(&wide(&rebuild(&args)), EntryMode::Process)
            .unwrap()
            .verb,
        Verb::Interactive
    );
}

#[test]
fn quotes_removed_by_the_shell_are_not_restored() {
    let args = ["wmic", "/node:a,b", "os", "get", "Caption"];
    assert_eq!(rebuild(&args), r#""wmic" /node:a,b os get Caption"#);
    assert_eq!(node_values(&args), vec![wide("a,b")]);
}

#[test]
fn argument_that_is_already_one_token_is_not_wrapped() {
    for arg in [
        r#"/output:"C:\my file.txt""#,
        "Name='a b'",
        r#""cmd /c echo a,b",null,0"#,
    ] {
        assert!(is_single_token(&wide(arg).0));
        assert_eq!(quote_argument(&wide(arg).0), wide(arg).0);
    }
}

#[test]
fn empty_argument_stays_visible() {
    assert_eq!(
        rebuild(&["wmic", "/namespace:", ""]),
        r#""wmic" /namespace: """#
    );
}

#[test]
fn program_name_is_quoted_and_a_quote_in_it_is_replaced() {
    assert_eq!(
        rebuild(&["./target/debug/wmic"]),
        r#""./target/debug/wmic""#
    );
    assert_eq!(rebuild(&["a\"b", "os"]), r#""wmic" os"#);
    assert_eq!(rebuild(&[]), r#""wmic""#);
}

#[test]
fn every_wrapped_argument_lexes_as_one_token() {
    for arg in [
        "a b",
        "a b\\",
        "a b\\\\",
        "say \"hi there\"",
        "say \"hi there\" and 'x y'",
        "a \"b\\\" c\\",
        "tab\tseparated",
        "line\nbreak",
        "' '",
        "\" '",
    ] {
        let quoted = quote_argument(&wide(arg).0);
        assert!(is_single_token(&quoted), "{arg:?} -> {quoted:?}");
    }
}

#[test]
fn trailing_backslash_does_not_escape_the_closing_quote() {
    assert_eq!(rebuild(&["wmic", "a b\\"]), "\"wmic\" \"a b\\\\\"");
    assert_eq!(rebuild(&["wmic", "a \"b\\"]), "\"wmic\" 'a \"b\\\\'");
    assert_eq!(
        rebuild(&["wmic", "a \"b' c\\"]),
        "\"wmic\" \"a \\\"b' c\\\\\""
    );
}

#[test]
fn backslashes_before_an_escaped_quote_are_doubled() {
    assert_eq!(
        rebuild(&["wmic", "a \\\"b' c"]),
        "\"wmic\" \"a \\\\\\\"b' c\""
    );
}

#[test]
fn lone_surrogate_passes_through() {
    let args = vec![wide("wmic").0, vec![0xD800], vec![0x61, 0xDC00, 32, 0x62]];
    assert_eq!(
        command_line(&args).0,
        [
            wide("\"wmic\" ").0,
            vec![0xD800, 32, 34, 0x61, 0xDC00, 32, 0x62, 34]
        ]
        .concat()
    );
}

#[test]
fn no_arguments_start_the_shell() {
    let command = parse_command(&wide(&rebuild(&["wmic"])), EntryMode::Process).unwrap();
    assert_eq!(command.verb, Verb::Interactive);
    assert!(command.modifiers.is_empty());
}

#[test]
fn exit_status_mapping_keeps_failures_nonzero() {
    assert_eq!(exit_action(0), ExitAction::Code(0));
    assert_eq!(exit_action(0xC000013A), ExitAction::Interrupt);
    assert_eq!(exit_action(0x80070005), ExitAction::Code(5));
    assert_eq!(exit_action(0x80041002), ExitAction::Code(2));
    assert_eq!(exit_action(0xAC67), ExitAction::Code(0x67));
    assert_eq!(exit_action(0xACB2), ExitAction::Code(0xB2));
    assert_eq!(exit_action(0x80041000), ExitAction::Code(1));
    assert_eq!(exit_action(0x100), ExitAction::Code(1));
    assert_eq!(exit_action(0xFFFF_FFFF), ExitAction::Code(255));
}

#[test]
fn node_name_is_the_first_label_in_upper_case() {
    assert_eq!(
        node_from_host_name(b"Jasons-Mac-mini.lan"),
        wide("JASONS-MAC-MINI")
    );
    assert_eq!(node_from_host_name(b"host"), wide("HOST"));
    assert_eq!(node_from_host_name(b".lan"), wide("."));
    assert_eq!(node_from_host_name(b""), wide("."));
}

#[test]
fn terminal_output_uses_line_feed_only() {
    let text = wide("a\r\r\nb\r\nc\rd\n").0;
    assert_eq!(native_newlines(&text), wide("a\nb\nc\rd\n").0);
}

#[test]
fn start_time_in_a_recording_has_the_command_line_format() {
    use std::collections::VecDeque;
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        model::{ManagementError, ManagementResult, ObjectSet},
        render::OutputError,
        session::*,
    };
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    struct Input(VecDeque<InputEvent>);
    impl InputSource for Input {
        fn next_input(&mut self) -> Result<InputEvent, InputError> {
            Ok(self.0.pop_front().unwrap_or(InputEvent::Eof))
        }
    }
    struct Null;
    impl OutputSink for Null {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Ok(())
        }
    }
    let record = std::env::temp_dir().join(format!("wmic-start-time-{}.xml", std::process::id()));
    let mut session = Session::new("HOST".into()).unwrap();
    session.settings.record = Some(record.to_str().unwrap().into());
    let mut input = Input(
        [InputEvent::Line {
            line: "os get Caption".into(),
            console: false,
        }]
        .into(),
    );
    let mut sink = RoutedOutput::new(Null);
    run_interactive(&mut session, &mut Provider, &mut input, &mut sink);
    let xml = String::from_utf8(std::fs::read(&record).unwrap()).unwrap();
    std::fs::remove_file(record).unwrap();
    let start = xml
        .split("STARTTIME=\"")
        .nth(1)
        .expect("STARTTIME attribute");
    let value = &start[..start.find('"').unwrap()];
    assert_eq!(value.len(), 19, "{value}");
    for (index, byte) in value.bytes().enumerate() {
        match index {
            2 | 5 => assert_eq!(byte, b'-', "{value}"),
            10 => assert_eq!(byte, b'T', "{value}"),
            13 | 16 => assert_eq!(byte, b':', "{value}"),
            _ => assert!(byte.is_ascii_digit(), "{value}"),
        }
    }
}

fn switch_value(args: &[&str]) -> (String, SwitchValue) {
    let line = rebuild(args);
    let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
    assert_eq!(command.modifiers.len(), 1, "{line}");
    (
        command.modifiers[0].name.to_string_lossy(),
        command.modifiers[0].value.clone(),
    )
}

#[test]
fn a_switch_value_with_whitespace_stays_a_switch_value() {
    for (arg, name, value) in [
        ("/output:/tmp/a b.txt", "output", "/tmp/a b.txt"),
        ("/password:a b", "password", "a b"),
        ("/output:say \"hi there\"", "output", "say \"hi there\""),
    ] {
        let args = ["wmic", arg, "os", "get", "Caption"];
        assert_eq!(
            switch_value(&args),
            (name.to_string(), SwitchValue::Value(wide(value))),
            "{arg}"
        );
        let command = parse_command(&wide(&rebuild(&args)), EntryMode::Process).unwrap();
        assert_eq!(command.target, Some(Target::Alias(wide("os"))), "{arg}");
    }
}

#[test]
fn a_verb_level_switch_value_with_whitespace_stays_a_switch_value() {
    let line = rebuild(&["wmic", "os", "get", "Caption", "/format:my style.xsl"]);
    let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
    let format = command
        .modifiers
        .iter()
        .find(|m| m.name.eq_ignore_case("format"))
        .unwrap();
    assert_eq!(format.value, SwitchValue::Value(wide("my style.xsl")));
    assert_eq!(command.properties, vec![wide("Caption")]);
}

#[test]
fn a_switch_prefix_with_whitespace_is_wrapped_as_one_token() {
    assert_eq!(quote_argument(&wide("/a b:c").0), wide("\"/a b:c\"").0);
    assert_eq!(quote_argument(&wide("/a b").0), wide("\"/a b\"").0);
}

#[test]
fn a_password_never_reaches_stderr_when_the_alias_is_unknown() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        model::{ManagementError, ManagementResult, ObjectSet},
        render::{DestinationProfile, OutputError, encode_output},
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    #[derive(Default)]
    struct Sink(Vec<u8>);
    impl OutputSink for Sink {
        fn write(&mut self, _: OutputStream, text: &WideText) -> Result<(), OutputError> {
            self.0
                .extend(encode_output(text, &DestinationProfile::pipe(1252))?);
            Ok(())
        }
    }
    for line in [
        rebuild(&["wmic", "/password:a b", "bogus", "get", "Caption"]),
        rebuild(&["wmic", "/PASSWORD:a b", "bogus", "get", "Caption"]),
        rebuild(&["wmic", "bogus", "get", "Caption", "/password:a b"]),
    ] {
        let mut sink = Sink::default();
        let out = Session::new("TEST".into()).unwrap().execute(
            &wide(&line),
            EntryMode::Process,
            &mut Provider,
            &mut sink,
        );
        let text = String::from_utf8_lossy(&sink.0).into_owned();
        assert_ne!(out.exit_status, 0, "{line}: {text}");
        assert!(!text.contains("a b"), "{line}: {text}");
    }
}

#[test]
fn wrapped_arguments_keep_their_text_unless_they_mix_both_quote_kinds_or_end_in_a_backslash() {
    for arg in [
        "a b",
        "say \"hi there\"",
        "it's a test",
        "tab\tseparated",
        "line\nbreak",
        r"C:\my dir\file.txt",
        "a, b",
    ] {
        let line = rebuild(&["wmic", "process", "call", "create", arg]);
        let command = parse_command(&wide(&line), EntryMode::Process).unwrap();
        assert_eq!(command.arguments, vec![wide(arg)], "{arg:?} -> {line}");
    }
}
