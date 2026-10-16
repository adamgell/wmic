use crate::{
    backend::windows::WindowsManagement,
    model::{EntryMode, WideText},
    session::Session,
};
use std::os::windows::ffi::OsStrExt;
use windows::Win32::System::{Environment::GetCommandLineW, Threading::ExitProcess};
pub fn run() -> ! {
    let raw = unsafe { WideText(GetCommandLineW().as_wide().to_vec()) };
    let node = std::env::var_os("COMPUTERNAME")
        .map(|s| WideText(s.encode_wide().collect()))
        .unwrap_or_else(|| ".".into());
    let status = match (Session::new(node), WindowsManagement::new()) {
        (Ok(mut session), Ok(mut backend)) => {
            let mut sink = super::output::NativeOutput;
            session
                .execute(&raw, EntryMode::Process, &mut backend, &mut sink)
                .exit_status
        }
        (Err(e), _) => e.code,
        (_, Err(e)) => e.code,
    };
    // std::process::ExitCode truncates to u8; WMIC exposes the full HRESULT DWORD.
    unsafe { ExitProcess(status) }
}
