pub mod cmdline;
pub mod console;
pub mod output;

use crate::model::WideText;

const INTERRUPT_STATUS: u32 = 0xC000013A;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitAction {
    Code(i32),
    Interrupt,
}

/// A WMIC status is a 32-bit value and a POSIX status is 8 bits; zero never stands for a failure.
pub fn exit_action(status: u32) -> ExitAction {
    match status {
        0 => ExitAction::Code(0),
        INTERRUPT_STATUS => ExitAction::Interrupt,
        _ => match status & 0xFF {
            0 => ExitAction::Code(1),
            low => ExitAction::Code(low as i32),
        },
    }
}

/// Runs no destructors; the caller drops its guards first.
pub fn terminate(status: u32) -> ! {
    match exit_action(status) {
        ExitAction::Code(code) => std::process::exit(code),
        ExitAction::Interrupt => {
            // The parent shell reports termination by SIGINT only when the process dies by the signal.
            unsafe {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::raise(libc::SIGINT);
            }
            std::process::exit(130)
        }
    }
}

/// Host name up to the first dot, upper-cased like the Windows COMPUTERNAME value.
pub fn node_from_host_name(name: &[u8]) -> WideText {
    let label = name.split(|b| *b == b'.').next().unwrap_or_default();
    if label.is_empty() {
        return ".".into();
    }
    String::from_utf8_lossy(&label.to_ascii_uppercase())
        .into_owned()
        .into()
}

pub fn local_node() -> WideText {
    let mut buffer = [0u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len() - 1) } != 0 {
        return ".".into();
    }
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    node_from_host_name(&buffer[..end])
}

pub fn current_user() -> String {
    if let Some(user) = std::env::var_os("USER").filter(|u| !u.is_empty()) {
        return user.to_string_lossy().into_owned();
    }
    let mut record = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 4096];
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut record,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || record.pw_name.is_null() {
        return String::new();
    }
    // pw_name points into `buffer` and is NUL-terminated by getpwuid_r on success.
    unsafe { std::ffi::CStr::from_ptr(record.pw_name) }
        .to_string_lossy()
        .into_owned()
}
