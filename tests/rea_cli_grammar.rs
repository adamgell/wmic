//! Regressions measured against Microsoft WMIC 10.0.26100.1 (REA passes 1–21).
use wmic_compat::{model::*, parse::parse_command, session::SessionSettings};

fn parse(text: &str) -> ParsedCommand {
    parse_command(&text.into(), EntryMode::Interactive).unwrap()
}

#[test]
fn global_prefixes_and_colon_boundaries_match_the_native_tokenizer() {
    for text in [
        "-node:. context",
        "/node : . context",
        "/node:./aggregate:off context",
        "/aggregate : off/node:. context",
    ] {
        let command = parse(text);
        assert_eq!(command.verb, Verb::Context, "{text}");
        let settings = SessionSettings::default().for_command(&command).unwrap();
        assert_eq!(settings.nodes, vec![WideText::from(".")], "{text}");
        if text.contains("aggregate") {
            assert!(!settings.aggregate, "{text}");
        }
        assert_eq!(command.command_line, WideText::from(text));
    }
}

#[test]
fn cli_keywords_and_booleans_ignore_width_without_normalizing_data() {
    let command = parse("/ＮＯＤＥ:. /aggregate:ＯＦＦ ＣＯＮＴＥＸＴ");
    assert_eq!(command.verb, Verb::Context);
    let settings = SessionSettings::default().for_command(&command).unwrap();
    assert!(!settings.aggregate);
    assert_eq!(settings.nodes, vec![WideText::from(".")]);
    // Authentication symbolic values use a different, ordinal lookup in WMIC.
    let command = parse("/authlevel:ＰＫＴＰＲＩＶＡＣＹ context");
    assert_eq!(
        SessionSettings::default()
            .for_command(&command)
            .unwrap_err()
            .code,
        0xAC49
    );
    let command = parse("os ＧＥＴ cApTiOn");
    assert_eq!(command.verb, Verb::Get);
    assert_eq!(command.properties, vec![WideText::from("cApTiOn")]);
    use wmic_compat::alias::AliasStore;
    assert_eq!(
        AliasStore::stock()
            .unwrap()
            .resolve(&parse("ＯＳ get Caption"), &SessionSettings::default())
            .unwrap_err()
            .code,
        0xAC67
    );
}

#[test]
fn context_rejects_every_trailing_switch_except_help_with_its_own_status() {
    for text in ["context /value", "context /aggregate:off"] {
        let error = parse_command(&text.into(), EntryMode::Interactive).unwrap_err();
        assert_eq!(error.code, 0xBBF1, "{text}");
        assert_eq!(error.message, WideText::from("Invalid context syntax."));
        assert!(error.token.is_none());
    }
    assert!(parse("context /?").help);
    assert!(parse("context -?").help);
}

#[test]
fn separately_quoted_get_properties_are_unquoted_individually() {
    let command = parse("os get \"Caption\",Version");
    assert_eq!(command.properties, vec!["Caption".into(), "Version".into()]);
    assert_eq!(
        parse("os get \"Caption,Version\"").properties,
        vec![WideText::from("\"Caption,Version\"")]
    );
}

#[test]
fn a_leading_colon_is_data_rather_than_a_switch_name_slice() {
    for text in [":", "os get :", "context :"] {
        let _ = parse_command(&text.into(), EntryMode::Interactive);
    }
}

