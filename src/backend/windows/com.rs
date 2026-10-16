use std::{marker::PhantomData, rc::Rc, sync::OnceLock};
use windows::{Win32::System::Com::*, core::*};
/// One successful CoInitializeEx is balanced by one CoUninitialize on this thread.
pub struct Apartment {
    _not_send: PhantomData<Rc<()>>,
}
impl Apartment {
    pub fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let a = Self {
                _not_send: PhantomData,
            };
            static SECURITY: OnceLock<std::result::Result<(), HRESULT>> = OnceLock::new();
            let sec = SECURITY.get_or_init(|| {
                let r = CoInitializeSecurity(
                    None,
                    -1,
                    None,
                    None,
                    RPC_C_AUTHN_LEVEL_DEFAULT,
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None,
                    EOAC_NONE,
                    None,
                );
                match r {
                    Err(e) if e.code().0 as u32 == 0x80010119 => Ok(()),
                    Err(e) => Err(e.code()),
                    Ok(()) => Ok(()),
                }
            });
            sec.map_err(Error::from_hresult)?;
            Ok(a)
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
