//! Thin readers over libc, Mach, IOKit and SystemConfiguration. Every reader returns `None` when the value is unavailable to an unprivileged user.
pub mod fs;
pub mod host;
pub mod identity;
pub mod iokit;
pub mod mach;
pub mod sysctl;
pub mod time;
pub mod users;

/// Converts a fixed-size C character array to text, stopping at the first NUL.
pub fn c_chars(chars: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
