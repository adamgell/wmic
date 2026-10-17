//! Power tests inject every privilege check and operating-system action. Never use Native here.
use super::{Action, Request, Runtime, request};
use std::{
    cell::{Cell, RefCell},
    ffi::c_int,
    io,
    os::unix::process::ExitStatusExt,
    process::ExitStatus,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Shutdown(String, Vec<String>),
    Force(c_int),
}

pub(crate) struct FakeRuntime {
    pub root: bool,
    pub privilege_checks: Cell<usize>,
    pub calls: RefCell<Vec<Call>>,
    pub error: Option<i32>,
    pub exit_status: i32,
}

impl FakeRuntime {
    pub fn new(root: bool) -> Self {
        Self {
            root,
            privilege_checks: Cell::new(0),
            calls: RefCell::new(vec![]),
            error: None,
            exit_status: 0,
        }
    }

    fn result(&self) -> io::Result<()> {
        self.error
            .map_or(Ok(()), |error| Err(io::Error::from_raw_os_error(error)))
    }
}

impl Runtime for FakeRuntime {
    fn is_root(&self) -> bool {
        self.privilege_checks.set(self.privilege_checks.get() + 1);
        self.root
    }

    fn shutdown(&self, program: &str, arguments: &[&str]) -> io::Result<ExitStatus> {
        self.calls.borrow_mut().push(Call::Shutdown(
            program.to_owned(),
            arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
        ));
        self.result()
            .map(|()| ExitStatus::from_raw(self.exit_status))
    }

    fn force_reboot(&self, how_to: c_int) -> io::Result<()> {
        self.calls.borrow_mut().push(Call::Force(how_to));
        self.result()
    }
}

#[test]
fn ordinary_restart_uses_shutdown_r_now() {
    let runtime = FakeRuntime::new(true);
    assert_eq!(
        request(
            Request {
                action: Action::Restart,
                force: false
            },
            &runtime
        ),
        0
    );
    assert_eq!(
        *runtime.calls.borrow(),
        [Call::Shutdown(
            "/sbin/shutdown".into(),
            vec!["-r".into(), "now".into()]
        )]
    );
}

#[test]
fn ordinary_halt_uses_shutdown_h_now() {
    let runtime = FakeRuntime::new(true);
    assert_eq!(
        request(
            Request {
                action: Action::Halt,
                force: false
            },
            &runtime
        ),
        0
    );
    assert_eq!(
        *runtime.calls.borrow(),
        [Call::Shutdown(
            "/sbin/shutdown".into(),
            vec!["-h".into(), "now".into()]
        )]
    );
}

#[test]
fn forced_restart_uses_only_kernel_autoboot() {
    let runtime = FakeRuntime::new(true);
    assert_eq!(
        request(
            Request {
                action: Action::Restart,
                force: true
            },
            &runtime
        ),
        0
    );
    assert_eq!(*runtime.calls.borrow(), [Call::Force(0)]);
}

#[test]
fn forced_halt_uses_only_kernel_halt() {
    let runtime = FakeRuntime::new(true);
    assert_eq!(
        request(
            Request {
                action: Action::Halt,
                force: true
            },
            &runtime
        ),
        0
    );
    assert_eq!(*runtime.calls.borrow(), [Call::Force(8)]);
}

#[test]
fn privilege_denial_performs_zero_actions_for_every_request() {
    for action in [Action::Restart, Action::Halt] {
        for force in [false, true] {
            let runtime = FakeRuntime::new(false);
            assert_eq!(request(Request { action, force }, &runtime), 5);
            assert_eq!(runtime.privilege_checks.get(), 1);
            assert!(runtime.calls.borrow().is_empty());
        }
    }
}

#[test]
fn shutdown_nonzero_exit_or_signal_is_failure_without_forced_fallback() {
    for (status, expected) in [(0, 0), (1 << 8, 8), (77 << 8, 8), (9, 8)] {
        let mut runtime = FakeRuntime::new(true);
        runtime.exit_status = status;
        assert_eq!(
            request(
                Request {
                    action: Action::Restart,
                    force: false
                },
                &runtime
            ),
            expected
        );
        assert_eq!(
            *runtime.calls.borrow(),
            [Call::Shutdown(
                "/sbin/shutdown".into(),
                vec!["-r".into(), "now".into()]
            )]
        );
    }
}

#[test]
fn shutdown_spawn_or_wait_error_maps_privilege_and_other_failures_without_fallback() {
    for (error, expected) in [
        (libc::EPERM, 5),
        (libc::EACCES, 5),
        (libc::ENOENT, 8),
        (libc::EIO, 8),
    ] {
        let mut runtime = FakeRuntime::new(true);
        runtime.error = Some(error);
        assert_eq!(
            request(
                Request {
                    action: Action::Halt,
                    force: false
                },
                &runtime
            ),
            expected
        );
        assert_eq!(
            *runtime.calls.borrow(),
            [Call::Shutdown(
                "/sbin/shutdown".into(),
                vec!["-h".into(), "now".into()]
            )]
        );
    }
}

#[test]
fn forced_errors_map_privilege_and_other_failures() {
    for (error, expected) in [(libc::EPERM, 5), (libc::EACCES, 5), (libc::EIO, 8)] {
        let mut runtime = FakeRuntime::new(true);
        runtime.error = Some(error);
        assert_eq!(
            request(
                Request {
                    action: Action::Restart,
                    force: true
                },
                &runtime
            ),
            expected
        );
        assert_eq!(*runtime.calls.borrow(), [Call::Force(0)]);
    }
}
