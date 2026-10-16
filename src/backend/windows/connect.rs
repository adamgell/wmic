use super::*;
use windows::{
    Win32::System::{Com::*, Wmi::*},
    core::*,
};
pub fn connect(options: &ConnectionOptions) -> Result<IWbemServices> {
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let mut ns = options.namespace.clone();
        if !options.node.eq_ascii_ignore_case(".") && !options.node.0.is_empty() {
            let mut v = vec![92, 92];
            v.extend(&options.node.0);
            v.push(92);
            v.extend(ns.0);
            ns = WideText(v);
        }
        let service = locator.ConnectServer(
            &bstr(&ns),
            &BSTR::new(),
            &BSTR::new(),
            &options.locale.as_ref().map(bstr).unwrap_or_default(),
            0,
            &BSTR::new(),
            None,
        )?;
        secure(&service)?;
        Ok(service)
    }
}
pub fn secure<I: Interface>(interface: &I) -> Result<()> {
    unsafe {
        // Borrow this exact proxy; QueryInterface(IUnknown) may select a different proxy.
        // Every COM interface starts with IUnknown; the borrow never owns/releases it.
        CoSetProxyBlanket(
            IUnknown::from_raw_borrowed(&interface.as_raw()),
            10,
            0,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
}
use crate::backend::ConnectionOptions;
