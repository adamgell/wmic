use crate::model::WideText;

// Microsoft's EVERY/REPEAT handler calls legacy msvcrt!wcstoul, base 10, and
// accepts only an end pointer at NUL. It does not inspect errno. ULONG remains
// 32 bits on Windows, including ARM64 and AMD64.
pub(super) fn number(value: &WideText) -> Option<u32> {
    #[cfg(windows)]
    if let Some(result) = windows_crt::number(value) {
        return result;
    }
    portable_number(value)
}

fn portable_number(value: &WideText) -> Option<u32> {
    if value.0.is_empty() {
        return Some(0);
    }
    let mut units = value
        .0
        .iter()
        .copied()
        .skip_while(|u| {
            matches!(
                u,
                0x09..=0x0d | 0x20 | 0xa0 | 0x1680 | 0x180e | 0x2000..=0x200a
                    | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000
            )
        })
        .peekable();
    let negative = match units.peek() {
        Some(0x2b) => {
            units.next();
            false
        }
        Some(0x2d) => {
            units.next();
            true
        }
        _ => false,
    };
    let mut consumed_digit = false;
    let mut overflow = false;
    let mut number = 0_u32;
    for unit in units {
        let digit = [
            0x0030, 0x0660, 0x06f0, 0x0966, 0x09e6, 0x0a66, 0x0ae6, 0x0b66, 0x0c66, 0x0ce6, 0x0d66,
            0x0e50, 0x0ed0, 0x0f20, 0x1040, 0x17e0, 0x1810, 0xff10,
        ]
        .into_iter()
        .find_map(|zero| unit.checked_sub(zero).filter(|digit| *digit < 10))?;
        consumed_digit = true;
        if let Some(next) = number
            .checked_mul(10)
            .and_then(|n| n.checked_add(u32::from(digit)))
        {
            number = next;
        } else {
            overflow = true;
        }
    }
    if !consumed_digit {
        None
    } else if overflow {
        Some(u32::MAX)
    } else if negative {
        Some(number.wrapping_neg())
    } else {
        Some(number)
    }
}

#[cfg(windows)]
mod windows_crt {
    use super::WideText;
    use std::{ffi::c_void, ptr};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }

    struct Library(*mut c_void);

    impl Drop for Library {
        fn drop(&mut self) {
            // SAFETY: this guard owns the successful LoadLibraryExW reference.
            unsafe { FreeLibrary(self.0) };
        }
    }

    pub(super) fn number(value: &WideText) -> Option<Option<u32>> {
        const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
        let name: Vec<u16> = "msvcrt.dll\0".encode_utf16().collect();
        // SAFETY: the fixed name is NUL terminated, no file handle is supplied,
        // and the loader is restricted to System32 rather than a search path.
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return None;
        }
        let library = Library(module);
        // SAFETY: library remains alive through the function call below and the
        // fixed export name is NUL terminated.
        let address = unsafe { GetProcAddress(library.0, c"wcstoul".as_ptr().cast()) };
        if address.is_null() {
            return None;
        }
        type Wcstoul = unsafe extern "C" fn(*const u16, *mut *mut u16, i32) -> u32;
        // SAFETY: the legacy CRT export has this C ABI, 16-bit wchar_t input,
        // and a 32-bit unsigned long result on each supported Windows target.
        let wcstoul: Wcstoul = unsafe { std::mem::transmute(address) };
        let mut input = value.0.clone();
        input.push(0);
        let mut end = ptr::null_mut();
        // SAFETY: input is NUL terminated and end points to valid pointer storage.
        let converted = unsafe { wcstoul(input.as_ptr(), &mut end, 10) };
        // SAFETY: wcstoul's non-null end points into the still-live input buffer.
        Some((!end.is_null() && unsafe { *end == 0 }).then_some(converted))
    }
}
