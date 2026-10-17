//! Detached process creation without a shell.
use std::{
    ffi::CString,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

const POSIX_SPAWN_SETSIGDEF: libc::c_short = 0x0004;
const POSIX_SPAWN_SETSIGMASK: libc::c_short = 0x0008;
const POSIX_SPAWN_SETSID: libc::c_short = 0x0400;
const POSIX_SPAWN_CLOEXEC_DEFAULT: libc::c_short = 0x4000;

fn c_string(text: &str) -> Result<CString, i32> {
    CString::new(text).map_err(|_| libc::EINVAL)
}

/// Program path as `posix_spawnp` must receive it. A relative path with a separator names a
/// file relative to the caller; the child's working directory changes before the lookup, so the
/// path is made absolute first. Bare names stay for the `PATH` search.
pub fn program_path(program: &str, base: &Path) -> PathBuf {
    let path = Path::new(program);
    if program.contains('/') && !path.is_absolute() {
        base.join(path)
    } else {
        path.to_owned()
    }
}

/// Waits for a child so that it does not stay a zombie. Only this child is waited for: a
/// process-wide `SIGCHLD` disposition or `waitpid(-1)` would take the status of unrelated
/// children.
fn reap(pid: libc::pid_t) {
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, 0) };
        if result >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

/// Starts `arguments[0]`, searched in `PATH`, in a new session with standard streams on
/// `/dev/null` and no inherited descriptors. Returns the process id, or the `errno` that
/// `posix_spawnp` reported. A detached thread reaps the child when it exits.
pub fn spawn(arguments: &[String], directory: Option<&str>) -> Result<i32, i32> {
    let owned = arguments
        .iter()
        .map(|a| c_string(a))
        .collect::<Result<Vec<_>, _>>()?;
    let name = arguments.first().ok_or(libc::EINVAL)?;
    let resolved = match std::env::current_dir() {
        Ok(base) => program_path(name, &base),
        Err(_) => PathBuf::from(name),
    };
    let program = CString::new(resolved.as_os_str().as_bytes()).map_err(|_| libc::EINVAL)?;
    let mut argv: Vec<*mut libc::c_char> = owned.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let directory = directory.map(c_string).transpose()?;
    let null = c_string("/dev/null")?;

    let mut attributes = unsafe { std::mem::zeroed::<libc::posix_spawnattr_t>() };
    let mut actions = unsafe { std::mem::zeroed::<libc::posix_spawn_file_actions_t>() };
    unsafe {
        libc::posix_spawnattr_init(&mut attributes);
        libc::posix_spawn_file_actions_init(&mut actions);
        let mut defaults = std::mem::zeroed::<libc::sigset_t>();
        libc::sigfillset(&mut defaults);
        libc::posix_spawnattr_setsigdefault(&mut attributes, &defaults);
        let mut mask = std::mem::zeroed::<libc::sigset_t>();
        libc::sigemptyset(&mut mask);
        libc::posix_spawnattr_setsigmask(&mut attributes, &mask);
        libc::posix_spawnattr_setflags(
            &mut attributes,
            POSIX_SPAWN_SETSIGDEF
                | POSIX_SPAWN_SETSIGMASK
                | POSIX_SPAWN_SETSID
                | POSIX_SPAWN_CLOEXEC_DEFAULT,
        );
        libc::posix_spawn_file_actions_addopen(&mut actions, 0, null.as_ptr(), libc::O_RDONLY, 0);
        libc::posix_spawn_file_actions_addopen(&mut actions, 1, null.as_ptr(), libc::O_WRONLY, 0);
        libc::posix_spawn_file_actions_addopen(&mut actions, 2, null.as_ptr(), libc::O_WRONLY, 0);
        if let Some(directory) = &directory {
            libc::posix_spawn_file_actions_addchdir_np(&mut actions, directory.as_ptr());
        }
    }
    let mut pid: libc::pid_t = 0;
    // SAFETY: `argv` and the strings it points to outlive the call; `_NSGetEnviron` returns the
    // process environment block.
    let status = unsafe {
        libc::posix_spawnp(
            &mut pid,
            program.as_ptr(),
            &actions,
            &attributes,
            argv.as_ptr(),
            *libc::_NSGetEnviron(),
        )
    };
    unsafe {
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attributes);
    }
    if status != 0 {
        return Err(status);
    }
    // A failed thread start leaves a zombie until the process exits, which does not fail the call.
    let _ = std::thread::Builder::new().spawn(move || reap(pid));
    Ok(pid)
}
