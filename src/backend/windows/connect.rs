use super::*;
use crate::backend::ConnectionOptions;
use windows::{
    Win32::System::{Com::*, Wmi::*},
    core::*,
};
pub fn connect(options: &ConnectionOptions) -> Result<IWbemServices> {
    let service = connect_unsecured(options)?;
    secure(&service, options)?;
    Ok(service)
}

/// Keep connection and proxy security separate for parser diagnostics.
pub(super) fn connect_unsecured(options: &ConnectionOptions) -> Result<IWbemServices> {
    unsafe {
        options
            .validate()
            .map_err(|e| Error::from_hresult(HRESULT(e.code as i32)))?;
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let mut ns = options.namespace.clone();
        if !options.node.eq_ignore_case(".") && !options.node.0.is_empty() {
            let mut v = vec![92, 92];
            v.extend(&options.node.0);
            v.push(92);
            v.extend(ns.0);
            ns = WideText(v);
        }
        let user = options.user.as_ref().map(bstr).unwrap_or_default();
        let password = options
            .password
            .as_ref()
            .map(|p| bstr(p.expose()))
            .unwrap_or_default();
        locator.ConnectServer(
            &bstr(&ns),
            &user,
            &password,
            &options.locale.as_ref().map(bstr).unwrap_or_default(),
            0,
            &options.authority.as_ref().map(bstr).unwrap_or_default(),
            None,
        )
    }
}
/// Copies Unicode credentials into RPC on each exact interface, including enumerators.
/// CoSetProxyBlanket permits discarding SEC_WINNT_AUTH_IDENTITY after it returns.
pub fn secure<I: Interface>(interface: &I, options: &ConnectionOptions) -> Result<()> {
    unsafe {
        let (user, domain) = options.identity_parts();
        let (mut user, mut domain) = (user.0, domain.0);
        let mut password = options
            .password
            .as_ref()
            .map(|p| p.expose().0.clone())
            .unwrap_or_default();
        let identity = COAUTHIDENTITY {
            User: user.as_mut_ptr(),
            UserLength: user.len() as u32,
            Domain: domain.as_mut_ptr(),
            DomainLength: domain.len() as u32,
            Password: password.as_mut_ptr(),
            PasswordLength: password.len() as u32,
            Flags: 2,
        };
        // Borrow the original pointer. QI(IUnknown) may select a different interface proxy.
        let result = CoSetProxyBlanket(
            IUnknown::from_raw_borrowed(&interface.as_raw()),
            u32::MAX,
            0,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL(options.authentication),
            RPC_C_IMP_LEVEL(options.impersonation),
            options
                .user
                .as_ref()
                .map(|_| (&identity as *const COAUTHIDENTITY).cast()),
            EOAC_NONE,
        );
        password.fill(0);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "selected Windows guest only"]
    fn per_interface_security_applies_to_services_and_enumerators() {
        unsafe {
            let _management = WindowsManagement::new().unwrap();
            let settings = ConnectionOptions::default();
            let service = connect(&settings).unwrap();
            let en = service
                .ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from("SELECT Caption FROM Win32_OperatingSystem"),
                    WBEM_GENERIC_FLAG_TYPE(0x30),
                    None,
                )
                .unwrap();
            secure(&en, &settings).unwrap();
            fn levels<I: Interface>(interface: &I) -> (u32, u32) {
                unsafe {
                    let (mut auth, mut imp) = (0, 0);
                    CoQueryProxyBlanket(
                        IUnknown::from_raw_borrowed(&interface.as_raw()),
                        None,
                        None,
                        None,
                        Some(&mut auth),
                        Some(&mut imp),
                        None,
                        None,
                    )
                    .unwrap();
                    (auth, imp)
                }
            }
            assert_eq!(levels(&service), (6, 3));
            assert_eq!(levels(&en), (6, 3));
        }
    }
}
