mod request;
use crate::model::{ManagementError, ManagementResult};
pub use request::*;

pub trait Management {
    fn reachable(&mut self, _node: &crate::model::WideText, _timeout_ms: u32) -> bool {
        true
    }
    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError>;
}
mod convert;
pub use convert::*;
#[cfg(windows)]
pub mod windows;
