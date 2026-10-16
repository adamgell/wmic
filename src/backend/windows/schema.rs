use super::*;
use windows::core::*;
fn pcw(name: &WideText) -> Vec<u16> {
    name.0.iter().copied().chain([0]).collect()
}
pub(super) fn qualifiers(set: &IWbemQualifierSet) -> Result<Vec<Qualifier>> {
    unsafe {
        set.BeginEnumeration(0)?;
        let mut items = vec![];
        let result: Result<()> = (|| {
            loop {
                let mut name = BSTR::new();
                let mut value = query::OwnedVariant::default();
                set.Next(0, &mut name, &mut value.0, std::ptr::null_mut())?;
                if name.is_empty() {
                    break;
                }
                items.push(Qualifier {
                    name: wide(&name),
                    value: query::qualifier_value(&value.0)?,
                });
            }
            Ok(())
        })();
        let end = set.EndEnumeration();
        result?;
        end?;
        Ok(items)
    }
}
fn property_qualifiers(object: &IWbemClassObject, name: &WideText) -> Result<Vec<Qualifier>> {
    unsafe { qualifiers(&object.GetPropertyQualifierSet(PCWSTR(pcw(name).as_ptr()))?) }
}
fn parameters(object: Option<IWbemClassObject>) -> Result<Vec<ParameterSchema>> {
    let Some(object) = object else {
        return Ok(vec![]);
    };
    let mut result = vec![];
    for p in query::record(&object)?
        .properties
        .into_iter()
        .filter(|p| !p.system)
    {
        let q = property_qualifiers(&object, &p.name)?;
        let id = q
            .iter()
            .find(|q| q.name.eq_ascii_ignore_case("ID"))
            .and_then(|q| match q.value {
                CimValue::Sint32(v) => u32::try_from(v).ok(),
                CimValue::Uint32(v) => Some(v),
                _ => None,
            });
        let optional = q
            .iter()
            .any(|q| q.name.eq_ascii_ignore_case("Optional") && q.value == CimValue::Boolean(true));
        result.push(ParameterSchema {
            name: p.name,
            cim_type: p.cim_type,
            id,
            optional,
            default_value: Some(p.value),
        });
    }
    result.sort_by_key(|p| p.id.unwrap_or(u32::MAX));
    Ok(result)
}
pub fn class_schema(object: &IWbemClassObject) -> Result<ClassSchema> {
    let record = query::record(object)?;
    let mut properties = vec![];
    for p in record.properties.into_iter().filter(|p| !p.system) {
        properties.push(PropertySchema {
            qualifiers: property_qualifiers(object, &p.name)?,
            name: p.name,
            cim_type: p.cim_type,
        });
    }
    let mut methods = vec![];
    unsafe {
        object.BeginMethodEnumeration(0)?;
        let res: Result<()> = (|| {
            loop {
                let mut name = BSTR::new();
                let mut input = None;
                let mut output = None;
                object.NextMethod(0, &mut name, &mut input, &mut output)?;
                if name.is_empty() {
                    break;
                }
                let method_name = wide(&name);
                let q =
                    qualifiers(&object.GetMethodQualifierSet(PCWSTR(pcw(&method_name).as_ptr()))?)?;
                let is_static = q.iter().any(|q| {
                    q.name.eq_ascii_ignore_case("Static") && q.value == CimValue::Boolean(true)
                });
                methods.push(MethodSchema {
                    name: method_name,
                    input: parameters(input)?,
                    output: parameters(output)?,
                    is_static,
                    qualifiers: q,
                });
            }
            Ok(())
        })();
        let end = object.EndMethodEnumeration();
        res?;
        end?;
    }
    Ok(ClassSchema {
        wmi_xml: None,
        name: record.class,
        properties,
        methods,
        qualifiers: unsafe { qualifiers(&object.GetQualifierSet()?)? },
    })
}
