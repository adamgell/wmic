mod support;
use support::wide;
use wmic_compat::{
    backend::{Management, ResolvedRequest},
    model::*,
    render::{DestinationProfile, OutputError, encode_output},
    session::{OutputSink, OutputStream, Session},
};
struct MissingClass;
impl Management for MissingClass {
    fn execute(&mut self, r: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        Err(ManagementError {
            code: 0x80041002,
            target: r.connection.node.clone(),
            message: wide("Not found"),
            operation: wide("class"),
            extended: vec![],
            partial: None,
        })
    }
}
#[derive(Default)]
struct Sink {
    stderr: Vec<u8>,
}
impl OutputSink for Sink {
    fn write(&mut self, s: OutputStream, t: &WideText) -> Result<(), OutputError> {
        if s == OutputStream::Stderr {
            self.stderr
                .extend(encode_output(t, &DestinationProfile::pipe(1252))?);
        }
        Ok(())
    }
}
#[test]
fn full_hresult_survives_controller() {
    let mut sink = Sink::default();
    let out = Session::new(wide("TEST")).unwrap().execute(
        &wide("path WmicResearchClassDoesNotExist get Name"),
        EntryMode::Interactive,
        &mut MissingClass,
        &mut sink,
    );
    assert_eq!(out.exit_status, 0x80041002);
    assert_eq!(
        sink.stderr,
        include_bytes!("fixtures/reference/error-class.stderr.bin")
    );
}
