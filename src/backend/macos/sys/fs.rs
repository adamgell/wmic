/// Result of `statfs` for a path.
pub fn statfs(path: &str) -> Option<libc::statfs> {
    let path = std::ffi::CString::new(path).ok()?;
    let mut buffer = unsafe { std::mem::zeroed::<libc::statfs>() };
    (unsafe { libc::statfs(path.as_ptr(), &mut buffer) } == 0).then_some(buffer)
}

/// Device node a path is mounted from, for example `/dev/disk3s1s1`.
pub fn mounted_from(path: &str) -> Option<String> {
    Some(super::c_chars(&statfs(path)?.f_mntfromname))
}

/// Every mounted file system.
pub fn mounts() -> Vec<libc::statfs> {
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if count <= 0 {
        return vec![];
    }
    let mut buffer = vec![unsafe { std::mem::zeroed::<libc::statfs>() }; count as usize];
    let size = (buffer.len() * std::mem::size_of::<libc::statfs>()) as libc::c_int;
    let filled = unsafe { libc::getfsstat(buffer.as_mut_ptr(), size, libc::MNT_NOWAIT) };
    buffer.truncate(filled.max(0) as usize);
    buffer
}
