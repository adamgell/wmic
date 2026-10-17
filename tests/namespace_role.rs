use wmic_compat::{
    model::{EntryMode, WideText},
    parse::parse_command,
    session::SessionSettings,
};

fn settings(arguments: &str) -> SessionSettings {
    let command = parse_command(&arguments.into(), EntryMode::Interactive).unwrap();
    SessionSettings::default().for_command(&command).unwrap()
}

// Microsoft ARM64 WMIC 2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c,
// rea-goal-25/batch-01-06 reference captures p04-00/p05-00 and p04-03/p05-03.
#[test]
fn equal_namespace_keeps_the_existing_spelling() {
    let actual = settings(r"/namespace:\\ROOT\CIMV2 context");
    assert_eq!(actual.namespace, WideText::from(r"root\cimv2"));
}

#[test]
fn equal_role_keeps_the_existing_spelling() {
    let actual = settings(r"/role:\\ROOT\CLI context");
    assert_eq!(actual.role, WideText::from(r"root\cli"));
}

#[test]
fn namespace_drops_one_trailing_backslash() {
    let actual = settings(r"/namespace:\\root\cimv2\ context");
    assert_eq!(actual.namespace, WideText::from(r"root\cimv2"));
}

#[test]
fn role_drops_one_trailing_backslash() {
    let actual = settings(r"/role:\\root\cli\ context");
    assert_eq!(actual.role, WideText::from(r"root\cli"));
}

// Additional CONTEXT-only namespace-parent/reference captures confirm the
// shared parent-prefix helper for both switch names.
#[test]
fn parent_prefix_removes_the_last_namespace_or_role_component() {
    let actual = settings(r"/namespace:.. /role:.. context");
    assert_eq!(actual.namespace, WideText::from("root"));
    assert_eq!(actual.role, WideText::from("root"));
}

#[test]
fn parent_prefix_appends_its_suffix_without_adding_a_separator() {
    let actual = settings(r"/namespace:..\Child /role:..\Child context");
    assert_eq!(actual.namespace, WideText::from(r"root\Child"));
    assert_eq!(actual.role, WideText::from(r"root\Child"));
}

#[test]
fn parent_prefix_does_not_require_a_separator_after_the_dots() {
    let actual = settings(r"/namespace:..Child /role:..Child context");
    assert_eq!(actual.namespace, WideText::from("rootChild"));
    assert_eq!(actual.role, WideText::from("rootChild"));
}

#[test]
fn parent_prefix_keeps_a_single_remaining_component() {
    let actual = settings(r"/namespace:.. /namespace:.. /role:.. /role:.. context");
    assert_eq!(actual.namespace, WideText::from("root"));
    assert_eq!(actual.role, WideText::from("root"));
}

#[test]
fn equal_custom_paths_keep_the_first_stored_spelling() {
    let actual = settings(
        r"/namespace:\\Root\Mixed /namespace:\\root\mixed /role:\\Root\Mixed /role:\\root\mixed context",
    );
    assert_eq!(actual.namespace, WideText::from(r"Root\Mixed"));
    assert_eq!(actual.role, WideText::from(r"Root\Mixed"));
}

#[test]
fn quoted_empty_values_restore_default_paths_after_custom_paths() {
    let actual =
        settings(r#"/namespace:\\Root\Mixed /namespace:"" /role:\\Root\Mixed /role:"" context"#);
    assert_eq!(actual.namespace, WideText::from(r"root\cimv2"));
    assert_eq!(actual.role, WideText::from(r"root\cli"));
}

#[test]
fn namespace_reset_with_an_unrelated_custom_role_restores_the_default() {
    let actual = settings(r#"/role:\\Something /namespace:other /namespace:"" context"#);
    assert_eq!(actual.namespace, WideText::from(r"root\cimv2"));
    assert_eq!(actual.role, WideText::from("Something"));
}

// unusual-reset-2/reference, role-cross-default: the original namespace setter
// checks the current ROLE against root\cimv2 before restoring the namespace.
#[test]
fn namespace_reset_keeps_the_namespace_when_the_role_equals_default_namespace() {
    let actual = settings(r#"/role:\\root\cimv2 /namespace:other /namespace:"" context"#);
    assert_eq!(actual.namespace, WideText::from(r"root\cimv2\other"));
    assert_eq!(actual.role, WideText::from(r"root\cimv2"));
}
