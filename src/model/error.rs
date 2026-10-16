use super::{ManagementResult, Property, WideText};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    pub code: u32,
    pub message: WideText,
    pub token: Option<WideText>,
}

impl ParseError {
    pub(crate) fn new(code: u32, message: &str, token: Option<WideText>) -> Self {
        Self {
            code,
            message: message.into(),
            token,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for ParseError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManagementError {
    pub code: u32,
    pub target: WideText,
    pub message: WideText,
    pub operation: WideText,
    pub extended: Vec<Property>,
    pub partial: Option<Box<ManagementResult>>,
}

impl fmt::Display for ManagementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (0x{:08X})", self.message, self.code)
    }
}
impl std::error::Error for ManagementError {}
