use super::*;
use crate::backend::methods::{InputError, bind_method};
use windows::{
    Win32::System::{
        Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        Ole::*,
        Variant::*,
    },
    core::*,
};
pub(super) fn input_error(e: InputError, r: &ResolvedRequest) -> ManagementError {
    ManagementError {
        code: e.code,
        target: r.class_name.clone(),
        message: e.message,
        operation: "input".into(),
        extended: vec![],
        partial: None,
    }
}
fn invalid() -> Error {
    Error::from_hresult(HRESULT(0x80041005u32 as i32))
}
/// Ownership of every BSTR/SAFEARRAY remains with OwnedVariant until Put copies it.
pub(super) fn variant(
    value: &CimValue,
    ty: u32,
    svc: &IWbemServices,
) -> Result<query::OwnedVariant> {
    let mut out = query::OwnedVariant::default();
    out.0 = match value {
        CimValue::Empty => VARIANT::default(),
        CimValue::Null => {
            unsafe { (*out.0.Anonymous.Anonymous).vt = VT_NULL };
            return Ok(out);
        }
        CimValue::String(s) | CimValue::Reference(s) | CimValue::DateTime(s) => {
            VARIANT::from(bstr(s))
        }
        CimValue::Boolean(v) => VARIANT::from(*v),
        CimValue::Sint8(v) => VARIANT::from(*v as i16),
        CimValue::Uint8(v) => VARIANT::from(*v),
        CimValue::Sint16(v) => VARIANT::from(*v),
        CimValue::Uint16(v) => VARIANT::from(*v as i32),
        CimValue::Sint32(v) => VARIANT::from(*v),
        CimValue::Uint32(v) => VARIANT::from(*v as i32),
        CimValue::Sint64(v) => VARIANT::from(BSTR::from(v.to_string())),
        CimValue::Uint64(v) => VARIANT::from(BSTR::from(v.to_string())),
        CimValue::Real32(v) => VARIANT::from(f32::from_bits(*v)),
        CimValue::Real64(v) => VARIANT::from(f64::from_bits(*v)),
        CimValue::Char16(v) => VARIANT::from(*v as i16),
        CimValue::Array(values) => unsafe {
            let base = match ty & 0xfff {
                2 | 16 | 103 => VT_I2,
                3 | 18 | 19 => VT_I4,
                17 => VT_UI1,
                4 => VT_R4,
                5 => VT_R8,
                11 => VT_BOOL,
                8 | 20 | 21 | 101 | 102 => VT_BSTR,
                13 => VT_UNKNOWN,
                _ => return Err(invalid()),
            };
            let array =
                SafeArrayCreateVector(base, 0, values.len().try_into().map_err(|_| invalid())?);
            if array.is_null() {
                return Err(Error::from_hresult(HRESULT(0x8007000eu32 as i32)));
            }
            (*out.0.Anonymous.Anonymous).vt = VARENUM(base.0 | VT_ARRAY.0);
            (*out.0.Anonymous.Anonymous).Anonymous.parray = array;
            for (i, value) in values.iter().enumerate() {
                let mut element = variant(value, ty & !0x2000, svc)?;
                let data = &mut (*element.0.Anonymous.Anonymous).Anonymous;
                // BSTR and interface element APIs accept the pointer value; scalar APIs accept its address.
                let ptr = match base {
                    VT_BSTR => data.bstrVal.as_ptr().cast(),
                    VT_UNKNOWN => data
                        .punkVal
                        .as_ref()
                        .map_or(std::ptr::null(), |u| u.as_raw().cast()),
                    _ => (data as *mut VARIANT_0_0_0).cast(),
                };
                SafeArrayPutElement(array, &(i as i32), ptr)?;
            }
            return Ok(out);
        },
        CimValue::Object(record) => {
            let object = unsafe { get_object(svc, &record.class)?.SpawnInstance(0)? };
            put_properties(&object, &record.properties, svc)?;
            VARIANT::from(object.cast::<IUnknown>()?)
        }
    };
    Ok(out)
}
pub(super) fn put_properties(
    object: &IWbemClassObject,
    properties: &[Property],
    svc: &IWbemServices,
) -> Result<()> {
    for p in properties {
        let value = variant(&p.value, p.cim_type, svc)?;
        let name: Vec<u16> = p.name.0.iter().copied().chain([0]).collect();
        unsafe { object.Put(PCWSTR(name.as_ptr()), 0, &value.0, 0)? }
    }
    Ok(())
}
pub(super) fn status_text(code: u32) -> WideText {
    unsafe {
        let result: Result<IWbemStatusCodeText> =
            CoCreateInstance(&WbemStatusCodeText, None, CLSCTX_INPROC_SERVER);
        if let Ok(text) = result
            && let Ok(message) = text.GetErrorCodeText(HRESULT(code as i32), 0, 0)
        {
            return wide(&message).trim_ascii();
        }
    }
    diagnostic(code).into()
}
pub(super) fn execute(
    svc: &IWbemServices,
    r: &ResolvedRequest,
    mut confirmation: Option<&mut dyn crate::backend::Confirmation>,
) -> std::result::Result<ManagementResult, ManagementError> {
    let class = get_object(svc, &r.class_name).map_err(|e| failure(e, r, "class"))?;
    let schema = schema::class_schema(&class).map_err(|e| failure(e, r, "schema"))?;
    let requested = r.method.clone().unwrap_or_default();
    let method = schema
        .methods
        .iter()
        .find(|m| requested.eq_cli_text(&m.name))
        .ok_or_else(|| {
            let mut msg = requested.clone();
            msg.0.extend(" - Invalid alias verb.".encode_utf16());
            input_error(
                InputError {
                    code: 0xABFD,
                    message: msg,
                },
                r,
            )
        })?;
    let bound = bind_method(method, &r.arguments).map_err(|e| input_error(e, r))?;
    let targets = if method.is_static || r.class_mode {
        vec![r.class_name.clone()]
    } else {
        query::query(svc, r)?
            .objects
            .into_iter()
            .filter_map(|o| o.path)
            .collect()
    };
    let mut signature = None;
    // WMIC recognizes the CLI verb before asking WMI to validate its original
    // spelling. Fullwidth WMI identifiers therefore fail with WBEM_E_NOT_FOUND.
    let name: Vec<u16> = requested.0.iter().copied().chain([0]).collect();
    unsafe {
        class.GetMethod(
            PCWSTR(name.as_ptr()),
            0,
            &mut signature,
            std::ptr::null_mut(),
        )
    }
    .map_err(|e| failure(e, r, "schema"))?;
    let input = signature
        .map(|s| unsafe { s.SpawnInstance(0) })
        .transpose()
        .map_err(|e| failure(e, r, "method input"))?;
    if let Some(input) = &input {
        put_properties(input, &bound, svc).map_err(|e| failure(e, r, "method input"))?;
    }
    let mut results = vec![];
    let had_targets = !targets.is_empty();
    let streamed = confirmation.as_ref().is_some_and(|c| c.streams_methods());
    let interactive = crate::backend::write::interactive(r);
    for path in
        crate::backend::cancellable_targets(targets, crate::platform::windows::console::interrupted)
    {
        let path =
            path.map_err(|_| cancelled(r, Some(ManagementResult::Methods(results.clone()))))?;
        if interactive {
            let mut prompt: WideText = "Execute (".into();
            prompt.0.extend(&path.0);
            prompt.0.extend(")->".encode_utf16());
            prompt.0.extend(&method.name.0);
            prompt.0.extend("() (Y/N/?)? ".encode_utf16());
            let Some(c) = confirmation.as_deref_mut() else {
                return Err(input_error(
                    InputError {
                        code: 0x80041008,
                        message: "Confirmation input unavailable".into(),
                    },
                    r,
                ));
            };
            if !c.confirm(&prompt)? {
                continue;
            }
        }
        if crate::platform::windows::console::interrupted() {
            return Err(cancelled(r, Some(ManagementResult::Methods(results))));
        }
        let mut output = None;
        let result = unsafe {
            svc.ExecMethod(
                &bstr(&path),
                &bstr(&method.name),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                input.as_ref(),
                Some(&mut output),
                None,
            )
        };
        if let Err(e) = result {
            let mut error = failure(e, r, "method");
            error.target = path;
            if !results.is_empty() {
                error.partial = Some(Box::new(ManagementResult::Methods(results)))
            }
            return Err(error);
        }
        let mut record = output
            .as_ref()
            .map(query::record)
            .transpose()
            .map_err(|e| failure(e, r, "convert"))?
            .unwrap_or_default();
        let ret = record
            .properties
            .iter()
            .position(|p| p.name.eq_ignore_case("ReturnValue"))
            .map(|i| record.properties.remove(i).value);
        let text = output
            .as_ref()
            .map(|o| unsafe { o.GetObjectText(0) })
            .transpose()
            .map_err(|e| failure(e, r, "method output"))?
            .map(|s| wide(&s));
        let result = MethodResult {
            path,
            return_value: ret,
            out_parameters: record
                .properties
                .into_iter()
                .filter(|p| !p.system)
                .collect(),
            text,
        };
        if streamed {
            if let Some(c) = confirmation.as_deref_mut() {
                c.method_completed(r, &result)?;
            }
        } else {
            results.push(result)
        }
    }
    if results.is_empty() && had_targets {
        return Ok(ManagementResult::Operation(OperationResult {
            action: Verb::Call,
            paths: vec![],
            instances: vec![Record::default()],
        }));
    }
    Ok(ManagementResult::Methods(results))
}

