//! System restart and halt through `reboot(2)`.
use std::ffi::c_int;

const RB_AUTOBOOT: c_int = 0;
const RB_HALT: c_int = 0x08;

unsafe extern "C" {
    fn reboot(how_to: c_int) -> c_int;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Restart,
    /// The kernel halts and powers off the machine.
    Halt,
}

pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Win32 return code of the request: 0 when the call returned success, 5 without privilege,
/// 8 otherwise. A successful request ends the process.
pub fn request(action: Action) -> u32 {
    let how_to = match action {
        Action::Restart => RB_AUTOBOOT,
        Action::Halt => RB_HALT,
    };
    unsafe { libc::sync() };
    if unsafe { reboot(how_to) } == 0 {
        return 0;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => 5,
        _ => 8,
    }
}
