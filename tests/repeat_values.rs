use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::{EntryMode, ManagementError, ManagementResult, ObjectSet, WideText},
    render::OutputError,
    session::{CommandOutcome, OutputSink, OutputStream, RepeatScheduler, Session},
};

#[derive(Default)]
struct Backend(usize);

impl Management for Backend {
    fn execute(&mut self, _: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        self.0 += 1;
        Ok(ManagementResult::Objects(ObjectSet::default()))
    }
}

#[derive(Default)]
struct Sink(WideText);

impl OutputSink for Sink {
    fn write(&mut self, stream: OutputStream, text: &WideText) -> Result<(), OutputError> {
        if stream == OutputStream::Stderr {
            self.0.0.extend(&text.0);
        }
        Ok(())
    }
}

struct Clock {
    waits: Vec<u32>,
    cancel_on_wait: bool,
    cancelled: bool,
}

impl RepeatScheduler for Clock {
    fn cancelled(&self) -> bool {
        self.cancelled
    }

    fn wait(&mut self, seconds: u32) -> bool {
        assert!(self.waits.is_empty(), "the test must never wait twice");
        self.waits.push(seconds);
        self.cancelled = self.cancel_on_wait;
        !self.cancelled
    }
}

fn run(arguments: &str, cancel_on_wait: bool) -> (CommandOutcome, Backend, Sink, Clock) {
    let mut backend = Backend::default();
    let mut sink = Sink::default();
    let mut clock = Clock {
        waits: vec![],
        cancel_on_wait,
        cancelled: false,
    };
    let outcome = Session::new("HOST".into()).unwrap().execute_with_scheduler(
        &arguments.into(),
        EntryMode::Interactive,
        &mut backend,
        &mut sink,
        &mut clock,
    );
    (outcome, backend, sink, clock)
}

fn interval(value: &str, expected: u32) {
    let (outcome, backend, sink, clock) = run(
        &format!(r#"os get Caption /every:"{value}" /repeat:2"#),
        false,
    );
    assert_eq!(outcome.exit_status, 0, "{value}: {:?}", sink.0);
    assert_eq!(backend.0, 2, "{value}");
    assert_eq!(clock.waits, vec![expected], "{value}");
}

fn count(value: &str) {
    let (outcome, backend, sink, clock) = run(
        &format!(r#"os get Caption /every:0 /repeat:"{value}""#),
        false,
    );
    assert_eq!(outcome.exit_status, 0, "{value}: {:?}", sink.0);
    assert_eq!(backend.0, 2, "{value}");
    assert_eq!(clock.waits, vec![0], "{value}");
}

// Values are established by passes-13-14 and the original ARM64 WMIC wcstoul
// handler, ev_9048db06ded9b622ddce5596ba874da1ad5a9626957a836a3f7b19b70b6bf838.
#[test]
fn every_accepts_leading_space() {
    interval(" 0", 0);
}

#[test]
fn an_unquoted_minus_is_a_native_token_separator() {
    for (arguments, status) in [
        ("os get Caption /every:-1 /repeat:1", 0xACAE),
        ("os get Caption /every:0 /repeat:-4294967294", 0xABE8),
    ] {
        let (outcome, backend, _, clock) = run(arguments, true);
        assert_eq!(outcome.exit_status, status);
        assert_eq!(backend.0, 0);
        assert!(clock.waits.is_empty());
    }
}

#[test]
fn every_accepts_the_quoted_empty_value_as_zero() {
    interval("", 0);
}

#[test]
fn every_accepts_fullwidth_digits() {
    interval("１２", 12);
}

#[test]
fn every_converts_negative_values_as_unsigned_32_bit() {
    interval("-1", u32::MAX);
}

#[test]
fn every_saturates_unsigned_32_bit_overflow() {
    interval("4294967296", u32::MAX);
}

#[test]
fn repeat_accepts_leading_space() {
    count(" 2");
}

#[test]
fn repeat_accepts_fullwidth_digits() {
    count("２");
}

#[test]
fn repeat_negates_an_in_range_unsigned_magnitude() {
    count("-4294967294");
}

#[test]
fn enormous_repeat_counts_remain_cancellable_without_real_waits() {
    for value in ["4294967296", "-1"] {
        let (outcome, backend, sink, clock) = run(
            &format!(r#"os get Caption /every:0 /repeat:"{value}""#),
            true,
        );
        assert_eq!(outcome.exit_status, 0xC000013A, "{value}: {:?}", sink.0);
        assert_eq!(backend.0, 1, "{value}");
        assert_eq!(clock.waits, vec![0], "{value}");
    }
}

#[test]
fn numeric_values_must_end_after_the_last_digit() {
    for (switch, code) in [("every", 0xACAE), ("repeat", 0xABE8)] {
        for value in ["2 ", "2ms", "0.0", "x", " ", "+", "-"] {
            let arguments = if switch == "every" {
                format!(r#"os get Caption /every:"{value}" /repeat:1"#)
            } else {
                format!(r#"os get Caption /every:0 /repeat:"{value}""#)
            };
            let (outcome, backend, _, clock) = run(&arguments, false);
            assert_eq!(outcome.exit_status, code, "{switch}:{value}");
            assert_eq!(backend.0, 0, "{switch}:{value}");
            assert!(clock.waits.is_empty(), "{switch}:{value}");
        }
    }
}

#[test]
fn repeat_must_immediately_follow_every() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 /value /repeat:2", false);
    assert_eq!(outcome.exit_status, 0xB0A6, "{:?}", sink.0);
    assert_eq!(backend.0, 0);
    assert!(clock.waits.is_empty());
}

#[test]
fn a_duplicate_repeat_does_not_follow_every() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 /repeat:1 /repeat:2", false);
    assert_eq!(outcome.exit_status, 0xB0A6, "{:?}", sink.0);
    assert_eq!(backend.0, 0);
    assert!(clock.waits.is_empty());
}

#[test]
fn an_earlier_numeric_error_precedes_a_later_repeat_order_error() {
    for (arguments, expected) in [
        ("os get Caption /every:x /value /repeat:2", 0xACAE),
        ("os get Caption /every:0 /repeat:x /repeat:2", 0xABE8),
    ] {
        let (outcome, backend, sink, clock) = run(arguments, false);
        assert_eq!(outcome.exit_status, expected, "{arguments}: {:?}", sink.0);
        assert_eq!(backend.0, 0);
        assert!(clock.waits.is_empty());
    }
}

#[test]
fn the_last_every_and_repeat_pair_controls_the_count() {
    let (outcome, backend, sink, clock) = run(
        "os get Caption /every:0 /repeat:1 /every:0 /repeat:2",
        false,
    );
    assert_eq!(outcome.exit_status, 0, "{:?}", sink.0);
    assert_eq!(backend.0, 2);
    assert_eq!(clock.waits, vec![0]);
}

#[test]
fn a_later_every_clears_the_earlier_repeat_limit() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 /repeat:1 /every:0", true);
    assert_eq!(outcome.exit_status, 0xC000013A, "{:?}", sink.0);
    assert_eq!(backend.0, 1);
    assert_eq!(clock.waits, vec![0]);
}

#[test]
fn every_requires_a_slash_before_its_nested_repeat() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 -repeat:1", false);
    assert_eq!(outcome.exit_status, 0xABE5, "{:?}", sink.0);
    assert_eq!(backend.0, 0);
    assert!(clock.waits.is_empty());
}

#[test]
fn formatting_after_a_valid_repeat_preserves_its_count() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 /repeat:2 /value", false);
    assert_eq!(outcome.exit_status, 0, "{:?}", sink.0);
    assert_eq!(backend.0, 2);
    assert_eq!(clock.waits, vec![0]);
}

