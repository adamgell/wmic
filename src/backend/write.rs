use super::methods::{InputError, bind_parameters};
use crate::model::*;
pub fn bind_properties(
    schema: &ClassSchema,
    args: &[WideText],
) -> Result<Vec<Property>, InputError> {
    if args.is_empty() || args.iter().any(|a| !a.0.contains(&61)) {
        return Err(InputError {
            code: 0xABFD,
            message: "Invalid alias verb.".into(),
        });
    }
    let method = MethodSchema {
        name: WideText::default(),
        input: schema
            .properties
            .iter()
            .map(|p| ParameterSchema {
                cim_type_name: None,
                name: p.name.clone(),
                cim_type: p.cim_type,
                id: None,
                optional: true,
                default_value: None,
            })
            .collect(),
        output: vec![],
        is_static: false,
        qualifiers: vec![],
    };
    bind_parameters(&method, args, false)
}

pub fn interactive(request: &super::ResolvedRequest) -> bool {
    let mut enabled = false;
    for m in &request.modifiers {
        if m.name.eq_ignore_case("nointeractive") {
            enabled = false;
        }
        if m.name.eq_ignore_case("interactive") {
            enabled = m.value == SwitchValue::Missing
                || matches!(&m.value,SwitchValue::Value(v) if v.eq_ignore_case("on"));
        }
    }
    enabled
}
