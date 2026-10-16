//! Console cancellation does not impose a timeout on a WMI operation.
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{Win32::System::Console::*, core::BOOL};
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}
unsafe extern "system" fn handler(event: u32) -> BOOL {
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        INTERRUPTED.store(true, Ordering::Relaxed);
        BOOL(1)
    } else {
        BOOL(0)
    }
}
pub struct InterruptGuard;
impl InterruptGuard {
    pub fn install() -> windows::core::Result<Self> {
        INTERRUPTED.store(false, Ordering::Relaxed);
        unsafe { SetConsoleCtrlHandler(Some(handler), true)? };
        Ok(Self)
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = SetConsoleCtrlHandler(Some(handler), false);
        }
    }
}

pub fn has_input_console() -> bool {
    unsafe { GetStdHandle(STD_INPUT_HANDLE) }
        .ok()
        .is_some_and(|h| unsafe { GetConsoleMode(h, &mut CONSOLE_MODE(0)) }.is_ok())
}
pub fn key_pressed() -> bool {
    if !has_input_console() {
        return false;
    }
    let Ok(handle) = (unsafe { GetStdHandle(STD_INPUT_HANDLE) }) else {
        return false;
    };
    let mut event = [INPUT_RECORD::default()];
    let mut count = 0;
    unsafe {
        while PeekConsoleInputW(handle, &mut event, &mut count).is_ok() && count > 0 {
            if ReadConsoleInputW(handle, &mut event, &mut count).is_err() || count == 0 {
                return false;
            }
            if event[0].EventType == KEY_EVENT as u16 && event[0].Event.KeyEvent.bKeyDown.as_bool()
            {
                return true;
            }
        }
    }
    false
}

pub fn clear_wait_prompt() -> windows::core::Result<()> {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE)?;
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        GetConsoleScreenBufferInfo(handle, &mut info)?;
        let origin = COORD {
            X: 0,
            Y: info.dwCursorPosition.Y,
        };
        let mut written = 0;
        FillConsoleOutputCharacterW(handle, 32, info.dwSize.X as u32, origin, &mut written)?;
        SetConsoleCursorPosition(handle, origin)
    }
}

/// WMIC uses a wide console buffer during execution, then restores its width.
pub struct LayoutGuard {
    handle: windows::Win32::Foundation::HANDLE,
    original: COORD,
}
impl LayoutGuard {
    pub fn install() -> Option<Self> {
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE).ok()?;
            let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
            GetConsoleScreenBufferInfo(handle, &mut info).ok()?;
            let original = info.dwSize;
            SetConsoleScreenBufferSize(
                handle,
                COORD {
                    X: 1500,
                    Y: original.Y,
                },
            )
            .ok()?;
            Some(Self { handle, original })
        }
    }
}
impl Drop for LayoutGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = SetConsoleScreenBufferSize(self.handle, self.original);
        }
    }
}
