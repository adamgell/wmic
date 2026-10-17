//! Schema-driven input binding. Lexical quotes are retained until the CIM type is known.
use crate::model::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub code: u32,
    pub message: WideText,
}
/// The error for a method the class does not declare: an alias target reports the verb, a path or
/// class target reports the class method.
pub fn undeclared_method(requested: &WideText, alias_target: bool) -> InputError {
    let (code, text) = if alias_target {
        (0xABFD, " - Invalid alias verb.")
    } else {
        (0xABFC, " - Invalid class method.")
    };
    let mut message = requested.clone();
    message.0.extend(text.encode_utf16());
    InputError { code, message }
}
fn invalid() -> InputError {
    InputError {
        code: 0x80041005,
        message: "Type mismatch".into(),
    }
}
fn unquote(v: &WideText) -> WideText {
    if v.0.len() >= 2 && matches!(v.0[0], 34 | 39) && v.0.last() == v.0.first() {
        WideText(v.0[1..v.0.len() - 1].to_vec())
    } else {
        v.clone()
    }
}
/// Commas within quoted values or nested arrays/objects do not split arguments.
pub fn split_values(v: &WideText) -> Vec<WideText> {
    if v.0.is_empty() {
        return vec![];
    }
    let (mut depth, mut quote, mut escape, mut start) = (0usize, None, false, 0);
    let mut out = vec![];
    for (i, &u) in v.0.iter().enumerate() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if u == 92 {
                escape = true;
            } else if u == q {
                quote = None;
            }
        } else if matches!(u, 34 | 39) {
            quote = Some(u)
        } else if matches!(u, 40 | 91 | 123) {
            depth += 1
        } else if matches!(u, 41 | 93 | 125) {
            depth = depth.saturating_sub(1)
        } else if u == 44 && depth == 0 {
            out.push(WideText(v.0[start..i].to_vec()).trim_ascii());
            start = i + 1;
        }
    }
    out.push(WideText(v.0[start..].to_vec()).trim_ascii());
    out
}
fn integer(v: &str) -> Result<i128, InputError> {
    let (negative, v) = v.strip_prefix('-').map_or((false, v), |v| (true, v));
    let v = v.strip_prefix('+').unwrap_or(v);
    let n = if v.starts_with("0x") || v.starts_with("0X") {
        i128::from_str_radix(&v[2..], 16)
    } else {
        v.parse()
    }
    .map_err(|_| invalid())?;
    Ok(if negative { -n } else { n })
}
pub fn convert_input(raw: &WideText, ty: u32) -> Result<CimValue, InputError> {
    let raw = raw.trim_ascii();
    if raw.eq_ignore_case("NULL") {
        return Ok(CimValue::Null);
    }
    if ty & 0x2000 != 0 {
        if raw.0.first() != Some(&123) || raw.0.last() != Some(&125) {
            return Err(invalid());
        }
        let body = WideText(raw.0[1..raw.0.len() - 1].to_vec());
        return split_values(&body)
            .iter()
            .map(|s| convert_input(s, ty & !0x2000))
            .collect::<Result<Vec<_>, _>>()
            .map(CimValue::Array);
    }
    let value = unquote(&raw);
    let text = String::from_utf16(&value.0);
    Ok(match ty & 0xfff {
        8 => CimValue::String(value),
        101 => CimValue::DateTime(value),
        102 => CimValue::Reference(value),
        103 if value.0.len() == 1 => CimValue::Char16(value.0[0]),
        11 if value.eq_ignore_case("TRUE") => CimValue::Boolean(true),
        11 if value.eq_ignore_case("FALSE") => CimValue::Boolean(false),
        4 => CimValue::Real32(
            text.map_err(|_| invalid())?
                .parse::<f32>()
                .map_err(|_| invalid())?
                .to_bits(),
        ),
        5 => CimValue::Real64(
            text.map_err(|_| invalid())?
                .parse::<f64>()
                .map_err(|_| invalid())?
                .to_bits(),
        ),
        t => {
            let n = integer(&text.map_err(|_| invalid())?)?;
            macro_rules! number {
                ($ty:ty,$variant:ident) => {
                    CimValue::$variant(<$ty>::try_from(n).map_err(|_| invalid())?)
                };
            }
            match t {
                2 => number!(i16, Sint16),
                3 => number!(i32, Sint32),
                16 => number!(i8, Sint8),
                17 => number!(u8, Uint8),
                18 => number!(u16, Uint16),
                19 => number!(u32, Uint32),
                20 => number!(i64, Sint64),
                21 => number!(u64, Uint64),
                _ => return Err(invalid()),
            }
        }
    })
}
pub fn bind_method(schema: &MethodSchema, args: &[WideText]) -> Result<Vec<Property>, InputError> {
    bind_parameters(schema, args, true)
}
pub(super) fn bind_parameters(
    schema: &MethodSchema,
    args: &[WideText],
    named_null_literal: bool,
) -> Result<Vec<Property>, InputError> {
    if schema.input.is_empty() && !args.is_empty() {
        if let Some(at) = args[0].0.iter().position(|u| *u == 61) {
            return Err(not_input(WideText(args[0].0[..at].to_vec()).trim_ascii()));
        }
        return Err(InputError {
            code: 0xADE6,
            message: "Verb Or Method has no input parameters.".into(),
        });
    }
    let mut parameters: Vec<_> = schema.input.iter().collect();
    parameters.sort_by_key(|p| p.id.unwrap_or(u32::MAX));
    let mut out: Vec<Property> = vec![];
    for (i, arg) in args.iter().enumerate() {
        let assignment = arg
            .0
            .iter()
            .position(|u| *u == 61)
            .filter(|at| !arg.0[..*at].iter().any(|u| matches!(u, 34 | 39 | 123)));
        let (p, expr) = if let Some(at) = assignment {
            let name = WideText(arg.0[..at].to_vec()).trim_ascii();
            let p = parameters
                .iter()
                .find(|p| name.eq_ignore_case(&p.name.to_string_lossy()))
                .ok_or_else(|| not_input(name))?;
            (*p, WideText(arg.0[at + 1..].to_vec()).trim_ascii())
        } else {
            (*parameters.get(i).ok_or_else(invalid)?, arg.clone())
        };
        if out.iter().any(|old| old.name == p.name) {
            return Err(invalid());
        }
        let expr = if named_null_literal && assignment.is_some() && expr.eq_ignore_case("NULL") {
            let mut quoted: WideText = "\"".into();
            quoted.0.extend(&expr.0);
            quoted.0.push(34);
            quoted
        } else {
            expr
        };
        out.push(Property {
            name: p.name.clone(),
            value: convert_input(&expr, p.cim_type)?,
            cim_type: p.cim_type,
            system: false,
            qualifiers: vec![],
        });
    }
    Ok(out)
}
fn not_input(name: WideText) -> InputError {
    let mut message = name;
    message.0.extend(" - Not a input parameter.".encode_utf16());
    InputError {
        code: 0xADE5,
        message,
    }
}
