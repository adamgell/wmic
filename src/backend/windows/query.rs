use super::*;
use crate::backend::{AutomationValue, build_query, collect_records, convert_value};
use windows::{
    Win32::System::{Ole::*, Variant::*, Wmi::*},
    core::*,
};
/// IWbemClassObject::Get and SAFEARRAY elements transfer VARIANT ownership to us.
#[derive(Default)]
pub(super) struct OwnedVariant(pub VARIANT);
impl Drop for OwnedVariant {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}
fn conversion_error() -> Error {
    Error::from_hresult(HRESULT(0x80041005u32 as i32))
}
pub(super) unsafe fn automation(v: &VARIANT, depth: usize) -> Result<AutomationValue> {
    unsafe {
        if depth > 32 {
            return Err(conversion_error());
        }
        let data = &v.Anonymous.Anonymous;
        let vt = data.vt.0;
        let union = &data.Anonymous;
        if vt & 0x2000 != 0 {
            let array = union.parray;
            if array.is_null() {
                return Ok(AutomationValue::Array(vec![]));
            }
            if SafeArrayGetDim(array) != 1 {
                return Err(conversion_error());
            }
            let lo = SafeArrayGetLBound(array, 1)?;
            let hi = SafeArrayGetUBound(array, 1)?;
            let mut items = vec![];
            for i in lo..=hi {
                let mut element = OwnedVariant::default();
                let base = vt & !0x2000;
                if base == 12 {
                    SafeArrayGetElement(array, &i, (&mut element.0 as *mut VARIANT).cast())?;
                } else {
                    (*element.0.Anonymous.Anonymous).vt = VARENUM(base);
                    SafeArrayGetElement(
                        array,
                        &i,
                        (&mut (*element.0.Anonymous.Anonymous).Anonymous as *mut VARIANT_0_0_0)
                            .cast(),
                    )?;
                }
                items.push(automation(&element.0, depth + 1)?);
            }
            return Ok(AutomationValue::Array(items));
        }
        Ok(match vt {
            0 => AutomationValue::Empty,
            1 => AutomationValue::Null,
            2 => AutomationValue::Signed(union.iVal as i64),
            3 | 22 => AutomationValue::Signed(union.lVal as i64),
            16 => AutomationValue::Signed(union.cVal as i64),
            17 => AutomationValue::Unsigned(union.bVal as u64),
            18 => AutomationValue::Unsigned(union.uiVal as u64),
            19 | 23 => AutomationValue::Unsigned(union.ulVal as u64),
            20 => AutomationValue::Signed(union.llVal),
            21 => AutomationValue::Unsigned(union.ullVal),
            4 => AutomationValue::Real32(union.fltVal.to_bits()),
            5 => AutomationValue::Real64(union.dblVal.to_bits()),
            8 => AutomationValue::String(wide(&union.bstrVal)),
            11 => AutomationValue::Boolean(union.boolVal.0 != 0),
            13 => {
                let object = union
                    .punkVal
                    .as_ref()
                    .ok_or_else(conversion_error)?
                    .cast::<IWbemClassObject>()?;
                AutomationValue::Object(Box::new(record_depth(&object, depth + 1)?))
            }
            9 => {
                let object = union
                    .pdispVal
                    .as_ref()
                    .ok_or_else(conversion_error)?
                    .cast::<IWbemClassObject>()?;
                AutomationValue::Object(Box::new(record_depth(&object, depth + 1)?))
            }
            _ => return Err(conversion_error()),
        })
    }
}
pub fn record(object: &IWbemClassObject) -> Result<Record> {
    record_depth(object, 0)
}
fn record_depth(object: &IWbemClassObject, depth: usize) -> Result<Record> {
    unsafe {
        if depth > 32 {
            return Err(conversion_error());
        }
        object.BeginEnumeration(0)?;
        let mut props = vec![];
        let result = (|| {
            loop {
                let mut name = BSTR::new();
                let mut v = OwnedVariant::default();
                let mut ty = 0;
                match object.Next(0, &mut name, &mut v.0, &mut ty, std::ptr::null_mut()) {
                    Ok(()) if name.is_empty() => break,
                    Ok(()) => {}
                    Err(e) if e.code().0 as u32 == 0x40005 => break,
                    Err(e) => return Err(e),
                }
                let name = wide(&name);
                let value = convert_value(automation(&v.0, depth)?, ty as u32)
                    .map_err(|_| conversion_error())?;
                props.push(Property {
                    system: name.0.starts_with(&[95, 95]),
                    name,
                    value,
                    cim_type: ty as u32,
                    qualifiers: vec![],
                });
            }
            Ok(())
        })();
        let end = object.EndEnumeration();
        result?;
        end?;
        let get = |name: &str| {
            props
                .iter()
                .find(|p| p.name.eq_ascii_ignore_case(name))
                .and_then(|p| {
                    if let CimValue::String(s) | CimValue::Reference(s) = &p.value {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
        };
        Ok(Record {
            class: get("__CLASS").unwrap_or_default(),
            path: get("__PATH"),
            server: get("__SERVER"),
            properties: props,
        })
    }
}
pub fn query(
    svc: &IWbemServices,
    r: &ResolvedRequest,
) -> std::result::Result<ObjectSet, ManagementError> {
    query_text(svc, r, &build_query(r))
}
pub(super) fn query_text(
    svc: &IWbemServices,
    r: &ResolvedRequest,
    query: &WideText,
) -> std::result::Result<ObjectSet, ManagementError> {
    unsafe {
        let en = svc
            .ExecQuery(
                &BSTR::from("WQL"),
                &bstr(query),
                WBEM_GENERIC_FLAG_TYPE(0x30),
                None,
            )
            .map_err(|e| failure(e, r, "query"))?;
        connect::secure(&en, &r.connection).map_err(|e| failure(e, r, "enumeration security"))?;
        let mut items = vec![];
        loop {
            let mut objects = [None];
            let mut returned = 0;
            let hr = en.Next(-1, &mut objects, &mut returned);
            if hr.is_err() {
                items.push(Err(failure(Error::from_hresult(hr), r, "enumerate")));
                break;
            }
            if returned == 0 {
                break;
            }
            if let Some(object) = objects[0].take() {
                match record(&object) {
                    Ok(o) => items.push(Ok(o)),
                    Err(e) => {
                        items.push(Err(failure(e, r, "convert")));
                        break;
                    }
                }
            }
        }
        collect_records(items)
    }
}
/// Qualifier sets carry Automation types without a separate CIMTYPE output.
pub(super) fn qualifier_value(v: &VARIANT) -> Result<CimValue> {
    unsafe {
        let vt = v.Anonymous.Anonymous.vt.0;
        let base = vt & !0x2000;
        let ty = match base {
            0 | 1 => 8,
            2 => 2,
            3 | 22 => 3,
            4 => 4,
            5 => 5,
            8 => 8,
            9 | 13 => 13,
            11 => 11,
            16 => 16,
            17 => 17,
            18 => 18,
            19 | 23 => 19,
            20 => 20,
            21 => 21,
            _ => return Err(conversion_error()),
        };
        convert_value(automation(v, 0)?, ty | (vt as u32 & 0x2000)).map_err(|_| conversion_error())
    }
}

pub(super) fn associations(
    svc: &IWbemServices,
    r: &ResolvedRequest,
) -> std::result::Result<Vec<AssociationGroup>, ManagementError> {
    let sources = if r.class_mode {
        let object = get_object(svc, &r.class_name).map_err(|e| failure(e, r, "class"))?;
        ObjectSet {
            objects: vec![record(&object).map_err(|e| failure(e, r, "convert"))?],
            schema: None,
        }
    } else {
        query(svc, r)?
    };
    let mut groups = vec![];
    for source in sources.objects {
        let path = source.path.as_ref().ok_or_else(|| {
            failure(
                Error::from_hresult(HRESULT(0x80041002u32 as i32)),
                r,
                "association source",
            )
        })?;
        let mut query = crate::backend::build_association_query(
            path,
            &r.association.clone().unwrap_or_default(),
        );
        if r.class_mode {
            query.0.extend(
                if r.association.as_ref().is_some_and(|f| {
                    f.assoc_class.is_some() || f.result_class.is_some() || f.result_role.is_some()
                }) {
                    " SchemaOnly"
                } else {
                    " WHERE SchemaOnly"
                }
                .encode_utf16(),
            );
        }
        match query_text(svc, r, &query) {
            Ok(set) => groups.push(AssociationGroup {
                source,
                associated: set.objects,
            }),
            Err(mut e) => {
                if !groups.is_empty() {
                    e.partial = Some(Box::new(ManagementResult::Associations(groups)));
                }
                return Err(e);
            }
        }
    }
    Ok(groups)
}
