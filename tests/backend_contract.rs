mod support;
use support::wide;
use wmic_compat::{
    backend::{AutomationValue, collect_records, convert_value},
    model::*,
};
#[test]
fn null_empty_string_and_array_remain_distinct() {
    assert_eq!(
        convert_value(AutomationValue::Null, 8).unwrap(),
        CimValue::Null
    );
    assert_eq!(
        convert_value(AutomationValue::String(wide("")), 8).unwrap(),
        CimValue::String(wide(""))
    );
    assert_eq!(
        convert_value(AutomationValue::Array(vec![]), 0x2008).unwrap(),
        CimValue::Array(vec![])
    );
}
#[test]
fn decimal_uint64_does_not_round_through_f64() {
    assert_eq!(
        convert_value(AutomationValue::String(wide("18446744073709551615")), 21).unwrap(),
        CimValue::Uint64(u64::MAX)
    );
    assert!(convert_value(AutomationValue::String(wide("18446744073709551616")), 21).is_err());
}
#[test]
fn embedded_objects_preserve_types() {
    let record = Record {
        wmi_xml: None,
        result_node: None,
        class: wide("Inner"),
        path: None,
        server: None,
        properties: vec![],
    };
    assert_eq!(
        convert_value(AutomationValue::Object(Box::new(record.clone())), 13).unwrap(),
        CimValue::Object(Box::new(record))
    );
}
#[test]
fn enumeration_error_preserves_partial_records_and_hresult() {
    let r = Record {
        wmi_xml: None,
        result_node: None,
        class: wide("Sample"),
        path: None,
        server: None,
        properties: vec![],
    };
    let e = ManagementError {
        code: 0x80041003,
        target: wide("node-b"),
        message: wide("Access denied"),
        operation: wide("enumerate"),
        extended: vec![],
        partial: None,
    };
    let err = collect_records(vec![Ok(r.clone()), Err(e)]).unwrap_err();
    assert_eq!(err.code, 0x80041003);
    assert_eq!(err.target, wide("node-b"));
    assert_eq!(
        *err.partial.unwrap(),
        ManagementResult::Objects(ObjectSet {
            objects: vec![r],
            schema: None
        })
    );
}
#[test]
fn string_datetime_reference_and_float_bits_preserved() {
    assert_eq!(
        convert_value(
            AutomationValue::String(wide("20261015000000.000000+000")),
            101
        )
        .unwrap(),
        CimValue::DateTime(wide("20261015000000.000000+000"))
    );
    assert_eq!(
        convert_value(AutomationValue::String(wide("X.Id=1")), 102).unwrap(),
        CimValue::Reference(wide("X.Id=1"))
    );
    let bits = f64::NAN.to_bits();
    assert_eq!(
        convert_value(AutomationValue::Real64(bits), 5).unwrap(),
        CimValue::Real64(bits)
    );
}
#[cfg(windows)]
mod live {
    use super::*;
    use wmic_compat::{
        alias::{AliasStore, ResolvedCommand},
        backend::{Management, windows::WindowsManagement},
        parse::parse_command,
        session::SessionSettings,
    };
    fn request(text: &str) -> wmic_compat::backend::ResolvedRequest {
        let ResolvedCommand::Management(r) = AliasStore::stock()
            .unwrap()
            .resolve(
                &parse_command(&wide(text), EntryMode::Interactive).unwrap(),
                &SessionSettings::default(),
            )
            .unwrap()
        else {
            panic!()
        };
        *r
    }
    #[test]
    #[ignore = "explicit selected Windows reference guest only"]
    fn local_os_query() {
        let result = WindowsManagement::new()
            .unwrap()
            .execute(&request("os get Caption,Version"))
            .unwrap();
        let ManagementResult::Objects(s) = result else {
            panic!()
        };
        assert_eq!(s.objects.len(), 1);
        assert!(
            s.objects[0]
                .properties
                .iter()
                .any(|p| p.name == wide("Caption")
                    && matches!(&p.value,CimValue::String(v) if !v.0.is_empty()))
        );
    }
    #[test]
    #[ignore = "explicit selected Windows reference guest only"]
    fn invalid_class_preserves_hresult() {
        let e = WindowsManagement::new()
            .unwrap()
            .execute(&request("path WmicResearchClassDoesNotExist get Name"))
            .unwrap_err();
        assert_eq!(e.code, 0x80041002);
    }
    #[test]
    #[ignore = "explicit selected Windows reference guest only"]
    fn local_schema_retains_static_method_and_parameter_id() {
        let result = WindowsManagement::new()
            .unwrap()
            .execute(&request("class Win32_Process get"))
            .unwrap();
        let ManagementResult::Schema(s) = result else {
            panic!()
        };
        let create = s
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_case("Create"))
            .unwrap();
        assert!(create.is_static);
        let command = create
            .input
            .iter()
            .find(|p| p.name.eq_ignore_case("CommandLine"))
            .unwrap();
        assert_eq!(command.id, Some(0));
        assert_eq!(command.cim_type, 8);
    }
}
#[test]
fn uint32_retains_unsigned_bits_from_automation_i4() {
    assert_eq!(
        convert_value(AutomationValue::Signed(-1), 19).unwrap(),
        CimValue::Uint32(u32::MAX)
    );
}

