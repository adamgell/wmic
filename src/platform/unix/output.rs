use crate::{
    model::WideText,
    render::{DestinationProfile, OutputError, encode_output},
    session::{InputError, InputEvent, InputSource, OutputSink, OutputStream},
};
use std::{
    fs::File,
    io::{self, Write},
    mem::ManuallyDrop,
    os::fd::FromRawFd,
};

pub struct NativeOutput;
impl OutputSink for NativeOutput {
    fn clear_wait_prompt(&mut self) -> Result<(), OutputError> {
        super::console::clear_wait_prompt().map_err(|e| OutputError(e.to_string()))
    }
    fn console_input(&self) -> bool {
        super::console::has_input_console()
    }
    fn clipboard(&mut self, text: &WideText) -> Result<(), OutputError> {
        #[cfg(target_os = "macos")]
        {
            crate::platform::macos::clipboard::copy(text)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = text;
            Err(OutputError("Clipboard unavailable".into()))
        }
    }
    fn record_user(&self) -> WideText {
        let mut user = super::local_node();
        user.0.push(u16::from(b'\\'));
        user.0.extend(super::current_user().encode_utf16());
        user
    }
    fn read_line(&mut self) -> Result<Option<WideText>, OutputError> {
        read_input(false)
    }
    fn read_secret(&mut self) -> Result<Option<WideText>, OutputError> {
        read_input(true)
    }
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        if text.0.is_empty() {
            return Ok(());
        }
        let bytes = encode_output(
            &WideText(native_newlines(&text.0)),
            &DestinationProfile::pipe(65001),
        )?;
        let fd = if stream == OutputStream::Stdout { 1 } else { 2 };
        write_fd(fd, &bytes).map_err(|e| OutputError(e.to_string()))
    }
}

/// Rendered text carries Windows line endings; terminals and pipes receive `\n`.
pub fn native_newlines(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i..].starts_with(&[13, 13, 10]) {
            out.push(10);
            i += 3;
        } else if units[i..].starts_with(&[13, 10]) {
            out.push(10);
            i += 2;
        } else {
            out.push(units[i]);
            i += 1;
        }
    }
    out
}

/// Writes without Rust-side buffering; `write_all` retries partial writes and EINTR.
pub(super) fn write_fd(fd: i32, bytes: &[u8]) -> io::Result<()> {
    // The descriptor belongs to the process; ManuallyDrop keeps it open.
    let mut file = ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
    file.write_all(bytes)
}

struct EchoOff {
    saved: libc::termios,
}
impl EchoOff {
    fn enter() -> Option<Self> {
        if !super::console::has_input_console() {
            return None;
        }
        let mut saved = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(0, &mut saved) } != 0 {
            return None;
        }
        let mut quiet = saved;
        quiet.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &quiet) } != 0 {
            return None;
        }
        Some(Self { saved })
    }
}
impl Drop for EchoOff {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.saved) };
        // The Enter key was not echoed, so the cursor is still on the prompt line.
        let _ = write_fd(2, b"\n");
    }
}

fn read_input(secret: bool) -> Result<Option<WideText>, OutputError> {
    let _echo = if secret { EchoOff::enter() } else { None };
    let mut out = vec![];
    let mut received = false;
    loop {
        let mut byte = 0u8;
        let count = unsafe { libc::read(0, (&mut byte as *mut u8).cast(), 1) };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                if super::console::interrupted() {
                    return Err(OutputError("Interrupted".into()));
                }
                continue;
            }
            return Err(OutputError(error.to_string()));
        }
        if count == 0 {
            break;
        }
        received = true;
        match byte {
            10 => break,
            13 => {}
            _ => out.push(byte),
        }
    }
    if !received {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&out).into_owned().into()))
}

pub struct NativeInput;
impl InputSource for NativeInput {
    fn next_input(&mut self) -> Result<InputEvent, InputError> {
        if super::console::interrupted() {
            return Ok(InputEvent::Interrupted);
        }
        let console = super::console::input_is_tty();
        match read_input(false) {
            Ok(Some(line)) => Ok(InputEvent::Line { line, console }),
            Ok(None) => Ok(InputEvent::Eof),
            Err(_) if super::console::interrupted() => Ok(InputEvent::Interrupted),
            Err(e) => Err(e.into()),
        }
    }
}