#[test]
fn hyphen_value_after_a_valid_repeat_preserves_its_count() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 /repeat:2 -value", false);
    assert_eq!(outcome.exit_status, 0, "{:?}", sink.0);
    assert_eq!(backend.0, 2);
    assert_eq!(clock.waits, vec![0]);
}

#[test]
fn every_allows_zero_but_repeat_rejects_zero() {
    interval("0", 0);
    for value in ["0", ""] {
        let (outcome, backend, _, clock) = run(
            &format!(r#"os get Caption /every:0 /repeat:"{value}""#),
            false,
        );
        assert_eq!(outcome.exit_status, 0xABE8);
        assert_eq!(backend.0, 0);
        assert!(clock.waits.is_empty());
    }
}

// syntax-followups/reference distinguishes missing switches from empty colons.
#[test]
fn missing_every_reports_the_switch_error() {
    let (outcome, backend, sink, clock) = run("os get Caption /every", false);
    assert_eq!(outcome.exit_status, 0xADD9);
    assert_eq!(sink.0, WideText::from("Invalid EVERY switch.\r\r\n"));
    assert_eq!(backend.0, 0);
    assert!(clock.waits.is_empty());
}

#[test]
fn missing_repeat_reports_the_switch_error() {
    let (outcome, backend, sink, clock) = run("os get Caption /every:0 /repeat", false);
    assert_eq!(outcome.exit_status, 0xABE6);
    assert_eq!(sink.0, WideText::from("Invalid REPEAT switch.\r\r\n"));
    assert_eq!(backend.0, 0);
    assert!(clock.waits.is_empty());
}

#[test]
fn bare_numeric_colons_report_the_value_error() {
    for (arguments, code, message) in [
        (
            "os get Caption /every:",
            0xACAE,
            "Invalid interval value specified with /EVERY switch.\r\r\n",
        ),
        (
            "os get Caption /every:0 /repeat:",
            0xABE8,
            "Invalid REPEAT count.\r\r\n",
        ),
    ] {
        let (outcome, backend, sink, clock) = run(arguments, false);
        assert_eq!(outcome.exit_status, code, "{arguments}");
        assert_eq!(sink.0, WideText::from(message), "{arguments}");
        assert_eq!(backend.0, 0);
        assert!(clock.waits.is_empty());
    }
}
