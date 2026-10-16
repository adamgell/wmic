use std::ffi::CString;

/// Raw bytes of a named sysctl value.
pub fn bytes(name: &str) -> Option<Vec<u8>> {
    let name = CString::new(name).ok()?;
    let mut length = 0usize;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    let mut buffer = vec![0u8; length];
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    buffer.truncate(length);
    Some(buffer)
}

/// Text value up to the first NUL.
pub fn string(name: &str) -> Option<String> {
    let bytes = bytes(name)?;
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// Unsigned view of a 1, 2, 4 or 8 byte integer value.
pub fn unsigned(name: &str) -> Option<u64> {
    let bytes = bytes(name)?;
    match bytes.len() {
        1 => Some(bytes[0] as u64),
        2 => Some(u16::from_ne_bytes(bytes.try_into().ok()?) as u64),
        4 => Some(u32::from_ne_bytes(bytes.try_into().ok()?) as u64),
        8 => Some(u64::from_ne_bytes(bytes.try_into().ok()?)),
        _ => None,
    }
}

/// Plain-data structure value; the sysctl must return exactly `size_of::<T>()` bytes.
pub fn structure<T: Copy>(name: &str) -> Option<T> {
    let bytes = bytes(name)?;
    if bytes.len() != std::mem::size_of::<T>() {
        return None;
    }
    // SAFETY: the length matches and callers use integer-only structures.
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

/// Swap totals in bytes: (total, available, used).
pub fn swap_usage() -> Option<(u64, u64, u64)> {
    let usage = structure::<libc::xsw_usage>("vm.swapusage")?;
    Some((usage.xsu_total, usage.xsu_avail, usage.xsu_used))
}

/// Seconds and microseconds of the last boot.
pub fn boot_time() -> Option<(i64, u32)> {
    let value = structure::<libc::timeval>("kern.boottime")?;
    Some((value.tv_sec, value.tv_usec.max(0) as u32))
}
