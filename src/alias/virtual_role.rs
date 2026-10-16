//! Read-only introspection of bundled aliases when the default CLI namespace is absent.
use super::*;
fn property(name: &str, value: CimValue, ty: u32) -> Property {
    Property {
        name: name.into(),
        value,
        cim_type: ty,
        system: false,
        qualifiers: vec![],
    }
}
fn child_class(parent: &str, name: &str) -> Option<(&'static str, bool)> {
    match (parent, name) {
        ("MSFT_CliAlias", "Connection") => Some(("MSFT_CliConnection", false)),
        ("MSFT_CliAlias", "Formats") => Some(("MSFT_CliFormat", true)),
        ("MSFT_CliAlias", "Verbs") => Some(("MSFT_CliVerb", true)),
        (_, "Qualifiers") => Some(("MSFT_CliQualifier", true)),
        ("MSFT_CliFormat", "Properties") => Some(("MSFT_CliProperty", true)),
        ("MSFT_CliVerb", "Parameters") => Some(("MSFT_CliParam", true)),
        _ => None,
    }
}
fn object(class: &str, json: &serde_json::Value) -> Record {
    let mut properties = vec![];
    for (name, value) in json.as_object().expect("embedded alias object") {
        if name == "LocalizedDescription" {
            continue;
        }
        let (ty, converted) = if let Some((child, array)) = child_class(class, name) {
            let ty = 13 | if array { 0x2000 } else { 0 };
            let converted = if value.is_null() {
                CimValue::Null
            } else if array {
                CimValue::Array(
                    value
                        .as_array()
                        .expect("embedded object array")
                        .iter()
                        .map(|v| CimValue::Object(Box::new(object(child, v))))
                        .collect(),
                )
            } else {
                CimValue::Object(Box::new(object(child, value)))
            };
            (ty, converted)
        } else {
            let ty = if name == "QualifierValue" {
                8 | 0x2000
            } else if name == "Optional" {
                11
            } else if name == "VerbType" {
                19
            } else {
                8
            };
            let converted = match value {
                serde_json::Value::Null => CimValue::Null,
                serde_json::Value::String(s) => CimValue::String(s.as_str().into()),
                serde_json::Value::Bool(v) => CimValue::Boolean(*v),
                serde_json::Value::Number(v) => {
                    CimValue::Uint32(v.as_u64().expect("embedded enum") as u32)
                }
                serde_json::Value::Array(v) => CimValue::Array(
                    v.iter()
                        .map(|v| {
                            CimValue::String(v.as_str().expect("embedded qualifier text").into())
                        })
                        .collect(),
                ),
                _ => unreachable!("embedded alias field"),
            };
            (ty, converted)
        };
        properties.push(property(name, converted, ty));
    }
    if class != "MSFT_CliAlias" {
        let count = properties.len() as i32;
        let mut system = vec![];
        for name in crate::render::system_property_order() {
            let (value, ty) = match name {
                "__CLASS" | "__DYNASTY" => (CimValue::String(class.into()), 8),
                "__GENUS" => (CimValue::Sint32(2), 3),
                "__PROPERTY_COUNT" => (CimValue::Sint32(count), 3),
                "__DERIVATION" => (CimValue::Array(vec![]), 8 | 0x2000),
                _ => (CimValue::Null, 8),
            };
            let mut p = property(name, value, ty);
            p.system = true;
            system.push(p);
        }
        system.extend(properties);
        properties = system;
    }
    Record {
        class: class.into(),
        properties,
        ..Default::default()
    }
}
fn invalid(r: &ResolvedRequest) -> ManagementError {
    ManagementError {
        code: 0x80041017,
        target: r.class_name.clone(),
        message: "Invalid query".into(),
        operation: "query".into(),
        extended: vec![],
        partial: None,
    }
}
pub(super) fn query(
    r: &ResolvedRequest,
    node: &WideText,
) -> Result<ManagementResult, ManagementError> {
    if !matches!(r.verb, Verb::Get | Verb::List) || r.class_mode {
        return Err(ManagementError {
            code: 0x8004100C,
            target: r.class_name.clone(),
            message: "Not supported".into(),
            operation: "query".into(),
            extended: vec![],
            partial: None,
        });
    }
    let mut rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../assets/aliases.json"))
            .expect("embedded aliases checked");
    let order: Vec<String> = serde_json::from_str(include_str!("../../assets/alias-order.json"))
        .expect("embedded alias order");
    rows.sort_by_key(|row| {
        order
            .iter()
            .position(|name| row["FriendlyName"].as_str() == Some(name))
            .unwrap_or(usize::MAX)
    });
    let predicate = r
        .selector
        .as_ref()
        .map(|text| super::predicate::Predicate::parse(text).map_err(|_| invalid(r)))
        .transpose()?;
    let mut objects = vec![];
    for json in rows {
        let mut record = object("MSFT_CliAlias", &json);
        record.server = Some(node.clone());
        record.result_node = Some(node.clone());
        if let Some(predicate) = &predicate
            && !predicate.matches(&record).map_err(|_| invalid(r))?
        {
            continue;
        }
        let friendly = record
            .properties
            .iter()
            .find(|p| p.name.eq_ignore_case("FriendlyName"))
            .and_then(|p| {
                if let CimValue::String(s) = &p.value {
                    Some(s.to_string_lossy())
                } else {
                    None
                }
            })
            .unwrap();
        record.path =
            Some(format!("\\\\{node}\\root\\cli:MSFT_CliAlias.FriendlyName=\"{friendly}\"").into());
        let columns = if r.verb == Verb::List {
            r.display
                .iter()
                .map(|(_, source)| source)
                .collect::<Vec<_>>()
        } else {
            r.properties.iter().collect()
        };
        if !columns.is_empty() {
            for name in &columns {
                if !record
                    .properties
                    .iter()
                    .any(|p| p.name.eq_ordinal_ignore_case(name))
                {
                    return Err(invalid(r));
                }
            }
            record
                .properties
                .retain(|p| columns.iter().any(|n| p.name.eq_ordinal_ignore_case(n)));
        }
        objects.push(record)
    }
    Ok(ManagementResult::Objects(ObjectSet {
        objects,
        schema: None,
    }))
}

