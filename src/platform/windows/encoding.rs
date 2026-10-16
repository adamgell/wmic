use crate::{model::WideText, render::OutputError};
use windows::{Win32::Globalization::WideCharToMultiByte, core::PCSTR};
pub fn encode_codepage(text: &WideText, code_page: u32) -> Result<Vec<u8>, OutputError> {
    let size = unsafe { WideCharToMultiByte(code_page, 0, &text.0, None, PCSTR::null(), None) };
    if size == 0 && !text.0.is_empty() {
        return Err(OutputError("Code page conversion failed".into()));
    }
    let mut out = vec![0; size as usize];
    let written =
        unsafe { WideCharToMultiByte(code_page, 0, &text.0, Some(&mut out), PCSTR::null(), None) };
    if written != size {
        return Err(OutputError("Code page conversion failed".into()));
    }
    Ok(out)
}
