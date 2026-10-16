use super::*;
use crate::backend::write::bind_properties;
use windows::{Win32::System::Wmi::*, core::*};
pub(super) fn execute(
    svc: &IWbemServices,
    r: &ResolvedRequest,
    mut confirmation: Option<&mut dyn crate::backend::Confirmation>,
) -> std::result::Result<ManagementResult, ManagementError> {
    let class = get_object(svc, &r.class_name).map_err(|e| failure(e, r, "class"))?;
    let schema = schema::class_schema(&class).map_err(|e| failure(e, r, "schema"))?;
    let bound = if r.verb == Verb::Delete {
        vec![]
    } else {
        bind_properties(&schema, &r.arguments).map_err(|e| methods::input_error(e, r))?
    };
    if r.verb == Verb::Create {
        if crate::backend::write::interactive(r) {
            let mut prompt: WideText = "Create instance of '".into();
            prompt.0.extend(&r.class_name.0);
            prompt.0.extend("' class (Y/N)?".encode_utf16());
            let Some(c) = confirmation.as_deref_mut() else {
                return Err(methods::input_error(
                    crate::backend::methods::InputError {
                        code: 0x80041008,
                        message: "Confirmation input unavailable".into(),
                    },
                    r,
                ));
            };
            if !c.confirm(&prompt)? {
                return Ok(ManagementResult::Operation(OperationResult {
                    action: r.verb,
                    paths: vec![],
                    instances: vec![Record::default()],
                }));
            }
        }
        if crate::platform::windows::console::interrupted() {
            return Err(cancelled(r, None));
        }
        let instance = unsafe { class.SpawnInstance(0) }.map_err(|e| failure(e, r, "create"))?;
        methods::put_properties(&instance, &bound, svc).map_err(|e| failure(e, r, "create"))?;
        if crate::platform::windows::console::interrupted() {
            return Err(cancelled(r, None));
        }
        unsafe { svc.PutInstance(&instance, WBEM_GENERIC_FLAG_TYPE(2), None, None) }
            .map_err(|e| failure(e, r, "create"))?;
        return Ok(ManagementResult::Operation(OperationResult {
            action: r.verb,
            paths: vec![],
            instances: vec![],
        }));
    }
    let selected = if r.class_mode {
        ObjectSet {
            objects: vec![query::record(&class).map_err(|e| failure(e, r, "class"))?],
            schema: None,
        }
    } else {
        query::query(svc, r)?
    };
    let interactive = crate::backend::write::interactive(r);
    let instances: Vec<_> = selected
        .objects
        .iter()
        .map(|o| Record {
            class: o.class.clone(),
            path: o.path.clone(),
            ..Default::default()
        })
        .collect();
    let mut paths = vec![];
    for record in crate::backend::cancellable_targets(
        selected.objects,
        crate::platform::windows::console::interrupted,
    ) {
        let record = record.map_err(|_| {
            cancelled(
                r,
                Some(ManagementResult::Operation(OperationResult {
                    action: r.verb,
                    paths: paths.clone(),
                    instances: vec![],
                })),
            )
        })?;
        let path = record.path.ok_or_else(|| {
            failure(
                Error::from_hresult(HRESULT(0x80041002u32 as i32)),
                r,
                "write",
            )
        })?;
        if interactive {
            let mut prompt: WideText = if r.verb == Verb::Delete {
                "Delete '"
            } else {
                "Update property(s) of '"
            }
            .into();
            prompt.0.extend(&path.0);
            prompt.0.extend("' (Y/N/?)? ".encode_utf16());
            let Some(c) = confirmation.as_deref_mut() else {
                return Err(methods::input_error(
                    crate::backend::methods::InputError {
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
            return Err(cancelled(
                r,
                Some(ManagementResult::Operation(OperationResult {
                    action: r.verb,
                    paths,
                    instances: vec![],
                })),
            ));
        }
        let result: windows::core::Result<()> = (|| {
            if r.verb == Verb::Delete {
                if r.class_mode {
                    unsafe {
                        svc.DeleteClass(&bstr(&r.class_name), WBEM_GENERIC_FLAG_TYPE(0), None, None)
                    }
                } else {
                    unsafe {
                        svc.DeleteInstance(&bstr(&path), WBEM_GENERIC_FLAG_TYPE(0), None, None)
                    }
                }
            } else {
                let instance = get_object(svc, &path)?;
                methods::put_properties(&instance, &bound, svc)?;
                if crate::platform::windows::console::interrupted() {
                    return Err(Error::from_hresult(HRESULT(0xC000013Au32 as i32)));
                }
                if r.class_mode {
                    unsafe { svc.PutClass(&instance, WBEM_GENERIC_FLAG_TYPE(1), None, None) }
                } else {
                    unsafe { svc.PutInstance(&instance, WBEM_GENERIC_FLAG_TYPE(1), None, None) }
                }
            }
        })();
        if let Err(e) = result {
            let mut error = failure(
                e,
                r,
                if r.verb == Verb::Delete {
                    "delete"
                } else {
                    "update"
                },
            );
            error.target = path;
            if !paths.is_empty() {
                error.partial = Some(Box::new(ManagementResult::Operation(OperationResult {
                    action: r.verb,
                    paths,
                    instances: vec![],
                })))
            }
            return Err(error);
        }
        paths.push(path);
    }
    Ok(ManagementResult::Operation(OperationResult {
        action: r.verb,
        paths,
        instances,
    }))
}