#[cfg(all(test, windows))]
mod native_tests {
    use super::*;
    use crate::{
        backend::Management,
        model::EntryMode,
        render::{DestinationProfile, OutputError, encode_output},
        session::{OutputSink, OutputStream, Session},
    };
    struct Absent;
    impl Management for Absent {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            panic!("virtual metadata must not require the missing namespace")
        }
    }
    #[derive(Default)]
    struct Sink {
        out: WideText,
        err: WideText,
    }
    impl OutputSink for Sink {
        fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
            if s == OutputStream::Stdout {
                self.out.0.extend(&t.0)
            } else {
                self.err.0.extend(&t.0)
            }
            Ok(())
        }
    }
    #[test]
    #[ignore = "requires Windows MSXML6 and recorded local reference profile"]
    fn bundled_alias_predicates_match_original_scalar_queries() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fallback-cases.json")).unwrap();
        let root = r"\\Mac\Home\Documents\ChatGPT\wmic-dropinrepalcement\tests\fixtures\fallback-reference";
        let node = std::env::var("COMPUTERNAME").unwrap();
        for case in manifest["cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let mut sink = Sink::default();
            let outcome = Session::new(node.as_str().into()).unwrap().execute(
                &case["arguments"].as_str().unwrap().into(),
                EntryMode::Interactive,
                &mut Absent,
                &mut sink,
            );
            let metadata: serde_json::Value =
                serde_json::from_slice(&std::fs::read(format!(r"{root}\capture.json")).unwrap())
                    .unwrap();
            let expected = metadata["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap();
            assert_eq!(
                outcome.exit_status,
                expected["exit_status"].as_u64().unwrap() as u32,
                "{id}"
            );
            for (stream, text) in [("stdout", &sink.out), ("stderr", &sink.err)] {
                let actual = encode_output(text, &DestinationProfile::pipe(437)).unwrap();
                let expected = std::fs::read(format!(r"{root}\{id}.{stream}.bin")).unwrap();
                assert_eq!(actual, expected, "{id}: {stream}");
            }
        }
    }
}