#[test]
fn native_node_quote_and_comma_scanning_consumes_the_same_command_words() {
    for text in [
        r#"/node:"a,b" context"#,
        r#"/node:".," context"#,
        r#"/node:"unfinished"#,
    ] {
        assert_eq!(parse(text).verb, Verb::Interactive, "{text}");
    }
    assert_eq!(parse(r#"/node:"a","b" context"#).verb, Verb::Context);
    let parsed = parse(r#"/node:".,,." context"#);
    assert_eq!(parsed.target, Some(Target::Alias(".\" context".into())));
}

#[test]
fn repeated_node_switches_use_an_iterative_scanner() {
    let text = format!("{}context", "/node:. ".repeat(3000));
    let command = parse(&text);
    assert_eq!(command.modifiers.len(), 3000);
    assert_eq!(command.verb, Verb::Context);
}

#[test]
fn aliases_for_the_local_machine_are_visited_only_once() {
    use wmic_compat::{
        backend::{Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    #[derive(Default)]
    struct Backend(usize);
    impl Management for Backend {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            self.0 += 1;
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    struct Sink;
    impl OutputSink for Sink {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Ok(())
        }
    }
    let mut backend = Backend::default();
    let outcome = Session::new("HOST".into()).unwrap().execute(
        &"/node:.,localhost,HOST os get Caption".into(),
        EntryMode::Interactive,
        &mut backend,
        &mut Sink,
    );
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(backend.0, 1);
}

#[test]
fn list_and_assoc_syntax_errors_are_reported_before_provider_execution() {
    use wmic_compat::alias::AliasStore;
    let aliases = AliasStore::stock().unwrap();
    for option in ["resultclass", "resultrole", "assocclass"] {
        assert!(parse(&format!("os assoc /{option} /?")).help);
    }
    let command = parse("os list ＢＲＩＥＦ");
    assert!(
        aliases
            .resolve(&command, &SessionSettings::default())
            .is_ok()
    );
    let error = aliases
        .resolve(&parse("os list br"), &SessionSettings::default())
        .unwrap_err();
    assert_eq!(error.code, 0xACB3);
    for (text, code) in [
        ("os list brief brief", 0xACB6),
        ("os assoc extra", 0xB3C3),
        ("os assoc /resultclass:", 0xB088),
        ("os assoc /resultclass", 0xB088),
        ("os assoc /invalid:x", 0xB088),
        ("process call GetOwner /? extra", 0xABE5),
    ] {
        assert_eq!(
            parse_command(&text.into(), EntryMode::Interactive)
                .unwrap_err()
                .code,
            code,
            "{text}"
        );
    }
}

#[test]
fn unknown_method_help_has_the_native_invalid_verb_diagnostic() {
    use wmic_compat::{
        alias::AliasStore,
        render::help::{HelpRequest, render_help},
    };
    let aliases = AliasStore::stock().unwrap();
    let command = parse("process call WmicReaMissing /?");
    let error = render_help(HelpRequest {
        command: &command,
        aliases: &aliases,
        schema: None,
    })
    .unwrap_err();
    assert_eq!(error.code, 0xABFD);
    assert_eq!(
        error.message,
        WideText::from("WmicReaMissing - Invalid alias verb.")
    );
}

#[test]
fn newly_accepted_password_spellings_are_redacted_in_success_and_failure() {
    use wmic_compat::parse::diagnostic_command_line;
    for prefix in ["-password", "/ＰＡＳＳＷＯＲＤ", "-Ｐａｓｓｗｏｒｄ"] {
        let text = format!("{prefix} : \"example secret\" context");
        let command = parse(&text);
        assert!(
            !command
                .command_line
                .to_string_lossy()
                .contains("example secret")
        );
        assert!(
            !diagnostic_command_line(&text.into(), EntryMode::Interactive)
                .to_string_lossy()
                .contains("example secret")
        );
        let invalid = format!("{prefix}:\"example secret");
        let error = parse_command(&invalid.clone().into(), EntryMode::Interactive).unwrap_err();
        assert!(error.token.is_none(), "{invalid}");
    }
    for text in [
        "/password：example-secret context",
        "/password_typo:example-secret context",
    ] {
        let error = parse_command(&text.into(), EntryMode::Interactive).unwrap_err();
        assert!(error.token.is_none());
        assert!(
            !diagnostic_command_line(&text.into(), EntryMode::Interactive)
                .to_string_lossy()
                .contains("example-secret")
        );
    }
}
