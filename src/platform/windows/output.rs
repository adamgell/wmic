use crate::{
    model::WideText,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream},
};
use windows::Win32::{Globalization::GetACP, Storage::FileSystem::WriteFile, System::Console::*};
pub struct NativeOutput;
impl OutputSink for NativeOutput {
    fn read_secret(&mut self) -> Result<Option<WideText>, OutputError> {
        read_password()
    }
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        if text.0.is_empty() {
            return Ok(());
        }
        let handle = unsafe {
            GetStdHandle(if stream == OutputStream::Stdout {
                STD_OUTPUT_HANDLE
            } else {
                STD_ERROR_HANDLE
            })
        }
        .map_err(|e| OutputError(e.to_string()))?;
        let mut mode = CONSOLE_MODE(0);
        if unsafe { GetConsoleMode(handle, &mut mode) }.is_ok() {
            let mut written = 0;
            unsafe { WriteConsoleW(handle, &text.0, Some(&mut written), None) }
                .map_err(|e| OutputError(e.to_string()))?;
            if written as usize != text.0.len() {
                return Err(OutputError("Incomplete console write".into()));
            }
            return Ok(());
        }
        let bytes = encode_output(text, &DestinationProfile::pipe(unsafe { GetACP() }))?;
        let mut at = 0;
        while at < bytes.len() {
            let mut written = 0;
            unsafe { WriteFile(handle, Some(&bytes[at..]), Some(&mut written), None) }
                .map_err(|e| OutputError(e.to_string()))?;
            if written == 0 {
                return Err(OutputError("Incomplete output write".into()));
            }
            at += written as usize;
        }
        Ok(())
    }
}

fn read_password() -> Result<Option<WideText>, OutputError> {
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::ReadFile};
    let handle =
        unsafe { GetStdHandle(STD_INPUT_HANDLE) }.map_err(|e| OutputError(e.to_string()))?;
    let mut mode = CONSOLE_MODE(0);
    let is_console = unsafe { GetConsoleMode(handle, &mut mode) }.is_ok();
    struct Restore {
        handle: HANDLE,
        mode: Option<CONSOLE_MODE>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(mode) = self.mode {
                unsafe {
                    let _ = SetConsoleMode(self.handle, mode);
                }
            }
        }
    }
    let _restore = Restore {
        handle,
        mode: is_console.then_some(mode),
    };
    if is_console {
        unsafe { SetConsoleMode(handle, CONSOLE_MODE(mode.0 & !ENABLE_ECHO_INPUT.0)) }
            .map_err(|e| OutputError(e.to_string()))?;
        let mut out = vec![];
        loop {
            let mut buffer = [0u16; 512];
            let mut count = 0;
            unsafe {
                ReadConsoleW(
                    handle,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut count,
                    None,
                )
            }
            .map_err(|e| OutputError(e.to_string()))?;
            if count == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..count as usize]);
            if out.last() == Some(&10) {
                break;
            }
        }
        while out.last().is_some_and(|u| *u == 10 || *u == 13) {
            out.pop();
        }
        Ok(Some(WideText(out)))
    } else {
        let mut out = vec![];
        loop {
            let mut byte = [0];
            let mut count = 0;
            match unsafe { ReadFile(handle, Some(&mut byte), Some(&mut count), None) } {
                Ok(()) => {}
                Err(e) if e.code().0 as u32 == 0x8007006d => break,
                Err(e) => return Err(OutputError(e.to_string())),
            }
            if count == 0 || byte[0] == 10 {
                break;
            }
            if byte[0] != 13 {
                out.push(byte[0]);
            }
        }
        use windows::Win32::Globalization::MultiByteToWideChar;
        let flags = windows::Win32::Globalization::MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
        let count = unsafe { MultiByteToWideChar(GetACP(), flags, &out, None) };
        let mut text = vec![0; count as usize];
        unsafe { MultiByteToWideChar(GetACP(), flags, &out, Some(&mut text)) };
        Ok(Some(WideText(text)))
    }
}