#[test]
fn association_collection_keeps_previous_and_current_partial_groups() {
    use wmic_compat::backend::collect_associations;
    let source = |path: &str| Record {
        path: Some(path.into()),
        ..Default::default()
    };
    let error = || ManagementError {
        code: 0x80041003,
        target: "node-b".into(),
        message: "Access denied".into(),
        operation: "enumerate".into(),
        extended: vec![],
        partial: Some(Box::new(ManagementResult::Objects(ObjectSet {
            objects: vec![source("associated-2")],
            schema: None,
        }))),
    };
    let sources = ObjectSet {
        objects: vec![
            source("source-1"),
            source("source-2"),
            source("must-not-run"),
        ],
        schema: None,
    };
    let mut count = 0;
    let e = collect_associations(Ok(sources), |_| {
        count += 1;
        if count == 1 {
            Ok(ObjectSet {
                objects: vec![source("associated-1")],
                schema: None,
            })
        } else {
            Err(error())
        }
    })
    .unwrap_err();
    assert_eq!(count, 2);
    assert_eq!(e.code, 0x80041003);
    assert_eq!(e.target, WideText::from("node-b"));
    let ManagementResult::Associations(groups) = *e.partial.unwrap() else {
        panic!("wrong partial type")
    };
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].source.path, Some("source-1".into()));
    assert_eq!(groups[1].associated[0].path, Some("associated-2".into()));
    let mut source_error = error();
    source_error.partial = Some(Box::new(ManagementResult::Objects(ObjectSet {
        objects: vec![source("retrieved-source")],
        schema: None,
    })));
    let e = collect_associations(Err(source_error), |_| {
        Ok(ObjectSet {
            objects: vec![source("retrieved-associated")],
            schema: None,
        })
    })
    .unwrap_err();
    let ManagementResult::Associations(groups) = *e.partial.unwrap() else {
        panic!("wrong partial type")
    };
    assert_eq!(groups[0].source.path, Some("retrieved-source".into()));
    assert_eq!(
        groups[0].associated[0].path,
        Some("retrieved-associated".into())
    );
}

#[test]
fn cancellation_stops_a_batch_before_the_next_target() {
    use std::cell::Cell;
    use wmic_compat::backend::cancellable_targets;
    let cancelled = Cell::new(false);
    let mut completed = vec![];
    for target in cancellable_targets([1, 2, 3], || cancelled.get()) {
        match target {
            Ok(target) => {
                completed.push(target);
                cancelled.set(true)
            }
            Err(code) => {
                assert_eq!(code, 0xC000013A);
                break;
            }
        }
    }
    assert_eq!(completed, vec![1]);
}
