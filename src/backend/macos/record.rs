//! Record construction keeps every schema property present and typed by its schema CIM type.
use super::schema::ClassInfo;
use crate::model::{CimValue, Property, Record};

/// Namespace served by this backend.
pub const NAMESPACE: &str = "root\\cimv2";

/// Conversion of a Rust value to the `CimValue` variant its property type requires.
pub trait IntoCim {
    fn into_cim(self, cim_type: u32) -> CimValue;
}

fn mismatch(cim_type: u32) -> CimValue {
    if cfg!(debug_assertions) {
        panic!("value does not fit CIM type {cim_type}");
    }
    CimValue::Null
}

fn integer(value: i128, cim_type: u32) -> CimValue {
    macro_rules! number {
        ($t:ty, $variant:ident) => {
            <$t>::try_from(value)
                .map(CimValue::$variant)
                .unwrap_or(CimValue::Null)
        };
    }
    match cim_type {
        2 => number!(i16, Sint16),
        3 => number!(i32, Sint32),
        16 => number!(i8, Sint8),
        17 => number!(u8, Uint8),
        18 => number!(u16, Uint16),
        19 => number!(u32, Uint32),
        20 => number!(i64, Sint64),
        21 => number!(u64, Uint64),
        103 => number!(u16, Char16),
        _ => mismatch(cim_type),
    }
}

fn text(value: &str, cim_type: u32) -> CimValue {
    match cim_type {
        8 => CimValue::String(value.into()),
        101 => CimValue::DateTime(value.into()),
        102 => CimValue::Reference(value.into()),
        _ => mismatch(cim_type),
    }
}

macro_rules! integers {
    ($($t:ty),*) => {$(
        impl IntoCim for $t {
            fn into_cim(self, cim_type: u32) -> CimValue {
                integer(self as i128, cim_type)
            }
        }
    )*};
}
integers!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);

impl IntoCim for bool {
    fn into_cim(self, cim_type: u32) -> CimValue {
        if cim_type == 11 {
            CimValue::Boolean(self)
        } else {
            mismatch(cim_type)
        }
    }
}
impl IntoCim for &str {
    fn into_cim(self, cim_type: u32) -> CimValue {
        text(self, cim_type)
    }
}
impl IntoCim for String {
    fn into_cim(self, cim_type: u32) -> CimValue {
        text(&self, cim_type)
    }
}
impl IntoCim for &String {
    fn into_cim(self, cim_type: u32) -> CimValue {
        text(self, cim_type)
    }
}
impl IntoCim for CimValue {
    fn into_cim(self, _: u32) -> CimValue {
        self
    }
}
impl<T: IntoCim> IntoCim for Option<T> {
    fn into_cim(self, cim_type: u32) -> CimValue {
        self.map_or(CimValue::Null, |v| v.into_cim(cim_type))
    }
}
impl<T: IntoCim> IntoCim for Vec<T> {
    fn into_cim(self, cim_type: u32) -> CimValue {
        if cim_type & 0x2000 == 0 {
            return mismatch(cim_type);
        }
        CimValue::Array(
            self.into_iter()
                .map(|v| v.into_cim(cim_type & !0x2000))
                .collect(),
        )
    }
}
impl<T: IntoCim, const N: usize> IntoCim for [T; N] {
    fn into_cim(self, cim_type: u32) -> CimValue {
        Vec::from(self).into_cim(cim_type)
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Text of a key value inside an object path, or `None` for values that cannot be keys.
pub fn key_literal(value: &CimValue) -> Option<String> {
    Some(match value {
        CimValue::String(s) | CimValue::Reference(s) | CimValue::DateTime(s) => {
            quoted(&s.to_string_lossy())
        }
        CimValue::Boolean(b) => (if *b { "TRUE" } else { "FALSE" }).into(),
        CimValue::Null | CimValue::Empty => quoted(""),
        other => key_number(other)?.to_string(),
    })
}

/// Integer value of a numeric `CimValue`.
pub fn key_number(value: &CimValue) -> Option<i128> {
    Some(match value {
        CimValue::Sint8(v) => *v as i128,
        CimValue::Uint8(v) => *v as i128,
        CimValue::Sint16(v) => *v as i128,
        CimValue::Uint16(v) | CimValue::Char16(v) => *v as i128,
        CimValue::Sint32(v) => *v as i128,
        CimValue::Uint32(v) => *v as i128,
        CimValue::Sint64(v) => *v as i128,
        CimValue::Uint64(v) => *v as i128,
        _ => return None,
    })
}

/// Builds one record of a class. Properties that are never set stay `Null`.
pub struct RecordBuilder<'a> {
    info: &'static ClassInfo,
    server: &'a str,
    values: Vec<CimValue>,
}

impl<'a> RecordBuilder<'a> {
    pub fn new(info: &'static ClassInfo, server: &'a str) -> Self {
        Self {
            info,
            server,
            values: vec![CimValue::Null; info.properties.len()],
        }
    }

    /// Sets a property by schema name; an unknown name is a provider bug.
    pub fn set(mut self, name: &str, value: impl IntoCim) -> Self {
        match self.info.property(name) {
            Some((index, property)) => self.values[index] = value.into_cim(property.cim_type),
            None if cfg!(debug_assertions) => {
                panic!("{} has no property {name}", self.info.name)
            }
            None => {}
        }
        self
    }

    pub fn build(self) -> Record {
        let info = self.info;
        let relpath = {
            let keys = info.keys();
            if info.singleton || keys.is_empty() {
                format!("{}=@", info.name)
            } else {
                let parts: Vec<String> = keys
                    .iter()
                    .map(|i| {
                        let literal = key_literal(&self.values[*i]).unwrap_or_else(|| quoted(""));
                        format!("{}={literal}", info.properties[*i].name)
                    })
                    .collect();
                format!("{}.{}", info.name, parts.join(","))
            }
        };
        let path = format!("\\\\{}\\{NAMESPACE}:{relpath}", self.server);
        let system = |name: &str, value: CimValue, cim_type: u32| Property {
            name: name.into(),
            value,
            cim_type,
            system: true,
            qualifiers: vec![],
        };
        let text = |s: &str| CimValue::String(s.into());
        let mut properties = vec![
            system("__PATH", text(&path), 8),
            system("__NAMESPACE", text(NAMESPACE), 8),
            system("__SERVER", text(self.server), 8),
            system(
                "__DERIVATION",
                CimValue::Array(info.derivation.iter().map(|d| text(d)).collect()),
                8 | 0x2000,
            ),
            system(
                "__PROPERTY_COUNT",
                CimValue::Sint32(info.properties.len() as i32),
                3,
            ),
            system("__RELPATH", text(&relpath), 8),
            system("__DYNASTY", text(&info.dynasty), 8),
            system(
                "__SUPERCLASS",
                info.superclass.as_deref().map_or(CimValue::Null, text),
                8,
            ),
            system("__CLASS", text(&info.name), 8),
            system("__GENUS", CimValue::Sint32(2), 3),
        ];
        for (property, value) in info.properties.iter().zip(self.values) {
            properties.push(Property {
                name: property.name.as_str().into(),
                value,
                cim_type: property.cim_type,
                system: false,
                qualifiers: vec![],
            });
        }
        Record {
            result_node: None,
            wmi_xml: None,
            class: info.name.as_str().into(),
            path: Some(path.into()),
            server: Some(self.server.into()),
            properties,
        }
    }
}
