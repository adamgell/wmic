mod support;
use support::wide;
use wmic_compat::{backend::write::bind_properties, model::*};
#[test]
fn write_arguments_bind_by_property_schema() {
    let schema = ClassSchema {
        name: wide("Test"),
        properties: vec![
            PropertySchema {
                name: wide("Enabled"),
                cim_type: 11,
                qualifiers: vec![],
            },
            PropertySchema {
                name: wide("Value"),
                cim_type: 8,
                qualifiers: vec![],
            },
        ],
        ..Default::default()
    };
    let properties =
        bind_properties(&schema, &[wide("value=\"a,b\""), wide("Enabled=FALSE")]).unwrap();
    assert_eq!(properties[0].name, wide("Value"));
    assert_eq!(properties[0].value, CimValue::String(wide("a,b")));
    assert_eq!(properties[1].value, CimValue::Boolean(false));
    assert!(bind_properties(&schema, &[wide("Missing=1")]).is_err());
    assert!(bind_properties(&schema, &[wide("Value=x"), wide("value=y")]).is_err());
}
#[test]
fn value_output_escapes_xml_text_like_native_wmic() {
    assert_eq!(
        wmic_compat::render::display_value(&CimValue::String(wide("a & <value>"))),
        wide("a &amp; &lt;value&gt;")
    );
}

#[test]
fn declined_confirmation_is_forwarded_before_any_change() {
    use wmic_compat::{
        backend::{Confirmation, Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider {
        changed: bool,
    }
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("interactive writes require the confirmation path")
        }
        fn execute_with_confirmation(
            &mut self,
            r: &ResolvedRequest,
            confirmation: &mut dyn Confirmation,
        ) -> Result<ManagementResult, ManagementError> {
            self.changed = confirmation.confirm(&wide("Delete 'owned' (Y/N/?)? "))?;
            Ok(ManagementResult::Operation(OperationResult {
                action: r.verb,
                paths: vec![],
                instances: vec![Record::default()],
            }))
        }
    }
    #[derive(Default)]
    struct Sink(WideText);
    impl OutputSink for Sink {
        fn read_line(&mut self) -> Result<Option<WideText>, OutputError> {
            Ok(Some(wide("N")))
        }
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if !t.0.is_empty() {
                assert_eq!(s, OutputStream::Stderr);
                self.0.0.extend(&t.0);
            }
            Ok(())
        }
    }
    let mut provider = Provider { changed: false };
    let mut sink = Sink::default();
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("path Owned delete /interactive"),
        EntryMode::Interactive,
        &mut provider,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
    assert!(!provider.changed);
    assert_eq!(sink.0, wide("Delete 'owned' (Y/N/?)? "));
}

#[test]
fn question_mark_confirmation_shows_measured_diagnostic_and_reprompts() {
    use wmic_compat::{
        backend::{Confirmation, Management, ResolvedRequest},
        render::OutputError,
        session::{OutputSink, OutputStream, Session},
    };
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!()
        }
        fn execute_with_confirmation(
            &mut self,
            r: &ResolvedRequest,
            c: &mut dyn Confirmation,
        ) -> Result<ManagementResult, ManagementError> {
            assert!(!c.confirm(&wide("Delete 'owned' (Y/N/?)? "))?);
            Ok(ManagementResult::Operation(OperationResult {
                action: r.verb,
                paths: vec![],
                instances: vec![Record::default()],
            }))
        }
    }
    struct Sink {
        input: Vec<WideText>,
        output: WideText,
    }
    impl OutputSink for Sink {
        fn read_line(&mut self) -> Result<Option<WideText>, OutputError> {
            Ok(if self.input.is_empty() {
                None
            } else {
                Some(self.input.remove(0))
            })
        }
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if !t.0.is_empty() {
                assert_eq!(s, OutputStream::Stderr);
                self.output.0.extend(&t.0)
            }
            Ok(())
        }
    }
    let mut sink = Sink {
        input: vec![wide("?"), wide("N")],
        output: wide(""),
    };
    let out = Session::new(wide("LOCAL")).unwrap().execute(
        &wide("path Owned delete /interactive"),
        EntryMode::Interactive,
        &mut Provider,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(
        sink.output,
        wide(
            "Delete 'owned' (Y/N/?)? Invalid XSL format (or) file name.\r\r\nDelete 'owned' (Y/N/?)? "
        )
    );
}

#[test]
fn confirmed_update_suppresses_redundant_updating_line() {
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        parse::parse_command,
        render::{RenderRequest, render},
        session::SessionSettings,
    };
    let command = parse_command(
        &wide("/interactive:on path Win32_Environment where Name='owned' set VariableValue=x"),
        EntryMode::Interactive,
    )
    .unwrap();
    let ResolvedCommand::Management(request) = AliasStore::stock()
        .unwrap()
        .resolve(&command, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    let value = ManagementResult::Operation(OperationResult {
        action: Verb::Set,
        paths: vec![wide("Win32_Environment.Name=\"owned\"")],
        instances: vec![],
    });
    assert_eq!(
        render(RenderRequest {
            request: &request,
            result: Ok(&value),
            node: &wide("LOCAL")
        })
        .unwrap()
        .stdout,
        wide("Property(s) update successful.\r\r\n")
    );
}

#[test]
fn failed_write_retains_completed_objects_and_current_target() {
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        parse::parse_command,
        render::{RenderRequest, render},
        session::SessionSettings,
    };
    let p = parse_command(&wide("path Test set Value=x"), EntryMode::Interactive).unwrap();
    let ResolvedCommand::Management(request) = AliasStore::stock()
        .unwrap()
        .resolve(&p, &SessionSettings::default())
        .unwrap()
    else {
        panic!()
    };
    let error = ManagementError {
        code: 0x80041001,
        target: wide("Test.Key=\"second\""),
        message: wide("Generic failure"),
        operation: wide("update"),
        extended: vec![],
        partial: Some(Box::new(ManagementResult::Operation(OperationResult {
            action: Verb::Set,
            paths: vec![wide("Test.Key=\"first\"")],
            instances: vec![],
        }))),
    };
    let out = render(RenderRequest {
        request: &request,
        result: Err(&error),
        node: &wide("LOCAL"),
    })
    .unwrap();
    assert_eq!(
        out.stdout,
        wide(
            "Updating property(s) of 'Test.Key=\"first\"'\r\r\nProperty(s) update successful.\r\r\nUpdating property(s) of 'Test.Key=\"second\"'\r\r\n"
        )
    );
    assert!(out.stderr.to_string_lossy().contains("Generic failure"));
}
