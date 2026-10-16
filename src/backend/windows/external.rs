//! Invoke the explicitly selected metadata derivation with native launch semantics.
use super::*;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{Console::*, Threading::*},
    },
    core::*,
};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub(super) fn launch(command: &WideText) -> std::result::Result<u32, ManagementError> {
    let action = || -> Result<u32> {
        unsafe {
            let mut raw: Vec<u16> = command.0.iter().copied().chain([0]).collect();
            let startup = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: GetStdHandle(STD_INPUT_HANDLE)?,
                hStdOutput: GetStdHandle(STD_OUTPUT_HANDLE)?,
                hStdError: GetStdHandle(STD_ERROR_HANDLE)?,
                ..Default::default()
            };
            let mut process = PROCESS_INFORMATION::default();
            CreateProcessW(
                None,
                Some(PWSTR(raw.as_mut_ptr())),
                None,
                None,
                true,
                PROCESS_CREATION_FLAGS(0),
                None,
                None,
                &startup,
                &mut process,
            )?;
            let process_handle = Handle(process.hProcess);
            let _thread = Handle(process.hThread);
            if WaitForSingleObject(process_handle.0, u32::MAX)
                != windows::Win32::Foundation::WAIT_OBJECT_0
            {
                return Err(Error::from_thread());
            }
            let mut code = 0;
            GetExitCodeProcess(process_handle.0, &mut code)?;
            Ok(code)
        }
    };
    action().map_err(|e| ManagementError {
        code: e.code().0 as u32,
        target: command.clone(),
        message: e.message().into(),
        operation: "external".into(),
        extended: vec![],
        partial: None,
    })
}
#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    #[ignore = "requires native Windows process creation"]
    fn external_process_preserves_full_exit_status() {
        assert_eq!(
            launch(&WideText::from("cmd.exe /d /c exit -2147217406")).unwrap(),
            0x80041002
        );
    }
}
