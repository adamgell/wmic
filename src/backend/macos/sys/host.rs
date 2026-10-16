use core_foundation::{
    base::{CFType, TCFType},
    string::CFString,
};
use std::ffi::c_void;

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCopyComputerName(store: *const c_void, encoding: *mut u32) -> *const c_void;
    fn SCDynamicStoreCopyLocalHostName(store: *const c_void) -> *const c_void;
    fn SCDynamicStoreCopyConsoleUser(
        store: *const c_void,
        uid: *mut libc::uid_t,
        gid: *mut libc::gid_t,
    ) -> *const c_void;
}

/// Converts an owned CFString reference from a `Copy` function.
fn owned_string(raw: *const c_void) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    // The Copy rule hands over one reference.
    let value = unsafe { CFType::wrap_under_create_rule(raw) };
    value.downcast::<CFString>().map(|s| s.to_string())
}

/// Full name from `gethostname`, for example `Jasons-Mac-mini.lan`.
pub fn host_name() -> Option<String> {
    let mut buffer = [0 as libc::c_char; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len() - 1) } != 0 {
        return None;
    }
    let name = super::c_chars(&buffer);
    (!name.is_empty()).then_some(name)
}

/// First label of the host name.
pub fn host_label() -> Option<String> {
    let name = host_name()?;
    let label = name.split('.').next().unwrap_or_default();
    (!label.is_empty()).then(|| label.to_owned())
}

/// Upper-cased first label of the host name; the value Windows reports as the server name.
pub fn server_name() -> String {
    host_label()
        .map(|l| l.to_uppercase())
        .unwrap_or_else(|| "LOCALHOST".into())
}

/// Bonjour name without domain, case preserved. Falls back to the first host-name label.
pub fn local_host_name() -> Option<String> {
    let raw = unsafe { SCDynamicStoreCopyLocalHostName(std::ptr::null()) };
    owned_string(raw)
        .filter(|n| !n.is_empty())
        .or_else(host_label)
}

/// User-visible computer name from System Settings.
pub fn computer_name() -> Option<String> {
    let mut encoding = 0u32;
    let raw = unsafe { SCDynamicStoreCopyComputerName(std::ptr::null(), &mut encoding) };
    owned_string(raw).filter(|n| !n.is_empty())
}

/// Account name at the console, if a user is logged in.
pub fn console_user() -> Option<String> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let raw = unsafe { SCDynamicStoreCopyConsoleUser(std::ptr::null(), &mut uid, &mut gid) };
    owned_string(raw).filter(|n| !n.is_empty() && n != "loginwindow")
}

/// Console user, else the account running the process.
pub fn user_name() -> String {
    console_user().unwrap_or_else(crate::platform::unix::current_user)
}

/// Domain suffix of the host name, if the name has one.
pub fn domain_suffix() -> Option<String> {
    let name = host_name()?;
    let (_, suffix) = name.split_once('.')?;
    (!suffix.is_empty()).then(|| suffix.to_owned())
}

/// True for names that address this machine.
pub fn is_local_node(node: &str) -> bool {
    let node = node.trim();
    if matches!(node, "." | "127.0.0.1" | "::1") || node.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let matches = |name: Option<String>| name.is_some_and(|n| n.eq_ignore_ascii_case(node));
    matches(host_name()) || matches(host_label()) || matches(local_host_name())
}
