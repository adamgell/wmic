mod request;
use crate::model::{ManagementError, ManagementResult};
pub use request::*;

pub trait Management {
    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError>;
}
