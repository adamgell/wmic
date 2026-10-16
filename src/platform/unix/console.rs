use std::cell::UnsafeCell;
use std::io;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static KEY_WATCH: AtomicBool = AtomicBool::new(false);
const SIGNALS: [libc::c_int; 2] = [libc::SIGINT, libc::SIGQUIT];

/// Terminal attributes that a fatal signal handler restores. The slot is written only while
/// `HOLDERS` is zero, before `READY` is set, so the handler never reads a partial value.
struct SavedTerminal(UnsafeCell<MaybeUninit<libc::termios>>);
unsafe impl Sync for SavedTerminal {}
static SAVED: SavedTerminal = SavedTerminal(UnsafeCell::new(MaybeUninit::zeroed()));
static READY: AtomicBool = AtomicBool::new(false);
static HOLDERS: AtomicUsize = AtomicUsize::new(0);

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Restores the terminal, then re-raises the signal with its default action so the parent
/// sees death by signal. Only async-signal-safe calls run here.
fn die_by(signal: libc::c_int) {
    if READY.load(Ordering::Acquire) {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, (*SAVED.0.get()).as_ptr()) };
    }
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

extern "C" fn handler(signal: libc::c_int) {
    if INTERRUPTED.swap(true, Ordering::Relaxed) {
        // A second request ends a process that is blocked in a call that does not poll the flag.
        die_by(signal);
    }
}

extern "C" fn terminate(signal: libc::c_int) {
    die_by(signal);
}

/// Keeps the saved terminal attributes available to the signal handlers while a guard that
/// changed the terminal mode is alive, and ends the process by SIGTERM only after restoring them.
pub(crate) struct TerminalHold {
    previous_term: Option<libc::sigaction>,
}
impl TerminalHold {
    pub(crate) fn acquire(saved: &libc::termios) -> Self {
        if HOLDERS.fetch_add(1, Ordering::AcqRel) == 0 {
            unsafe { (*SAVED.0.get()).write(*saved) };
            READY.store(true, Ordering::Release);
        }
        Self {
            previous_term: install(libc::SIGTERM, terminate),
        }
    }
}
impl Drop for TerminalHold {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous_term {
            unsafe { libc::sigaction(libc::SIGTERM, previous, std::ptr::null_mut()) };
        }
        if HOLDERS.fetch_sub(1, Ordering::AcqRel) == 1 {
            READY.store(false, Ordering::Release);
        }
    }
}

/// Installs `handler` without SA_RESTART so a blocked read returns EINTR. Returns the previous
/// action, or `None` when the signal was ignored on entry or the call failed; an ignored
/// signal stays ignored.
fn install(signal: libc::c_int, handler: extern "C" fn(libc::c_int)) -> Option<libc::sigaction> {
    let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(signal, std::ptr::null(), &mut previous) } != 0
        || previous.sa_sigaction == libc::SIG_IGN
    {
        return None;
    }
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = handler as libc::sighandler_t;
    action.sa_flags = 0;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    (unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0).then_some(previous)
}

/// Installs the cancellation handler for SIGINT and SIGQUIT.
pub struct InterruptGuard {
    previous: [Option<libc::sigaction>; 2],
}
impl InterruptGuard {
    pub fn install() -> io::Result<Self> {
        INTERRUPTED.store(false, Ordering::Relaxed);
        Ok(Self {
            previous: SIGNALS.map(|signal| install(signal, handler)),
        })
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        for (signal, previous) in SIGNALS.iter().zip(&self.previous) {
            if let Some(previous) = previous {
                unsafe { libc::sigaction(*signal, previous, std::ptr::null_mut()) };
            }
        }
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
    _hold: TerminalHold,
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
        let hold = TerminalHold::acquire(&saved);
        let mut raw = saved;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        KEY_WATCH.store(true, Ordering::Relaxed);
        Some(Self { saved, _hold: hold })
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

    fn in_child(body: impl FnOnce()) -> libc::c_int {
        match unsafe { libc::fork() } {
            0 => {
                body();
                unsafe { libc::_exit(99) }
            }
            pid if pid > 0 => {
                let mut status = 0;
                unsafe { libc::waitpid(pid, &mut status, 0) };
                status
            }
            _ => panic!("fork failed"),
        }
    }

    #[test]
    fn a_second_interrupt_ends_the_process_by_the_signal() {
        let status = in_child(|| {
            let _guard = InterruptGuard::install();
            unsafe {
                libc::raise(libc::SIGINT);
                libc::raise(libc::SIGINT);
            }
        });
        assert!(libc::WIFSIGNALED(status), "status {status:#x}");
        assert_eq!(libc::WTERMSIG(status), libc::SIGINT);
    }

    #[test]
    fn terminate_while_the_terminal_is_held_ends_the_process_by_the_signal() {
        let status = in_child(|| {
            let mut saved = unsafe { std::mem::zeroed::<libc::termios>() };
            unsafe { libc::tcgetattr(0, &mut saved) };
            let _hold = TerminalHold::acquire(&saved);
            unsafe { libc::raise(libc::SIGTERM) };
        });
        assert!(libc::WIFSIGNALED(status), "status {status:#x}");
        assert_eq!(libc::WTERMSIG(status), libc::SIGTERM);
    }

    #[test]
    fn an_ignored_signal_stays_ignored() {
        let status = in_child(|| {
            unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
            let guard = InterruptGuard::install();
            let mut current = unsafe { std::mem::zeroed::<libc::sigaction>() };
            unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current) };
            let kept = current.sa_sigaction == libc::SIG_IGN;
            drop(guard);
            unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current) };
            let restored = current.sa_sigaction == libc::SIG_IGN;
            unsafe { libc::_exit(if kept && restored { 0 } else { 1 }) }
        });
        assert!(libc::WIFEXITED(status), "status {status:#x}");
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }
}
