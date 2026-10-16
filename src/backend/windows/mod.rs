//! COM interfaces remain in the creating apartment. No interface is sent to a worker thread.
mod com;
mod connect;
mod ping;
mod privileges;
mod query;
mod schema;
use crate::{
    backend::{Management, ResolvedRequest},
    model::*,
};
use windows::{
    Win32::System::Wmi::*,
    core::{BSTR, IUnknown, Interface},
};
pub(crate) fn bstr(s: &WideText) -> BSTR {
    BSTR::from_wide(&s.0)
}
pub(crate) fn wide(s: &BSTR) -> WideText {
    WideText(s.to_vec())
}
pub(crate) fn diagnostic(code: u32) -> &'static str {
    match code {
        0x80041002 => "Not found",
        0x80041003 | 0x80070005 => "Access denied",
        0x8004100e => "Invalid namespace",
        0x80041010 => "Invalid class",
        0x80041017 => "Invalid query",
        0x80041008 => "Invalid parameter",
        0x8004100c => "Not supported",
        0x800706ba => "The RPC server is unavailable.",
        _ => "Unknown error",
    }
}
pub(crate) fn failure(
    e: windows::core::Error,
    r: &ResolvedRequest,
    operation: &str,
) -> ManagementError {
    let extended = unsafe {
        // Error owns IErrorInfo for this borrow; WMI errors can also expose IWbemClassObject.
        IUnknown::from_raw_borrowed(&e.as_ptr())
            .and_then(|info| info.cast::<IWbemClassObject>().ok())
            .and_then(|object| query::record(&object).ok())
            .map(|record| record.properties)
            .unwrap_or_default()
    };
    ManagementError {
        code: e.code().0 as u32,
        target: r.connection.node.clone(),
        message: diagnostic(e.code().0 as u32).into(),
        operation: operation.into(),
        extended,
        partial: None,
    }
}
pub struct WindowsManagement {
    _apartment: com::Apartment,
}
impl WindowsManagement {
    pub fn new() -> Result<Self, ManagementError> {
        com::Apartment::new()
            .map(|a| Self { _apartment: a })
            .map_err(|e| ManagementError {
                code: e.code().0 as u32,
                target: ".".into(),
                message: e.message().into(),
                operation: "COM initialization".into(),
                extended: vec![],
                partial: None,
            })
    }
}
impl Management for WindowsManagement {
    fn reachable(&mut self, node: &WideText, timeout_ms: u32) -> bool {
        ping::reachable(node, timeout_ms)
    }
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        let _privileges = privileges::PrivilegeGuard::new(r.connection.privileges)
            .map_err(|e| failure(e, r, "privileges"))?;
        let svc = connect::connect(&r.connection).map_err(|e| failure(e, r, "query"))?;
        let schema_obj = get_object(&svc, &r.class_name).map_err(|e| failure(e, r, "class"))?;
        match r.verb {
            Verb::Get | Verb::List => {
                if r.class_mode {
                    return schema::class_schema(&schema_obj)
                        .map(ManagementResult::Schema)
                        .map_err(|e| failure(e, r, "schema"));
                }
                query::query(&svc, r).map(ManagementResult::Objects)
            }
            _ => Err(ManagementError {
                code: 0x8004100c,
                target: r.connection.node.clone(),
                message: "Not supported".into(),
                operation: "unimplemented operation".into(),
                extended: vec![],
                partial: None,
            }),
        }
    }
}
pub(crate) fn get_object(
    svc: &IWbemServices,
    path: &WideText,
) -> windows::core::Result<IWbemClassObject> {
    unsafe {
        let mut object = None;
        svc.GetObject(
            &bstr(path),
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut object),
            None,
        )?;
        object.ok_or_else(|| {
            windows::core::Error::from_hresult(windows::core::HRESULT(0x80041002u32 as i32))
        })
    }
}
