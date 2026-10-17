//! Ordinary requests use shutdown(8)'s launchd path. Only explicit force uses reboot(2).
use std::{
    ffi::c_int,
    io,
    process::{Command, ExitStatus, Stdio},
};

const RB_AUTOBOOT: c_int = 0;
const RB_HALT: c_int = 0x08;

unsafe extern "C" {
    fn reboot(how_to: c_int) -> c_int;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Restart,
    /// Halt and power off the machine.
    Halt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub action: Action,
    pub force: bool,
}

/// The privilege and operating-system action boundary, injected into method tests.
pub(crate) trait Runtime {
    fn is_root(&self) -> bool;
    fn shutdown(&self, program: &str, arguments: &[&str]) -> io::Result<ExitStatus>;
    fn force_reboot(&self, how_to: c_int) -> io::Result<()>;
}

pub(crate) struct Native;

impl Runtime for Native {
    fn is_root(&self) -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    fn shutdown(&self, program: &str, arguments: &[&str]) -> io::Result<ExitStatus> {
        Command::new(program)
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    }

    fn force_reboot(&self, how_to: c_int) -> io::Result<()> {
        // RB_NOSYNC is never set: the kernel still flushes the filesystem cache.
        if unsafe { reboot(how_to) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(test)]
#[path = "power_tests.rs"]
pub(crate) mod tests;

/// Win32 return code: 0 when accepted, 5 without privilege, 8 otherwise. An accepted request
/// may terminate this process before a result can be returned; it does not prove power-off.
pub(crate) fn request(request: Request, runtime: &impl Runtime) -> u32 {
    if !runtime.is_root() {
        return 5;
    }
    let result = if request.force {
        let how_to = match request.action {
            Action::Restart => RB_AUTOBOOT,
            Action::Halt => RB_HALT,
        };
        runtime.force_reboot(how_to)
    } else {
        let flag = match request.action {
            Action::Restart => "-r",
            Action::Halt => "-h",
        };
        // No shell, -o, -n, or forced fallback. shutdown(8) coordinates with launchd.
        match runtime.shutdown("/sbin/shutdown", &[flag, "now"]) {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => return 8,
            Err(error) => Err(error),
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => match error.raw_os_error() {
            Some(libc::EPERM | libc::EACCES) => 5,
            _ => 8,
        },
    }
}
