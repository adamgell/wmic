//! Local users and groups from the directory-service backed passwd and group databases.
use std::{
    ffi::{CStr, c_char},
    path::PathBuf,
    sync::Mutex,
};

pub struct User {
    pub name: String,
    pub uid: u32,
    pub gecos: String,
}

pub struct Group {
    pub name: String,
    pub gid: u32,
}

/// The passwd and group enumeration cursors are process-global state.
static CURSOR: Mutex<()> = Mutex::new(());

fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: non-null passwd and group string fields are NUL-terminated.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

pub fn users() -> Vec<User> {
    let _guard = CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = vec![];
    unsafe { libc::setpwent() };
    loop {
        let entry = unsafe { libc::getpwent() };
        if entry.is_null() {
            break;
        }
        // SAFETY: getpwent returns static storage that stays valid until the next call.
        let entry = unsafe { &*entry };
        out.push(User {
            name: text(entry.pw_name),
            uid: entry.pw_uid,
            gecos: text(entry.pw_gecos),
        });
    }
    unsafe { libc::endpwent() };
    out
}

pub fn groups() -> Vec<Group> {
    let _guard = CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = vec![];
    unsafe { libc::setgrent() };
    loop {
        let entry = unsafe { libc::getgrent() };
        if entry.is_null() {
            break;
        }
        // SAFETY: getgrent returns static storage that stays valid until the next call.
        let entry = unsafe { &*entry };
        out.push(Group {
            name: text(entry.gr_name),
            gid: entry.gr_gid,
        });
    }
    unsafe { libc::endgrent() };
    out
}

/// Account name and home directory of a user id.
fn lookup(uid: u32) -> Option<(String, PathBuf)> {
    let mut record = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16384];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut record,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    Some((text(record.pw_name), PathBuf::from(text(record.pw_dir))))
}

pub fn user_name(uid: u32) -> Option<String> {
    lookup(uid).map(|(name, _)| name).filter(|n| !n.is_empty())
}

/// Home directory of the account that started the process.
pub fn home_directory() -> Option<PathBuf> {
    lookup(unsafe { libc::getuid() })
        .map(|(_, home)| home)
        .filter(|h| !h.as_os_str().is_empty())
}
