mod support;
use support::wide;
use wmic_compat::model::{EntryMode, ModifierScope, SwitchValue, Target, Verb, WideText};
use wmic_compat::parse::parse_command;

#[test]
fn parser_preserves_wql_quotes() {
    let command = parse_command(
        &wide(r#""C:\Program Files\wmic.exe" os where "Version like '10.%'" get Caption,Version"#),
        EntryMode::Process,
    )
    .unwrap();
    assert_eq!(command.selector, Some(wide("Version like '10.%'")));
    assert_eq!(command.target, Some(Target::Alias(wide("os"))));
    assert_eq!(command.properties, vec![wide("Caption"), wide("Version")]);
}

#[test]
fn parser_preserves_unpaired_utf16() {
    let mut input = wide("wmic.exe os where \"").0;
    input.push(0xD800);
    input.extend(wide("\" get Caption").0);
    let raw = WideText(input);
    let command = parse_command(&raw, EntryMode::Process).unwrap();
    assert_eq!(command.selector.unwrap().0, vec![0xD800]);
    assert_eq!(command.raw, raw);
}

#[test]
fn parser_distinguishes_reset_from_omitted() {
    let reset = parse_command(
        &wide(r#"wmic.exe /namespace:"" os get Caption"#),
        EntryMode::Process,
    )
    .unwrap();
    let omitted = parse_command(&wide("wmic.exe os get Caption"), EntryMode::Process).unwrap();
    assert_eq!(reset.modifiers[0].value, SwitchValue::Reset);
    assert_eq!(reset.modifiers[0].scope, ModifierScope::Global);
    assert!(omitted.modifiers.is_empty());
}

#[test]
fn parser_does_not_split_quoted_commas() {
    let command = parse_command(
        &wide(r#"wmic.exe process call create "cmd /c echo a,b",null,0"#),
        EntryMode::Process,
    )
    .unwrap();
    assert_eq!(command.method, Some(wide("create")));
    assert_eq!(
        command.arguments,
        vec![wide("cmd /c echo a,b"), wide("null"), wide("0")]
    );
}

#[test]
fn keyword_case_does_not_change_data_spelling() {
    let command = parse_command(&wide("OS GeT cApTiOn,vErSiOn"), EntryMode::Interactive).unwrap();
    assert_eq!(command.verb, Verb::Get);
    assert_eq!(command.target, Some(Target::Alias(wide("OS"))));
    assert_eq!(command.properties, vec![wide("cApTiOn"), wide("vErSiOn")]);
}

#[test]
fn global_path_and_format_order_remain_intact() {
    let command = parse_command(&wide(r#"wmic.exe /output:"C:\some dir\answer.txt" os get Caption /format:csv /translate:NoComma"#), EntryMode::Process).unwrap();
    assert_eq!(
        command.modifiers[0].value,
        SwitchValue::Value(wide(r"C:\some dir\answer.txt"))
    );
    assert_eq!(command.modifiers[1].name, wide("format"));
    assert_eq!(command.modifiers[2].name, wide("translate"));
    assert_eq!(command.modifiers[1].scope, ModifierScope::Verb);
}

#[test]
fn direct_object_path_keeps_key_quotes() {
    let command = parse_command(
        &wide(r#"wmic.exe path Win32_Process.Handle="123" get Name"#),
        EntryMode::Process,
    )
    .unwrap();
    assert_eq!(
        command.target,
        Some(Target::Path(wide(r#"Win32_Process.Handle="123""#)))
    );
}

#[test]
fn list_view_and_default_verb_are_distinct_from_help_scope() {
    let brief = parse_command(&wide("os list brief"), EntryMode::Interactive).unwrap();
    assert_eq!(brief.verb, Verb::List);
    assert_eq!(brief.view, Some(wide("brief")));
    let alias_help = parse_command(&wide("os /?"), EntryMode::Interactive).unwrap();
    let list_help = parse_command(&wide("os list /?"), EntryMode::Interactive).unwrap();
    assert!(alias_help.help && list_help.help);
    assert!(!alias_help.verb_explicit);
    assert!(list_help.verb_explicit);
}

#[test]
fn method_help_does_not_become_a_method_call() {
    let help = parse_command(&wide("process call /?"), EntryMode::Interactive).unwrap();
    assert!(help.help);
    assert_eq!(help.verb, Verb::Call);
    assert!(help.method.is_none());
}

#[test]
fn context_exit_and_interactive_entry_have_no_provider_target() {
    for (text, verb) in [
        ("context", Verb::Context),
        ("quit", Verb::Exit),
        ("exit", Verb::Exit),
    ] {
        let command = parse_command(&wide(text), EntryMode::Interactive).unwrap();
        assert_eq!(command.verb, verb);
        assert!(command.target.is_none());
    }
    assert_eq!(
        parse_command(&wide("wmic.exe"), EntryMode::Process)
            .unwrap()
            .verb,
        Verb::Interactive
    );
}

#[test]
fn unknown_global_switch_has_native_status() {
    let error = parse_command(
        &wide("wmic.exe /wmic_research_invalid:1 os get Caption"),
        EntryMode::Process,
    )
    .unwrap_err();
    assert_eq!(error.code, 0xAC5D);
}

#[test]
fn global_switches_are_not_accepted_at_alias_or_get_level() {
    assert_eq!(
        parse_command(
            &wide(r"os /namespace:root\cimv2 get Caption"),
            EntryMode::Interactive
        )
        .unwrap_err()
        .code,
        0xAC5C
    );
    assert_eq!(
        parse_command(
            &wide(r"os get Caption /namespace:root\cimv2"),
            EntryMode::Interactive
        )
        .unwrap_err()
        .code,
        0xACB4
    );
}

#[test]
fn unquoted_multiword_where_matches_native_invalid_verb() {
    let error = parse_command(
        &wide("os where Version like '10.%' get Caption,Version"),
        EntryMode::Interactive,
    )
    .unwrap_err();
    assert_eq!(error.code, 0xABFD);
    assert_eq!(error.token, Some(wide("like")));
}

#[test]
fn parenthesized_where_is_one_expression() {
    let command = parse_command(
        &wide("os where (Version like '10.%') get Caption,Version"),
        EntryMode::Interactive,
    )
    .unwrap();
    assert_eq!(command.selector, Some(wide("(Version like '10.%')")));
}

#[test]
fn double_quoted_wql_value_is_preserved() {
    let command = parse_command(
        &wide(r#"os where Caption="Microsoft Windows 11 Pro" get Version"#),
        EntryMode::Interactive,
    )
    .unwrap();
    assert_eq!(
        command.selector,
        Some(wide(r#"Caption="Microsoft Windows 11 Pro""#))
    );
}

#[test]
fn get_commas_are_required_but_spaces_after_commas_are_valid() {
    let spaced = parse_command(&wide("os get Caption, Version"), EntryMode::Interactive).unwrap();
    assert_eq!(spaced.properties, vec![wide("Caption"), wide("Version")]);
    assert_eq!(
        parse_command(&wide("os get Caption Version"), EntryMode::Interactive)
            .unwrap_err()
            .code,
        0xACBC
    );
}

#[test]
fn abbreviated_value_switch_is_not_silently_accepted() {
    assert_eq!(
        parse_command(&wide("os get Caption /v"), EntryMode::Interactive)
            .unwrap_err()
            .code,
        0xACB4
    );
}

#[test]
fn format_parameters_keep_their_individual_quotes() {
    let command = parse_command(
        &wide(r#"os get Caption /format:"htable.xsl":"sortby=Caption":"datatype=text""#),
        EntryMode::Interactive,
    )
    .unwrap();
    assert_eq!(
        command.modifiers[0].value,
        SwitchValue::Value(wide(r#""htable.xsl":"sortby=Caption":"datatype=text""#))
    );
}

#[test]
fn an_unrecognized_alias_argument_remains_available_for_pwhere_resolution() {
    let command = parse_command(&wide("os ge Caption"), EntryMode::Interactive).unwrap();
    assert_eq!(command.verb, Verb::List);
    assert_eq!(command.positionals, vec![wide("ge"), wide("Caption")]);
}

#[test]
fn embedded_nul_is_rejected_without_truncation() {
    let mut input = wide("wmic.exe os").0;
    input.extend([0, b'x' as u16]);
    assert!(parse_command(&WideText(input), EntryMode::Process).is_err());
}
