mod request;
use crate::model::{ManagementError, ManagementResult};
pub use request::*;

pub trait Management {
    fn load_aliases(
        &mut self,
        _role: &crate::model::WideText,
        _selected: Option<&crate::model::WideText>,
        _locale: &crate::model::WideText,
    ) -> Result<Option<Vec<crate::alias::AliasDefinition>>, crate::alias::ResolveError> {
        Ok(None)
    }
    fn reachable(&mut self, _node: &crate::model::WideText, _timeout_ms: u32) -> bool {
        true
    }
    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError>;
}
mod convert;
pub use convert::*;
#[cfg(windows)]
pub mod windows;
mod wql;
pub use wql::*;
