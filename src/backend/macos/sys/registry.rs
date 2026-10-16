//! Walks of the IOService plane and class conformance checks.
use super::{cfvalue, iokit::Entry};
use io_kit_sys::{
    IOObjectConformsTo, IOObjectGetClass, IORegistryEntryGetParentEntry, keys::kIOServicePlane,
};
use std::ffi::CString;

/// Direct parent in the IOService plane.
pub fn parent(entry: &Entry) -> Option<Entry> {
    let mut raw = 0;
    let status = unsafe { IORegistryEntryGetParentEntry(entry.0, kIOServicePlane, &mut raw) };
    (status == 0 && raw != 0).then_some(Entry(raw))
}

/// True when the entry's class is, or derives from, the named class.
pub fn conforms_to(entry: &Entry, class: &str) -> bool {
    let Ok(class) = CString::new(class) else {
        return false;
    };
    unsafe { IOObjectConformsTo(entry.0, class.as_ptr()) != 0 }
}

/// Class name of the entry.
pub fn class_name(entry: &Entry) -> Option<String> {
    // An io_name_t holds 128 characters including the terminator.
    let mut name = [0 as libc::c_char; 128];
    let status = unsafe { IOObjectGetClass(entry.0, name.as_mut_ptr()) };
    (status == 0).then(|| super::c_chars(&name))
}

/// Nearest ancestor of the entry that conforms to the class, within `depth` generations.
pub fn ancestor_conforming_to(entry: &Entry, class: &str, depth: usize) -> Option<Entry> {
    let mut current = parent(entry)?;
    for _ in 0..depth {
        if conforms_to(&current, class) {
            return Some(current);
        }
        current = parent(&current)?;
    }
    None
}

/// Dictionary-valued property of an entry.
pub fn dictionary(entry: &Entry, key: &str) -> Option<cfvalue::Dictionary> {
    cfvalue::dictionary(&entry.property(key)?)
}
