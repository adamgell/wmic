use windows::{
    Win32::{Foundation::*, Security::*, System::Threading::*},
    core::*,
};
/// Adjust only this process token, restoring the exact original state on every exit.
pub struct PrivilegeGuard {
    token: HANDLE,
    previous: Vec<usize>,
}
impl PrivilegeGuard {
    pub fn new(enable: bool) -> Result<Self> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
                &mut token,
            )?;
            let mut guard = Self {
                token,
                previous: vec![],
            };
            let mut required = 0;
            let _ = GetTokenInformation(token, TokenPrivileges, None, 0, &mut required);
            if required < 4 {
                return Err(Error::from_thread());
            }
            guard.previous = vec![0; (required as usize).div_ceil(std::mem::size_of::<usize>())];
            GetTokenInformation(
                token,
                TokenPrivileges,
                Some(guard.previous.as_mut_ptr().cast()),
                required,
                &mut required,
            )?;
            let mut updated = guard.previous.clone();
            let privileges = updated.as_mut_ptr().cast::<TOKEN_PRIVILEGES>();
            let count = (*privileges).PrivilegeCount as usize;
            let offset = std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges);
            if offset + count * std::mem::size_of::<LUID_AND_ATTRIBUTES>() > required as usize {
                return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
            }
            let entries =
                std::slice::from_raw_parts_mut((*privileges).Privileges.as_mut_ptr(), count);
            for entry in entries {
                if enable {
                    entry.Attributes |= SE_PRIVILEGE_ENABLED
                } else {
                    entry.Attributes &= !SE_PRIVILEGE_ENABLED
                }
            }
            AdjustTokenPrivileges(token, false, Some(privileges), 0, None, None)?;
            Ok(guard)
        }
    }
}
impl Drop for PrivilegeGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_empty() {
                let _ = AdjustTokenPrivileges(
                    self.token,
                    false,
                    Some(self.previous.as_ptr().cast()),
                    0,
                    None,
                    None,
                );
            }
            let _ = CloseHandle(self.token);
        }
    }
}
