//! Read class and method metadata without querying instances or invoking operations.
use super::*;
use crate::backend::{
    PathPreflightError, PathPreflightOutcome, PathPreflightRequest, PathPreflightStage,
};
use windows::{
    Win32::System::{Com::SAFEARRAY, Ole::SafeArrayDestroy},
    core::{Error, HRESULT, PCWSTR},
};

fn cancellation(
    request: &PathPreflightRequest<'_>,
    stage: PathPreflightStage,
) -> PathPreflightError {
    PathPreflightError {
        stage,
        completed_method_signatures: 0,
        error: ManagementError {
            code: 0xC000013A,
            target: request.connection.node.clone(),
            message: "Operation cancelled".into(),
            operation: "cancel".into(),
            extended: vec![],
            partial: None,
        },
    }
}

fn check_interrupted(
    request: &PathPreflightRequest<'_>,
    stage: PathPreflightStage,
) -> Result<(), PathPreflightError> {
    if crate::platform::windows::console::interrupted() {
        Err(cancellation(request, stage))
    } else {
        Ok(())
    }
}

fn failure(
    error: Error,
    request: &PathPreflightRequest<'_>,
    stage: PathPreflightStage,
) -> PathPreflightError {
    if crate::platform::windows::console::interrupted() {
        return cancellation(request, stage);
    }
    let error = failure_with_connection(error, request.connection, "class");
    if crate::platform::windows::console::interrupted() {
        return cancellation(request, stage);
    }
    PathPreflightError {
        stage,
        completed_method_signatures: 0,
        error,
    }
}

/// GetNames transfers the array and its BSTR elements to the caller.
struct Names(*mut SAFEARRAY);
impl Drop for Names {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // This owner never locks or borrows the array's backing storage.
            unsafe {
                let _ = SafeArrayDestroy(self.0);
            }
        }
    }
}

fn names(
    object: &IWbemClassObject,
    request: &PathPreflightRequest<'_>,
) -> Result<(), PathPreflightError> {
    check_interrupted(request, PathPreflightStage::Names)?;
    let _names = Names(
        unsafe { object.GetNames(PCWSTR::null(), WBEM_FLAG_NONSYSTEM_ONLY, std::ptr::null()) }
            .map_err(|error| failure(error, request, PathPreflightStage::Names))?,
    );
    check_interrupted(request, PathPreflightStage::Names)
}

pub(super) fn execute(
    request: PathPreflightRequest<'_>,
) -> Result<PathPreflightOutcome, PathPreflightError> {
    check_interrupted(&request, PathPreflightStage::Connect)?;
    let service = connect::connect_unsecured(request.connection)
        .map_err(|error| failure(error, &request, PathPreflightStage::Connect))?;
    check_interrupted(&request, PathPreflightStage::Security)?;
    connect::secure(&service, request.connection)
        .map_err(|error| failure(error, &request, PathPreflightStage::Security))?;
    check_interrupted(&request, PathPreflightStage::Class)?;
    let mut class = None;
    unsafe {
        service.GetObject(
            &bstr(request.class_name),
            WBEM_FLAG_USE_AMENDED_QUALIFIERS,
            None,
            Some(&mut class),
            None,
        )
    }
    .map_err(|error| failure(error, &request, PathPreflightStage::Class))?;
    check_interrupted(&request, PathPreflightStage::Class)?;
    let class = class.ok_or_else(|| {
        failure(
            Error::from_hresult(HRESULT(0x80041002u32 as i32)),
            &request,
            PathPreflightStage::Class,
        )
    })?;
    let method_signatures = if let Some(method) = request.method {
        method_signatures(&class, method, &request)?
    } else {
        names(&class, &request)?;
        0
    };
    check_interrupted(
        &request,
        if request.method.is_some() {
            PathPreflightStage::Method
        } else {
            PathPreflightStage::Names
        },
    )
    .map_err(|mut error| {
        error.completed_method_signatures = method_signatures;
        error
    })?;
    Ok(PathPreflightOutcome::Ready { method_signatures })
}

