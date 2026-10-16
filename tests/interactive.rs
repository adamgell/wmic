mod support;
use std::collections::VecDeque;
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::OutputError,
    session::*,
};
struct Backend;
impl Management for Backend {
    fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        Ok(ManagementResult::Objects(ObjectSet::default()))
    }
}
struct Input(VecDeque<InputEvent>);
impl InputSource for Input {
    fn next_input(&mut self) -> Result<InputEvent, InputError> {
        Ok(self.0.pop_front().unwrap_or(InputEvent::Eof))
    }
}
#[derive(Default)]
struct Sink {
    out: WideText,
    err: WideText,
}
impl OutputSink for Sink {
    fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
        if s == OutputStream::Stdout {
            self.out.0.extend(&t.0)
        } else {
            self.err.0.extend(&t.0)
        }
        Ok(())
    }
}
#[test]
fn redirected_shell_eof_has_one_prompt_and_zero_status() {
    let mut session = Session::new("HOST".into()).unwrap();
    let mut sink = Sink::default();
    let outcome = run_interactive(
        &mut session,
        &mut Backend,
        &mut Input(VecDeque::new()),
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0);
    assert_eq!(sink.out, WideText::from("wmic:root\\cli>"));
}
#[test]
fn inline_setting_changes_persist_and_quit_resets_last_error_status() {
    let mut session = Session::new("HOST".into()).unwrap();
    let mut sink = Sink::default();
    let mut input = Input(
        ["/aggregate:off CONTEXT", "Missing", "QUIT"]
            .into_iter()
            .map(|s| InputEvent::Line {
                line: s.into(),
                console: false,
            })
            .collect(),
    );
    let outcome = run_interactive(&mut session, &mut Backend, &mut input, &mut sink);
    assert_eq!(outcome.exit_status, 0);
    assert!(!session.settings.aggregate);
    assert!(
        sink.out
            .to_string_lossy()
            .contains("AGGREGATE             : OFF")
    );
    assert!(
        sink.err
            .to_string_lossy()
            .contains("Missing - Alias not found.")
    );
}
#[test]
fn eof_after_error_retains_last_status() {
    let mut session = Session::new("HOST".into()).unwrap();
    let mut sink = Sink::default();
    let outcome = run_interactive(
        &mut session,
        &mut Backend,
        &mut Input(
            [InputEvent::Line {
                line: "Missing".into(),
                console: false,
            }]
            .into(),
        ),
        &mut sink,
    );
    assert_eq!(outcome.exit_status, 0xAC67);
}
#[test]
fn repeat_count_bounds_execution_without_waiting_after_last_pass() {
    struct Clock(u32);
    impl RepeatScheduler for Clock {
        fn wait(&mut self, _: u32) -> bool {
            self.0 += 1;
            true
        }
    }
    let mut clock = Clock(0);
    let mut session = Session::new("HOST".into()).unwrap();
    let mut sink = Sink::default();
    let result = session.execute_with_scheduler(
        &"os get Caption /every:0 /repeat:2".into(),
        EntryMode::Interactive,
        &mut Backend,
        &mut sink,
        &mut clock,
    );
    assert_eq!(result.exit_status, 0);
    assert_eq!(clock.0, 1);
}

#[test]
fn cancellation_during_interval_retains_control_c_status() {
    struct Clock(bool);
    impl RepeatScheduler for Clock {
        fn cancelled(&self) -> bool {
            self.0
        }
        fn wait(&mut self, _: u32) -> bool {
            self.0 = true;
            false
        }
    }
    let result = Session::new("HOST".into()).unwrap().execute_with_scheduler(
        &"os get Caption /every:1".into(),
        EntryMode::Interactive,
        &mut Backend,
        &mut Sink::default(),
        &mut Clock(false),
    );
    assert_eq!(result.exit_status, 0xC000013A);
}

#[test]
fn cancellation_between_nodes_keeps_completed_node_and_skips_the_next() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Clock(Arc<AtomicBool>);
    impl RepeatScheduler for Clock {
        fn cancelled(&self) -> bool {
            self.0.load(Ordering::Relaxed)
        }
        fn wait(&mut self, _: u32) -> bool {
            panic!("no repetition requested")
        }
    }
    struct Provider {
        flag: Arc<AtomicBool>,
        calls: usize,
    }
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            self.calls += 1;
            self.flag.store(true, Ordering::Relaxed);
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    let flag = Arc::new(AtomicBool::new(false));
    let mut provider = Provider {
        flag: flag.clone(),
        calls: 0,
    };
    let mut clock = Clock(flag);
    let out = Session::new("HOST".into()).unwrap().execute_with_scheduler(
        &"/node:one,two os get Caption".into(),
        EntryMode::Interactive,
        &mut provider,
        &mut Sink::default(),
        &mut clock,
    );
    assert_eq!(out.exit_status, 0xC000013A);
    assert_eq!(provider.calls, 1);
    assert_eq!(out.nodes.len(), 1);
}

#[test]
fn repetition_applies_global_changes_and_password_prompt_once() {
    struct Clock(bool);
    impl RepeatScheduler for Clock {
        fn wait(&mut self, _: u32) -> bool {
            let old = self.0;
            self.0 = false;
            old
        }
    }
    struct Provider;
    impl Management for Provider {
        fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            assert!(r.connection.password.is_some());
            assert_eq!(
                r.render_context
                    .as_ref()
                    .unwrap()
                    .settings
                    .iter()
                    .find(|(n, _)| n.eq_ignore_case("namespace"))
                    .unwrap()
                    .1,
                WideText::from("root\\cimv2\\relative")
            );
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    #[derive(Default)]
    struct SecretSink(usize);
    impl OutputSink for SecretSink {
        fn write(&mut self, _: OutputStream, _: &WideText) -> Result<(), OutputError> {
            Ok(())
        }
        fn read_secret(&mut self) -> Result<Option<WideText>, OutputError> {
            self.0 += 1;
            Ok(Some("test-only".into()))
        }
    }
    let mut sink = SecretSink::default();
    let out = Session::new("HOST".into()).unwrap().execute_with_scheduler(
        &"/namespace:relative /user:sample os get Caption /every:0".into(),
        EntryMode::Interactive,
        &mut Provider,
        &mut sink,
        &mut Clock(true),
    );
    assert_eq!(out.exit_status, 0);
    assert_eq!(sink.0, 1);
}

#[test]
fn cancellation_during_the_only_backend_call_returns_control_c() {
    use std::{cell::Cell, rc::Rc};
    struct Clock(Rc<Cell<bool>>);
    impl RepeatScheduler for Clock {
        fn cancelled(&self) -> bool {
            self.0.get()
        }
        fn wait(&mut self, _: u32) -> bool {
            panic!("one-shot must not wait")
        }
    }
    struct Provider(Rc<Cell<bool>>);
    impl Management for Provider {
        fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
            self.0.set(true);
            Ok(ManagementResult::Objects(ObjectSet::default()))
        }
    }
    let flag = Rc::new(Cell::new(false));
    let out = Session::new("HOST".into()).unwrap().execute_with_scheduler(
        &"os get Caption".into(),
        EntryMode::Interactive,
        &mut Provider(flag.clone()),
        &mut Sink::default(),
        &mut Clock(flag),
    );
    assert_eq!(out.exit_status, 0xC000013A);
    assert_eq!(out.nodes.len(), 1);
}
