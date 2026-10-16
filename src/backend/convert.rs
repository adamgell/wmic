use crate::model::*;
#[derive(Clone, Debug)]
pub enum AutomationValue {
    Empty,
    Null,
    String(WideText),
    Signed(i64),
    Unsigned(u64),
    Real32(u32),
    Real64(u64),
    Boolean(bool),
    Object(Box<Record>),
    Array(Vec<AutomationValue>),
}
#[derive(Debug)]
pub struct ConversionError;
impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WMI value does not match its CIM type")
    }
}
impl std::error::Error for ConversionError {}
pub fn convert_value(v: AutomationValue, ty: u32) -> Result<CimValue, ConversionError> {
    if let AutomationValue::Array(values) = v {
        return values
            .into_iter()
            .map(|v| convert_value(v, ty & !0x2000))
            .collect::<Result<Vec<_>, _>>()
            .map(CimValue::Array);
    }
    match v {
        AutomationValue::Empty => return Ok(CimValue::Empty),
        AutomationValue::Null => return Ok(CimValue::Null),
        _ => {}
    }
    macro_rules! integer {
        ($variant:ident,$t:ty) => {{
            let value = match &v {
                AutomationValue::Signed(v) => <$t>::try_from(*v).ok(),
                AutomationValue::Unsigned(v) => <$t>::try_from(*v).ok(),
                AutomationValue::String(s) => s.to_string_lossy().parse::<$t>().ok(),
                _ => None,
            };
            value.map(CimValue::$variant).ok_or(ConversionError)
        }};
    }
    match ty & 0xfff {
        2 => integer!(Sint16, i16),
        3 => integer!(Sint32, i32),
        16 => integer!(Sint8, i8),
        17 => integer!(Uint8, u8),
        18 => integer!(Uint16, u16),
        19 => match v {
            AutomationValue::Signed(n) if i32::try_from(n).is_ok() => {
                Ok(CimValue::Uint32(n as i32 as u32))
            }
            _ => integer!(Uint32, u32),
        },
        20 => integer!(Sint64, i64),
        21 => integer!(Uint64, u64),
        103 => integer!(Char16, u16),
        4 => {
            if let AutomationValue::Real32(v) = v {
                Ok(CimValue::Real32(v))
            } else {
                Err(ConversionError)
            }
        }
        5 => {
            if let AutomationValue::Real64(v) = v {
                Ok(CimValue::Real64(v))
            } else {
                Err(ConversionError)
            }
        }
        11 => {
            if let AutomationValue::Boolean(v) = v {
                Ok(CimValue::Boolean(v))
            } else {
                Err(ConversionError)
            }
        }
        8 | 101 | 102 => {
            if let AutomationValue::String(s) = v {
                Ok(match ty & 0xfff {
                    101 => CimValue::DateTime(s),
                    102 => CimValue::Reference(s),
                    _ => CimValue::String(s),
                })
            } else {
                Err(ConversionError)
            }
        }
        13 => {
            if let AutomationValue::Object(r) = v {
                Ok(CimValue::Object(r))
            } else {
                Err(ConversionError)
            }
        }
        _ => Err(ConversionError),
    }
}
pub fn collect_records(
    items: impl IntoIterator<Item = Result<Record, ManagementError>>,
) -> Result<ObjectSet, ManagementError> {
    let mut objects = vec![];
    for item in items {
        match item {
            Ok(r) => objects.push(r),
            Err(mut e) => {
                if !objects.is_empty() {
                    e.partial = Some(Box::new(ManagementResult::Objects(ObjectSet {
                        objects,
                        schema: None,
                    })));
                }
                return Err(e);
            }
        }
    }
    Ok(ObjectSet {
        objects,
        schema: None,
    })
}
