use crate::{
    model::WideText,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream},
};
use windows::Win32::{Globalization::GetOEMCP, Storage::FileSystem::WriteFile, System::Console::*};
pub struct NativeOutput;
impl OutputSink for NativeOutput {
    fn consume_query_key(&mut self) -> Result<(), OutputError> {
        let _ = super::console::key_pressed();
        Ok(())
    }
    fn clear_wait_prompt(&mut self) -> Result<(), OutputError> {
        super::console::clear_wait_prompt().map_err(|e| OutputError(e.to_string()))
    }
    fn console_input(&self) -> bool {
        super::console::has_input_console()
    }
    fn clipboard(&mut self, text: &WideText) -> Result<(), OutputError> {
        write_clipboard(text)
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
        let bytes = encode_output(text, &DestinationProfile::pipe(output_code_page()))?;
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

fn read_input(secret: bool) -> Result<Option<WideText>, OutputError> {
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
        if secret {
            unsafe { SetConsoleMode(handle, CONSOLE_MODE(mode.0 & !ENABLE_ECHO_INPUT.0)) }
                .map_err(|e| OutputError(e.to_string()))?;
        }
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
        if out.is_empty() || out.first() == Some(&26) {
            return Ok(None);
        }
        while out.last().is_some_and(|u| *u == 10 || *u == 13) {
            out.pop();
        }
        Ok(Some(WideText(out)))
    } else {
        let mut out = vec![];
        let mut received = false;
        loop {
            let mut byte = [0];
            let mut count = 0;
            match unsafe { ReadFile(handle, Some(&mut byte), Some(&mut count), None) } {
                Ok(()) => {}
                Err(e) if e.code().0 as u32 == 0x8007006d => break,
                Err(e) => return Err(OutputError(e.to_string())),
            }
            if count == 0 {
                break;
            }
            received = true;
            if byte[0] == 10 {
                break;
            }
            if byte[0] != 13 {
                out.push(byte[0]);
            }
        }
        if !received {
            return Ok(None);
        }
        use windows::Win32::Globalization::MultiByteToWideChar;
        let flags = windows::Win32::Globalization::MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
        let count = unsafe { MultiByteToWideChar(output_code_page(), flags, &out, None) };
        let mut text = vec![0; count as usize];
        unsafe { MultiByteToWideChar(output_code_page(), flags, &out, Some(&mut text)) };
        Ok(Some(WideText(text)))
    }
}

fn write_clipboard(text: &WideText) -> Result<(), OutputError> {
    use windows::Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{DataExchange::*, Memory::*},
        UI::WindowsAndMessaging::*,
    };
    let result: windows::core::Result<()> = unsafe {
        (|| {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                windows::core::w!("STATIC"),
                windows::core::w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )?;
            struct Window(windows::Win32::Foundation::HWND);
            impl Drop for Window {
                fn drop(&mut self) {
                    unsafe {
                        let _ = DestroyWindow(self.0);
                    }
                }
            }
            let _window = Window(window);
            let memory = GlobalAlloc(GMEM_MOVEABLE, (text.0.len() + 1) * 2)?;
            let pointer = GlobalLock(memory).cast::<u16>();
            if pointer.is_null() {
                let _ = GlobalFree(Some(memory));
                return Err(windows::core::Error::from_thread());
            }
            std::ptr::copy_nonoverlapping(text.0.as_ptr(), pointer, text.0.len());
            *pointer.add(text.0.len()) = 0;
            let _ = GlobalUnlock(memory);
            if let Err(error) = OpenClipboard(Some(window)) {
                let _ = GlobalFree(Some(memory));
                return Err(error);
            }
            struct Close;
            impl Drop for Close {
                fn drop(&mut self) {
                    unsafe {
                        let _ = CloseClipboard();
                    }
                }
            }
            let _close = Close;
            if let Err(error) = EmptyClipboard() {
                let _ = GlobalFree(Some(memory));
                return Err(error);
            }
            if let Err(error) = SetClipboardData(13, Some(HANDLE(memory.0))) {
                let _ = GlobalFree(Some(memory));
                return Err(error);
            }
            Ok(())
        })()
    };
    result.map_err(|e| OutputError(e.to_string()))
}

fn output_code_page() -> u32 {
    let cp = unsafe { GetConsoleOutputCP() };
    if cp == 0 { unsafe { GetOEMCP() } } else { cp }
}

pub struct NativeInput;
impl crate::session::InputSource for NativeInput {
    fn next_input(&mut self) -> Result<crate::session::InputEvent, crate::session::InputError> {
        use crate::session::InputEvent;
        if super::console::interrupted() {
            return Ok(InputEvent::Interrupted);
        }
        let console = unsafe { GetStdHandle(STD_INPUT_HANDLE) }
            .ok()
            .is_some_and(|handle| unsafe { GetConsoleMode(handle, &mut CONSOLE_MODE(0)) }.is_ok());
        match read_input(false) {
            Ok(Some(line)) => Ok(InputEvent::Line { line, console }),
            Ok(None) => Ok(InputEvent::Eof),
            Err(_) if super::console::interrupted() => Ok(InputEvent::Interrupted),
            Err(e) => Err(e.into()),
        }
    }
}
