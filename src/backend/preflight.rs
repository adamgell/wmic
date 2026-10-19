use super::ConnectionOptions;
use crate::model::{ManagementError, WideText};

/// Parser metadata only: this request cannot carry a query, mutation or confirmation.
pub struct PathPreflightRequest<'a> {
    pub connection: &'a ConnectionOptions,
    pub class_name: &'a WideText,
    pub method: Option<&'a WideText>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathPreflightStage {
    Connect,
    Security,
    Class,
    Method,
    Names,
}

#[derive(Debug)]
pub struct PathPreflightError {
    pub stage: PathPreflightStage,
    /// Successfully inspected CALL signatures before a later metadata error.
    pub completed_method_signatures: u8,
    pub error: ManagementError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathPreflightOutcome {
    /// No safe metadata-only implementation is available for this transport.
    Unsupported,
    Ready {
        method_signatures: u8,
    },
}
