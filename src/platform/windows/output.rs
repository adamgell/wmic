use crate::{
    model::WideText,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream},
};
use windows::Win32::{Globalization::GetACP, Storage::FileSystem::WriteFile, System::Console::*};
pub struct NativeOutput;
impl OutputSink for NativeOutput {
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