#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    #[ignore = "requires native Windows COM"]
    fn wmi_status_text_describes_method_errors() {
        let _apartment = com::Apartment::new().unwrap();
        let text: IWbemStatusCodeText =
            unsafe { CoCreateInstance(&WbemStatusCodeText, None, CLSCTX_INPROC_SERVER) }.unwrap();
        let result = unsafe { text.GetErrorCodeText(HRESULT(0x8004102fu32 as i32), 0, 0) };
        assert_eq!(
            wide(&result.unwrap()).trim_ascii(),
            WideText::from("Invalid method Parameter(s)")
        );
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;
    #[test]
    #[ignore = "requires native Windows WMI; creates in-memory objects only"]
    fn arrays_and_embedded_startup_object_roundtrip_without_invocation() {
        let _apartment = com::Apartment::new().unwrap();
        let svc = connect::connect(&crate::backend::ConnectionOptions::default()).unwrap();
        let class = get_object(&svc, &"Win32_Process".into()).unwrap();
        let mut signature = None;
        unsafe {
            class.GetMethod(
                windows::core::w!("Create"),
                0,
                &mut signature,
                std::ptr::null_mut(),
            )
        }
        .unwrap();
        let input = unsafe { signature.unwrap().SpawnInstance(0) }.unwrap();
        let record = Record {
            class: "Win32_ProcessStartup".into(),
            properties: vec![
                Property {
                    name: "ShowWindow".into(),
                    value: CimValue::Uint16(0),
                    cim_type: 18,
                    system: false,
                    qualifiers: vec![],
                },
                Property {
                    name: "EnvironmentVariables".into(),
                    value: CimValue::Array(vec![
                        CimValue::String("WmicCompat=a,b".into()),
                        CimValue::String("WmicCompat2=x&<y>".into()),
                    ]),
                    cim_type: 0x2008,
                    system: false,
                    qualifiers: vec![],
                },
            ],
            ..Default::default()
        };
        put_properties(
            &input,
            &[Property {
                name: "ProcessStartupInformation".into(),
                value: CimValue::Object(Box::new(record)),
                cim_type: 13,
                system: false,
                qualifiers: vec![],
            }],
            &svc,
        )
        .unwrap();
        let output = query::record(&input).unwrap();
        let property = output
            .properties
            .iter()
            .find(|p| p.name.eq_ignore_case("ProcessStartupInformation"))
            .unwrap();
        let CimValue::Object(record) = &property.value else {
            panic!("embedded object lost")
        };
        assert!(
            record
                .properties
                .iter()
                .any(|p| p.name.eq_ignore_case("ShowWindow") && p.value == CimValue::Uint16(0))
        );
        assert!(
            record
                .properties
                .iter()
                .any(|p| p.name.eq_ignore_case("EnvironmentVariables")
                    && p.value
                        == CimValue::Array(vec![
                            CimValue::String("WmicCompat=a,b".into()),
                            CimValue::String("WmicCompat2=x&<y>".into())
                        ]))
        );
    }
}
