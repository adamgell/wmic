mod support;
use support::wide;
use wmic_compat::{
    alias::{AliasStore, ResolveError, ResolvedCommand, RoleLoader},
    model::*,
    parse::parse_command,
    session::SessionSettings,
};
fn resolve(text: &str) -> ResolvedCommand {
    AliasStore::stock()
        .unwrap()
        .resolve(
            &parse_command(&wide(text), EntryMode::Interactive).unwrap(),
            &SessionSettings::default(),
        )
        .unwrap()
}
#[test]
fn stock_aliases_work_without_root_cli() {
    let store = AliasStore::stock().unwrap();
    assert_eq!(store.definitions().len(), 81);
    for (alias, class) in [
        ("os", "Win32_OperatingSystem"),
        ("csproduct", "Win32_ComputerSystemProduct"),
        ("memorychip", "Win32_PhysicalMemory"),
    ] {
        let ResolvedCommand::Management(r) = resolve(&format!("{alias} get Name")) else {
            panic!()
        };
        assert_eq!(r.class_name, wide(class));
    }
}
struct Missing;
impl RoleLoader for Missing {
    fn load(
        &mut self,
        _: &WideText,
    ) -> Result<Option<Vec<wmic_compat::alias::AliasDefinition>>, ResolveError> {
        Ok(None)
    }
}
#[test]
fn missing_explicit_role_does_not_fallback() {
    let mut store = AliasStore::stock().unwrap();
    let settings = SessionSettings {
        role: wide(r"root\WmicResearchMissingRole"),
        explicit_role: true,
        ..Default::default()
    };
    assert_eq!(
        store.load_role(&settings, &mut Missing).unwrap_err().code,
        0x8004100e
    );
}
#[test]
fn alias_positional_selector_uses_pwhere() {
    let ResolvedCommand::Management(r) = resolve("alias OS list brief") else {
        panic!()
    };
    assert_eq!(r.class_name, wide("Msft_CliAlias"));
    assert_eq!(r.connection.namespace, wide(r"ROOT\cli"));
    assert_eq!(r.selector, Some(wide("FriendlyName = 'OS'")));
    assert_eq!(
        r.display.iter().map(|x| x.0.clone()).collect::<Vec<_>>(),
        vec![wide("FriendlyName"), wide("PWhere"), wide("Target")]
    );
}
#[test]
fn alias_seealso_expands_to_derived_command() {
    for line in ["alias OS call SeeAlso", "alias OS SeeAlso"] {
        let ResolvedCommand::Derived(d) = resolve(line) else {
            panic!()
        };
        assert_eq!(d.template, wide("wmic Alias #__PATH assoc"));
        assert_eq!(
            d.expand(&Record {
                class: wide("MSFT_CliAlias"),
                path: Some(wide(r#"MSFT_CliAlias.FriendlyName="OS""#)),
                server: None,
                properties: vec![]
            })
            .unwrap(),
            wide(r#"wmic Alias MSFT_CliAlias.FriendlyName="OS" assoc"#)
        );
        assert_eq!(d.selection.selector, Some(wide("FriendlyName = 'OS'")));
    }
}
#[test]
fn process_shorthand_and_undefined_pwhere() {
    let ResolvedCommand::Management(r) = resolve("process 4 get Name") else {
        panic!()
    };
    assert_eq!(r.selector, Some(wide("ProcessId='4'")));
    let store = AliasStore::stock().unwrap();
    let err = store
        .resolve(
            &parse_command(&wide("os ge Caption"), EntryMode::Interactive).unwrap(),
            &SessionSettings::default(),
        )
        .unwrap_err();
    assert_eq!(err.code, 0xABEA);
}
#[test]
fn namespace_switch_does_not_override_alias_connection() {
    let settings = SessionSettings {
        namespace: wide(r"root\default"),
        explicit_namespace: true,
        ..Default::default()
    };
    let ResolvedCommand::Management(r) = AliasStore::stock()
        .unwrap()
        .resolve(
            &parse_command(&wide("os get Caption"), EntryMode::Interactive).unwrap(),
            &settings,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(r.connection.namespace, wide(r"ROOT\CIMV2"));
}
#[test]
fn external_verb_is_resolved_without_execution() {
    let ResolvedCommand::External(d) = resolve("nicconfig call FlushDns") else {
        panic!()
    };
    assert_eq!(d.template, wide("ipconfig /FlushDns"));
}
#[test]
fn all_views_preserve_derived_properties_and_names() {
    let store = AliasStore::stock().unwrap();
    assert_eq!(
        store
            .definitions()
            .iter()
            .map(|a| a.formats.len())
            .sum::<usize>(),
        426
    );
    let os = store.find(&wide("OS")).unwrap();
    assert_eq!(
        os.formats
            .iter()
            .find(|f| f.name == "FULL")
            .unwrap()
            .format
            .as_deref(),
        Some("list")
    );
    assert!(
        store
            .definitions()
            .iter()
            .flat_map(|a| &a.formats)
            .all(|f| !f.name.is_empty())
    );
}
#[test]
fn unknown_alias_preserves_native_code() {
    assert_eq!(
        AliasStore::stock()
            .unwrap()
            .resolve(
                &parse_command(
                    &wide("WmicResearchMissing get Name"),
                    EntryMode::Interactive
                )
                .unwrap(),
                &SessionSettings::default()
            )
            .unwrap_err()
            .code,
        0xAC67
    );
}
#[test]
fn alias_connection_password_is_retained_without_debug_exposure() {
    let store=AliasStore::from_json(r#"[{"FriendlyName":"Sample","Target":"Win32_OperatingSystem","Connection":{"NameSpace":"root\\cimv2","User":"DOMAIN\\user","Password":"alias-test-only-secret"}}]"#).unwrap();
    assert!(!format!("{store:?}").contains("alias-test-only-secret"));
    let p = parse_command(&wide("Sample get Caption"), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(r) = store.resolve(&p, &SessionSettings::default()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        r.connection.password.unwrap().expose(),
        &wide("alias-test-only-secret")
    );
}