fn method_signatures(
    class: &IWbemClassObject,
    requested: &WideText,
    request: &PathPreflightRequest<'_>,
) -> Result<u8, PathPreflightError> {
    check_interrupted(request, PathPreflightStage::Method)?;
    unsafe { class.BeginMethodEnumeration(0) }
        .map_err(|error| failure(error, request, PathPreflightStage::Method))?;
    let result: Result<u8, PathPreflightError> = (|| {
        loop {
            check_interrupted(request, PathPreflightStage::Method)?;
            let mut name = BSTR::new();
            let mut input = None;
            let mut output = None;
            unsafe { class.NextMethod(0, &mut name, &mut input, &mut output) }
                .map_err(|error| failure(error, request, PathPreflightStage::Method))?;
            check_interrupted(request, PathPreflightStage::Method)?;
            if name.is_empty() {
                let error = crate::backend::methods::unknown_method(requested, false);
                return Err(PathPreflightError {
                    stage: PathPreflightStage::Method,
                    completed_method_signatures: 0,
                    error: ManagementError {
                        code: error.code,
                        target: request.class_name.clone(),
                        message: error.message,
                        operation: "input".into(),
                        extended: vec![],
                        partial: None,
                    },
                });
            }
            if requested.eq_cli_text(&wide(&name)) {
                let mut count = 0;
                for signature in [input, output].into_iter().flatten() {
                    if let Err(mut error) = names(&signature, request) {
                        error.completed_method_signatures = count;
                        return Err(error);
                    }
                    count += 1;
                }
                return Ok(count);
            }
        }
    })();
    let end = unsafe { class.EndMethodEnumeration() };
    let completed = result
        .as_ref()
        .map_or_else(|error| error.completed_method_signatures, |count| *count);
    check_interrupted(request, PathPreflightStage::Method).map_err(|mut error| {
        error.completed_method_signatures = completed;
        error
    })?;
    let count = result?;
    end.map_err(|error| {
        let mut error = failure(error, request, PathPreflightStage::Method);
        error.completed_method_signatures = count;
        error
    })?;
    Ok(count)
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    #[test]
    #[ignore = "selected Windows guest only; owns the global console cancellation guard"]
    fn interrupted_preflight_stops_before_com_connection() {
        let _apartment = com::Apartment::new().unwrap();
        let _guard = crate::platform::windows::console::InterruptGuard::install().unwrap();
        // This thread does not initialize COM. Even if the runtime makes COM
        // available implicitly, interruption must prevent a metadata connection.
        let error = std::thread::spawn(|| {
            crate::platform::windows::console::request_cancel_for_test();
            execute(PathPreflightRequest {
                connection: &crate::backend::ConnectionOptions::default(),
                class_name: &"Win32_OperatingSystem".into(),
                method: None,
            })
            .unwrap_err()
        })
        .join()
        .unwrap();
        assert_eq!(error.error.code, 0xC000013A);
        assert!(error.error.operation.eq_ignore_case("cancel"));
        assert!(error.error.partial.is_none());
    }

    #[test]
    #[ignore = "selected Windows guest only; owns the global console cancellation guard"]
    fn interrupted_metadata_failure_uses_quiet_cancellation_code() {
        let _apartment = com::Apartment::new().unwrap();
        let _guard = crate::platform::windows::console::InterruptGuard::install().unwrap();
        crate::platform::windows::console::request_cancel_for_test();
        let error = failure(
            Error::from_hresult(HRESULT(0x800706bau32 as i32)),
            &PathPreflightRequest {
                connection: &crate::backend::ConnectionOptions::default(),
                class_name: &"Win32_OperatingSystem".into(),
                method: None,
            },
            PathPreflightStage::Names,
        );
        assert_eq!(error.error.code, 0xC000013A);
        assert!(error.error.operation.eq_ignore_case("cancel"));
        assert!(error.error.extended.is_empty());
    }
}
