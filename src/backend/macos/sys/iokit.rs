use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::CFData,
    number::CFNumber,
    string::CFString,
};
use io_kit_sys::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IORegistryEntryFromPath,
    IOServiceGetMatchingService, IOServiceGetMatchingServices, IOServiceMatching,
    types::io_object_t,
};
use std::ffi::CString;

/// The default main port is passed as zero.
const DEFAULT_PORT: u32 = 0;

/// Owned IORegistry entry, released on drop.
pub struct Entry(io_object_t);

impl Drop for Entry {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0) };
    }
}

impl Entry {
    /// Entry at a plane path such as `IODeviceTree:/chosen`.
    pub fn from_path(path: &str) -> Option<Entry> {
        let path = CString::new(path).ok()?;
        let raw = unsafe { IORegistryEntryFromPath(DEFAULT_PORT, path.as_ptr().cast_mut()) };
        (raw != 0).then_some(Entry(raw))
    }

    /// First service matching an IOKit class name.
    pub fn matching_service(class: &str) -> Option<Entry> {
        let class = CString::new(class).ok()?;
        let matching = unsafe { IOServiceMatching(class.as_ptr()) };
        if matching.is_null() {
            return None;
        }
        // The call consumes the matching dictionary.
        let raw = unsafe { IOServiceGetMatchingService(DEFAULT_PORT, matching.cast_const()) };
        (raw != 0).then_some(Entry(raw))
    }

    /// Every service matching an IOKit class name.
    pub fn matching_services(class: &str) -> Vec<Entry> {
        let Ok(class) = CString::new(class) else {
            return vec![];
        };
        let matching = unsafe { IOServiceMatching(class.as_ptr()) };
        if matching.is_null() {
            return vec![];
        }
        let mut iterator: io_object_t = 0;
        // The call consumes the matching dictionary.
        let status = unsafe {
            IOServiceGetMatchingServices(DEFAULT_PORT, matching.cast_const(), &mut iterator)
        };
        if status != 0 {
            return vec![];
        }
        let mut out = vec![];
        loop {
            let raw = unsafe { IOIteratorNext(iterator) };
            if raw == 0 {
                break;
            }
            out.push(Entry(raw));
        }
        unsafe { IOObjectRelease(iterator) };
        out
    }

    pub fn property(&self, key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        let raw = unsafe {
            IORegistryEntryCreateCFProperty(self.0, key.as_concrete_TypeRef(), std::ptr::null(), 0)
        };
        // The create rule hands over one reference.
        (!raw.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(raw) })
    }

    /// Text property; byte-array properties are read up to the first NUL.
    pub fn string(&self, key: &str) -> Option<String> {
        let value = self.property(key)?;
        if let Some(text) = value.downcast::<CFString>() {
            return Some(text.to_string());
        }
        let data = value.downcast::<CFData>()?;
        Some(nul_terminated(data.bytes()))
    }

    pub fn data(&self, key: &str) -> Option<Vec<u8>> {
        Some(self.property(key)?.downcast::<CFData>()?.bytes().to_vec())
    }

    pub fn number(&self, key: &str) -> Option<i64> {
        self.property(key)?.downcast::<CFNumber>()?.to_i64()
    }

    pub fn boolean(&self, key: &str) -> Option<bool> {
        Some(self.property(key)?.downcast::<CFBoolean>()?.into())
    }
}

/// Text of a byte array up to the first NUL.
pub fn nul_terminated(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
