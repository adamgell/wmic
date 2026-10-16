//! Rebuilds the Windows command-line text that the parser expects from POSIX argv.
use crate::{model::WideText, parse::is_single_token};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

const QUOTE: u16 = b'"' as u16;
const APOSTROPHE: u16 = b'\'' as u16;
const BACKSLASH: u16 = b'\\' as u16;

/// Non-UTF-8 bytes become U+FFFD.
pub fn argument_units(args: impl IntoIterator<Item = OsString>) -> Vec<Vec<u16>> {
    args.into_iter()
        .map(|arg| {
            String::from_utf8_lossy(&arg.into_vec())
                .encode_utf16()
                .collect()
        })
        .collect()
}

/// The program name is always quoted and exactly one space follows each argument boundary.
pub fn command_line(args: &[Vec<u16>]) -> WideText {
    let mut out = vec![QUOTE];
    match args.first() {
        Some(program) if !program.contains(&QUOTE) => out.extend(program),
        _ => out.extend("wmic".encode_utf16()),
    }
    out.push(QUOTE);
    for arg in args.iter().skip(1) {
        out.push(32);
        out.extend(quote_argument(arg));
    }
    WideText(out)
}

/// Keeps every argument a single parser token without changing arguments the shell already shaped.
pub fn quote_argument(arg: &[u16]) -> Vec<u16> {
    if arg.is_empty() {
        return vec![QUOTE, QUOTE];
    }
    if !arg.iter().any(|u| matches!(u, 9 | 10 | 13 | 32)) || is_single_token(arg) {
        return arg.to_vec();
    }
    if !arg.contains(&QUOTE) {
        return wrap(arg, QUOTE);
    }
    if !arg.contains(&APOSTROPHE) {
        return wrap(arg, APOSTROPHE);
    }
    let mut out = vec![QUOTE];
    let mut slashes = 0;
    for &unit in arg {
        match unit {
            BACKSLASH => {
                slashes += 1;
                out.push(unit);
            }
            QUOTE => {
                out.extend(std::iter::repeat_n(BACKSLASH, slashes));
                out.extend([BACKSLASH, QUOTE]);
                slashes = 0;
            }
            _ => {
                slashes = 0;
                out.push(unit);
            }
        }
    }
    out.extend(std::iter::repeat_n(BACKSLASH, slashes));
    out.push(QUOTE);
    out
}

/// A closing quote preceded by an odd number of backslashes would read as escaped.
fn wrap(arg: &[u16], quote: u16) -> Vec<u16> {
    let trailing = arg.iter().rev().take_while(|u| **u == BACKSLASH).count();
    let mut out = Vec::with_capacity(arg.len() + trailing + 2);
    out.push(quote);
    out.extend(arg);
    out.extend(std::iter::repeat_n(BACKSLASH, trailing));
    out.push(quote);
    out
}
