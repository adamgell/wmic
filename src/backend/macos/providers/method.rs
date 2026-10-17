//! Argument access and result shaping shared by providers that implement methods.
use crate::{
    backend::macos::{
        provider::{MethodCall, MethodOutput},
        record::key_number,
    },
    model::CimValue,
};

pub fn argument<'a>(call: &'a MethodCall<'_>, name: &str) -> Option<&'a CimValue> {
    call.input
        .iter()
        .find(|p| p.name.eq_ignore_case(name))
        .map(|p| &p.value)
}

/// String argument; `None` when absent or null.
pub fn text_argument(call: &MethodCall<'_>, name: &str) -> Option<String> {
    match argument(call, name)? {
        CimValue::String(value) => Some(value.to_string_lossy()),
        _ => None,
    }
}

/// Integer argument of any width; `None` when absent or null.
pub fn integer_argument(call: &MethodCall<'_>, name: &str) -> Option<i128> {
    key_number(argument(call, name)?)
}

/// Result that carries only a `ReturnValue`.
pub fn code(value: u32) -> MethodOutput {
    MethodOutput {
        return_value: Some(CimValue::Uint32(value)),
        out_parameters: vec![],
    }
}
