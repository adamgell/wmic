use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static KEY_WATCH: AtomicBool = AtomicBool::new(false);
const SIGNALS: [libc::c_int; 2] = [libc::SIGINT, libc::SIGQUIT];

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

extern "C" fn handler(_signal: libc::c_int) {
    if INTERRUPTED.swap(true, Ordering::Relaxed) {
        // A second request ends a process that is blocked in a call that does not poll the flag.
        unsafe { libc::_exit(130) }
    }
}

/// Installs the cancellation handler without SA_RESTART so a blocked read returns EINTR.
pub struct InterruptGuard {
    previous: [libc::sigaction; 2],
}
impl InterruptGuard {
    pub fn install() -> io::Result<Self> {
        INTERRUPTED.store(false, Ordering::Relaxed);
        let mut previous = [unsafe { std::mem::zeroed::<libc::sigaction>() }; 2];
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = 0;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        for (index, signal) in SIGNALS.iter().enumerate() {
            if unsafe { libc::sigaction(*signal, &action, &mut previous[index]) } != 0 {
                let error = io::Error::last_os_error();
                restore(&previous[..index]);
                return Err(error);
            }
        }
        Ok(Self { previous })
    }
}
fn restore(previous: &[libc::sigaction]) {
    for (signal, action) in SIGNALS.iter().zip(previous) {
        unsafe { libc::sigaction(*signal, action, std::ptr::null_mut()) };
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        restore(&self.previous);
        INTERRUPTED.store(false, Ordering::Relaxed);
    }
}

pub fn input_is_tty() -> bool {
    unsafe { libc::isatty(0) == 1 }
}

/// Terminal mode changes from a background process group stop the process with SIGTTOU.
pub fn has_input_console() -> bool {
    input_is_tty() && unsafe { libc::tcgetpgrp(0) == libc::getpgrp() }
}

/// Switches the terminal to unbuffered, non-echoing input until dropped; ISIG stays on.
pub struct KeyWatch {
    saved: libc::termios,
}
impl KeyWatch {
    pub fn enter() -> Option<Self> {
        if !has_input_console() {
            return None;
        }
        let mut saved = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(0, &mut saved) } != 0 {
            return None;
        }
        let mut raw = saved;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        KEY_WATCH.store(true, Ordering::Relaxed);
        Some(Self { saved })
    }
}
impl Drop for KeyWatch {
    fn drop(&mut self) {
        KEY_WATCH.store(false, Ordering::Relaxed);
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.saved) };
    }
}

/// Consumes pending input and reports whether any existed; false unless a `KeyWatch` is alive.
pub fn key_pressed() -> bool {
    if !KEY_WATCH.load(Ordering::Relaxed) {
        return false;
    }
    let mut poll = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    if unsafe { libc::poll(&mut poll, 1, 0) } <= 0 || poll.revents & libc::POLLIN == 0 {
        return false;
    }
    // The buffer holds the remaining bytes of a multi-byte key such as an arrow key.
    let mut buffer = [0u8; 64];
    unsafe { libc::read(0, buffer.as_mut_ptr().cast(), buffer.len()) > 0 }
}

pub fn clear_wait_prompt() -> io::Result<()> {
    if unsafe { libc::isatty(1) } != 1 {
        return Ok(());
    }
    let dumb = std::env::var_os("TERM").is_some_and(|t| t == "dumb");
    let text = if dumb {
        let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
        let columns =
            if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut size) } == 0 && size.ws_col > 0 {
                usize::from(size.ws_col)
            } else {
                80
            };
        format!("\r{}\r", " ".repeat(columns))
    } else {
        "\r\x1b[2K".to_string()
    };
    super::output::write_fd(1, text.as_bytes())
}

#[cfg(test)]
pub(crate) fn request_cancel_for_test() {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_set_the_flag_and_the_guard_restores_state() {
        let guard = InterruptGuard::install().unwrap();
        assert!(!interrupted());
        unsafe { libc::raise(libc::SIGINT) };
        assert!(interrupted());
        drop(guard);
        assert!(!interrupted());
        let mut current = unsafe { std::mem::zeroed::<libc::sigaction>() };
        unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current) };
        assert_ne!(
            current.sa_sigaction,
            handler as extern "C" fn(libc::c_int) as libc::sighandler_t
        );
        request_cancel_for_test();
        assert!(interrupted());
        INTERRUPTED.store(false, Ordering::Relaxed);
    }
}
